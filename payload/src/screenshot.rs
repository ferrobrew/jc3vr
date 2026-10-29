//! On-demand screenshots to the session's screenshots (F12; see [`crate::session`]), for in-headset
//! diagnosis. Every capture shows both eyes side by side in a single PNG: under single-pass collapse
//! the linear back buffer *is* that side-by-side render and is captured directly, while in two-pass
//! stereo the back buffer only ever holds one eye, so the two per-eye capture textures the VR
//! presentation path submits are stitched left/right into one double-wide image instead (see
//! [`CaptureSource`]).
//!
//! Only the work that genuinely needs the immediate context happens here, inside the engine's
//! immediate-context critical section with the draw thread and the whole frame blocked behind it:
//! the staging copy, the map, and a copy of the rows out of the mapping. The conversion, the PNG
//! encode, and the file writes belong to the shared [`crate::image_writer`], which also owns the
//! file naming and the writer thread's shutdown.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, bail};
use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
        D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
    },
    core::Interface as _,
};

use crate::{
    image_writer::{self, ChannelOrder, Image, ImageTarget},
    stereo::single_pass::FrameDiagnostics,
};

/// What a capture reads its pixels from. Both variants produce one PNG containing both eyes; they
/// differ in whether the GPU already laid the eyes out side by side.
pub enum CaptureSource<'a> {
    /// A single texture that already holds the full frame: the collapse's side-by-side back buffer,
    /// or the flat back buffer outside stereo.
    Full(&'a ID3D11Resource),
    /// The two per-eye capture textures (left, right), stitched into one double-wide image. Both
    /// must hold the same frame -- the caller services the request on the second eye's dispatch.
    Eyes([&'a ID3D11Texture2D; 2]),
}

/// Request a screenshot on the next rendered frame. Called from the F12 wndproc handler; the actual
/// capture runs on the render thread in [`capture_if_requested`].
pub fn request() {
    REQUESTED.store(true, Ordering::Relaxed);
}

/// Whether a screenshot is waiting for a frame to capture.
pub fn is_requested() -> bool {
    REQUESTED.load(Ordering::Relaxed)
}

/// If a screenshot was requested, copy `source` to a CPU-readable staging texture (stitching the
/// per-eye textures side by side for [`CaptureSource::Eyes`]) and read its pixels back, then hand
/// them to the [`crate::image_writer`] with the frame's single-pass sidecar. Render thread, inside
/// the engine's context critical section; a no-op unless requested. Errors (unsupported format, map
/// failure) are logged, never propagated -- a screenshot must not take the frame down with it.
pub fn capture_if_requested(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: CaptureSource<'_>,
) {
    if !REQUESTED.swap(false, Ordering::Relaxed) {
        return;
    }
    match read_back(device, context, source) {
        Ok(capture) => submit(capture),
        Err(e) => tracing::error!("screenshot: {e:#}"),
    }
}

static REQUESTED: AtomicBool = AtomicBool::new(false);

/// One read-back frame and what its sidecar records about it.
struct Capture {
    image: Image,
    /// The capture's source format, for the sidecar.
    format: String,
    /// The capture's stamp, which names its file; taken on the render thread so the names follow
    /// the order the frames were captured in, not the order they are written in.
    stamp: String,
    /// The single-pass state of the captured frame, snapshotted here because by the time the image
    /// is written the engine is several frames on.
    diagnostics: Option<FrameDiagnostics>,
}

/// The render-thread half: staging copy (or two half-copies for the stitched form), map, memcpy
/// out. Everything here needs the immediate context, and nothing that does not is allowed in.
fn read_back(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: CaptureSource<'_>,
) -> anyhow::Result<Capture> {
    // SAFETY: called on the render thread with the live immediate device/context; the sources are
    // live textures whose GPU writes for this frame are complete. The staging copies + map read only
    // this frame's finished pixels.
    unsafe {
        match source {
            CaptureSource::Full(resource) => {
                let texture: ID3D11Texture2D = resource
                    .cast()
                    .context("the back buffer is not a Texture2D")?;
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                texture.GetDesc(&mut desc);
                let order = channel_order(&desc)?;
                let staging = create_staging(device, &desc)?;
                context.CopyResource(&staging, &texture);
                map_out(context, &staging, &desc, order)
            }
            CaptureSource::Eyes([left, right]) => {
                let mut left_desc = D3D11_TEXTURE2D_DESC::default();
                let mut right_desc = D3D11_TEXTURE2D_DESC::default();
                left.GetDesc(&mut left_desc);
                right.GetDesc(&mut right_desc);
                if (left_desc.Width, left_desc.Height, left_desc.Format)
                    != (right_desc.Width, right_desc.Height, right_desc.Format)
                {
                    bail!(
                        "the per-eye captures disagree: {}x{} {:?} vs {}x{} {:?}",
                        left_desc.Width,
                        left_desc.Height,
                        left_desc.Format,
                        right_desc.Width,
                        right_desc.Height,
                        right_desc.Format,
                    );
                }
                let order = channel_order(&left_desc)?;
                let staging_desc = D3D11_TEXTURE2D_DESC {
                    Width: left_desc.Width * 2,
                    ..left_desc
                };
                let staging = create_staging(device, &staging_desc)?;
                context.CopySubresourceRegion(&staging, 0, 0, 0, 0, left, 0, None);
                context.CopySubresourceRegion(&staging, 0, left_desc.Width, 0, 0, right, 0, None);
                map_out(context, &staging, &staging_desc, order)
            }
        }
    }
}

/// The channel order of `desc`'s format, or an error for anything but the 8-bit UNORM formats the
/// image writer converts.
fn channel_order(desc: &D3D11_TEXTURE2D_DESC) -> anyhow::Result<ChannelOrder> {
    ChannelOrder::of(desc.Format).with_context(|| {
        format!(
            "unsupported capture format {:?} (only 8-bit RGBA/BGRA handled)",
            desc.Format
        )
    })
}

/// A CPU-readable staging texture matching `desc`'s size and format.
unsafe fn create_staging(
    device: &ID3D11Device,
    desc: &D3D11_TEXTURE2D_DESC,
) -> anyhow::Result<ID3D11Texture2D> {
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        ..*desc
    };
    let mut staging: Option<ID3D11Texture2D> = None;
    // SAFETY: per the caller's contract -- the live device on the render thread.
    unsafe {
        device
            .CreateTexture2D(&staging_desc, None, Some(&mut staging))
            .context("creating the staging texture")?;
    }
    staging.context("the staging texture was not created")
}

/// Map `staging` and copy its rows out into an owned [`Capture`].
unsafe fn map_out(
    context: &ID3D11DeviceContext,
    staging: &ID3D11Texture2D,
    desc: &D3D11_TEXTURE2D_DESC,
    order: ChannelOrder,
) -> anyhow::Result<Capture> {
    // SAFETY: per the caller's contract -- the live immediate context, and a staging texture the
    // copies into which were issued on this same context. The mapping spans `RowPitch` bytes per row
    // for `Height` rows, and is read only before the unmap.
    let image = unsafe {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .context("mapping the staging texture")?;
        let row_pitch = mapped.RowPitch as usize;
        let bytes =
            std::slice::from_raw_parts(mapped.pData.cast::<u8>(), row_pitch * desc.Height as usize);
        let image = Image::from_rows(bytes, row_pitch, desc.Width, desc.Height, order);
        context.Unmap(staging, 0);
        image
    }
    .context("the mapped rows are shorter than the texture")?;
    Ok(Capture {
        image,
        format: format!("{:?}", desc.Format),
        stamp: crate::session::stamp(),
        diagnostics: crate::stereo::single_pass::last_frame_diagnostics(),
    })
}

/// Hand `capture` to the image writer with its sidecar: everything relevant about the frame's
/// single-pass state (the per-eye matrices, the centre transform, the viewport, the config), so the
/// exact matrices that produced the image can be inspected offline instead of read from a log line.
fn submit(capture: Capture) {
    let sidecar = serde_json::json!({
        "image": crate::session::CaptureFile::Screenshot.file_name(&capture.stamp, None).ok(),
        "width": capture.image.width(),
        "height": capture.image.height(),
        "format": capture.format,
        "single_pass": serde_json::to_value(&capture.diagnostics)
            .unwrap_or(serde_json::Value::Null),
    });
    image_writer::submit(
        ImageTarget::Screenshot {
            stamp: capture.stamp,
            sidecar,
        },
        capture.image,
    );
}
