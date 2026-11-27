//! Dumps of compared pixels: the render-thread readback of a cropped region of a snapshot plane,
//! and the writer-thread encode (PNG for 8-bit images, gzip-compressed raw pixels otherwise).

use std::{io::Write as _, path::Path};

use anyhow::{Context as _, bail};
use flate2::{Compression, write::GzEncoder};
use jc3vr_render::readback::{read_mapped, staging_texture};
use serde::Serialize;
use windows::Win32::Graphics::{
    Direct3D11::{D3D11_BOX, D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11DeviceContext},
    Dxgi::Common::DXGI_SAMPLE_DESC,
};

use crate::screenshot::harness::snapshot::{
    store::{Format, PlaneTexture},
    summary::Kind,
};

/// A cropped region of a snapshot plane, read back.
pub(crate) struct DumpImage {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    /// The crop's top-left pixel within the eye.
    origin: [u32; 2],
    format: Format,
    /// Whether the image is written as a PNG (the final image) rather than raw pixels (a stage, whose
    /// every channel, alpha included, counts).
    png: bool,
}

impl DumpImage {
    /// Read `region` (`left, top, width, height`, in eye pixels) of the eye at `origin` in
    /// `plane`, to be written as a PNG when `png` (only for 8-bit images).
    ///
    /// # Safety
    /// Render thread, with the engine's immediate context locked; `plane` is live.
    pub(crate) unsafe fn read(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        plane: &PlaneTexture,
        origin: [u32; 2],
        region: (u32, u32, u32, u32),
        png: bool,
    ) -> anyhow::Result<Self> {
        let (left, top, width, height) = region;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: plane.format.storage,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            ..Default::default()
        };
        let row_bytes = (width * plane.format.pixel_bytes) as usize;
        // SAFETY: per the caller's contract; the staging texture is ours and the box lies within
        // the plane.
        unsafe {
            let staging = staging_texture(device, &desc).context("creating the dump readback")?;
            context.CopySubresourceRegion(
                &staging,
                0,
                0,
                0,
                0,
                &plane.texture,
                0,
                Some(&D3D11_BOX {
                    left: origin[0] + left,
                    top: origin[1] + top,
                    front: 0,
                    right: origin[0] + left + width,
                    bottom: origin[1] + top + height,
                    back: 1,
                }),
            );
            let bytes = read_mapped(context, &staging, |mapped| {
                if mapped.row_pitch() < row_bytes {
                    bail!("the dump readback mapped too little");
                }
                let mut bytes = Vec::with_capacity(row_bytes * height as usize);
                for y in 0..height as usize {
                    bytes.extend_from_slice(mapped.row(y, row_bytes));
                }
                Ok(bytes)
            })
            .context("mapping the dump readback")??;
            Ok(Self {
                bytes,
                width,
                height,
                origin: [left, top],
                format: plane.format,
                png: png && plane.format.kind == Kind::Unorm8,
            })
        }
    }
}

/// What a comparison's dump wrote, for its acknowledgement.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct DumpRecord {
    /// The comparison's file stem: `<against>.<plane>.eye<N>`.
    pub stem: String,
    /// The reference (the kept snapshot) and the test (this snapshot) files, relative to the
    /// captures folder.
    pub reference: String,
    pub test: String,
    /// `png` (the final image, RGB with opaque alpha) or `raw.gz` (a stage's pixels as stored, row
    /// by row, gzip-compressed).
    pub encoding: &'static str,
    /// The crop's top-left pixel within the eye, and its size.
    pub origin: [u32; 2],
    pub width: u32,
    pub height: u32,
    /// The stored DXGI format and bytes per pixel, for decoding a raw dump.
    pub format: i32,
    pub pixel_bytes: u32,
    pub error: Option<String>,
}

/// Write each comparison's pair of dumps into `folder`. Writer thread.
pub(crate) fn write(folder: &Path, dumps: Vec<(String, [DumpImage; 2])>) -> Vec<DumpRecord> {
    if let Err(error) = std::fs::create_dir_all(folder) {
        tracing::error!("snapshot: creating {}: {error}", folder.display());
    }
    let folder_name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    dumps
        .into_iter()
        .map(|(stem, [reference, test])| {
            let png = reference.png;
            let encoding = if png { "png" } else { "raw.gz" };
            let names = [
                format!("{stem}.reference.{encoding}"),
                format!("{stem}.test.{encoding}"),
            ];
            let error = [&reference, &test]
                .into_iter()
                .zip(&names)
                .map(|(image, name)| encode(&folder.join(name), image, png))
                .find_map(Result::err)
                .map(|error| format!("snapshot: {error:#}"));
            DumpRecord {
                stem,
                reference: format!("{folder_name}/{}", names[0]),
                test: format!("{folder_name}/{}", names[1]),
                encoding,
                origin: reference.origin,
                width: reference.width,
                height: reference.height,
                format: reference.format.storage.0,
                pixel_bytes: reference.format.pixel_bytes,
                error,
            }
        })
        .collect()
}

/// What a saved plane wrote, for the acknowledgement.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SavedRecord {
    /// `<label>.<plane>.eye<N>`.
    pub stem: String,
    /// The file, relative to the captures folder.
    pub file: String,
    /// `png` or `raw.gz`, as for a comparison's dump.
    pub encoding: &'static str,
    pub width: u32,
    pub height: u32,
    /// The stored DXGI format and bytes per pixel, for decoding a raw file.
    pub format: i32,
    pub pixel_bytes: u32,
    pub error: Option<String>,
}

/// Write each saved plane into `folder`. Writer thread.
pub(crate) fn write_saved(folder: &Path, saves: Vec<(String, DumpImage)>) -> Vec<SavedRecord> {
    if let Err(error) = std::fs::create_dir_all(folder) {
        tracing::error!("snapshot: creating {}: {error}", folder.display());
    }
    let folder_name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    saves
        .into_iter()
        .map(|(stem, image)| {
            let encoding = if image.png { "png" } else { "raw.gz" };
            let name = format!("{stem}.{encoding}");
            let error = encode(&folder.join(&name), &image, image.png)
                .err()
                .map(|error| format!("snapshot: {error:#}"));
            SavedRecord {
                stem,
                file: format!("{folder_name}/{name}"),
                encoding,
                width: image.width,
                height: image.height,
                format: image.format.storage.0,
                pixel_bytes: image.format.pixel_bytes,
                error,
            }
        })
        .collect()
}

fn encode(path: &Path, image: &DumpImage, png: bool) -> anyhow::Result<()> {
    if png {
        let mut rgba = image.bytes.clone();
        if image.format.bgra {
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        image::save_buffer(
            path,
            &rgba,
            image.width,
            image.height,
            image::ExtendedColorType::Rgba8,
        )
        .with_context(|| format!("writing {}", path.display()))
    } else {
        let file =
            std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
        let mut encoder = GzEncoder::new(std::io::BufWriter::new(file), Compression::fast());
        encoder
            .write_all(&image.bytes)
            .and_then(|()| encoder.finish().map(drop))
            .with_context(|| format!("writing {}", path.display()))
    }
}
