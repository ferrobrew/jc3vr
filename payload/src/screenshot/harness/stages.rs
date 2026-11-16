//! One-frame post-chain measurements for the screenshot harness.

use std::{ffi::c_void, path::PathBuf};

use anyhow::{Context as _, bail};
use jc3gi::graphics_engine::{
    graphics_engine::GraphicsEngine, post_effects::PostEffectsManager, texture::Texture,
};
use parking_lot::Mutex;
use serde::Serialize;
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct3D11::{
                D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_RASTERIZER_DESC,
                D3D11_TEXTURE2D_DESC, D3D11_VIEWPORT, ID3D11DeviceContext, ID3D11Texture2D,
            },
            Dxgi::Common::{
                DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_TYPELESS,
                DXGI_FORMAT_R11G11B10_FLOAT,
            },
        },
    },
    core::Interface as _,
};

use crate::{
    image_writer::{ChannelOrder, Image},
    screenshot::{create_staging, harness::wants_eyes},
    stereo::{draw_index, engine_context::EngineContext},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    MainColor,
    PreAa,
    PostAa,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StageRecord {
    pub frame: i64,
    pub stage: Stage,
    pub eye: usize,
    pub source: Option<Source>,
    pub viewports: Vec<Viewport>,
    pub scissors: Vec<Scissor>,
    pub scissor_enabled: Option<bool>,
    pub means: Option<SplitMean>,
    pub png: Option<PathBuf>,
    pub raw: Option<PathBuf>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Source {
    pub width: u32,
    pub height: u32,
    pub format: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Scissor {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// RGB means over every pixel in each horizontal half, excluding row padding and alpha.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct SplitMean {
    pub left: [f64; 3],
    pub right: [f64; 3],
}

pub(crate) fn capture_main_color() {
    capture(Stage::MainColor, || {
        // SAFETY: the post-chain hook runs on the render thread with live engine textures.
        unsafe { GraphicsEngine::get() }
            .map(|ge| ge.m_MainColorBuffer)
            .context("MainColor unavailable")
    });
}

pub(crate) fn capture_slot(stage: Stage, mgr: *mut c_void, slot: u32) {
    capture(stage, || {
        if mgr.is_null() {
            bail!("post-effects manager unavailable");
        }
        // SAFETY: the existing post-chain hook supplies the manager and its current result slot.
        Ok(unsafe {
            (mgr as *const *mut Texture)
                .add(slot as usize + PostEffectsManager::RESULT_TEXTURE_SLOT_BASE as usize)
                .read()
        })
    });
}

pub(crate) struct StageImage {
    pub record: usize,
    pub artifact: StageArtifact,
}

pub(crate) enum StageArtifact {
    Png(Image),
    R11G11B10(Vec<u8>),
}

pub(crate) fn take(frame: i64) -> (Vec<StageRecord>, Vec<StageImage>) {
    let mut state = STATE.lock();
    if state.frame != Some(frame) {
        return (Vec::new(), Vec::new());
    }
    state.frame = None;
    (
        std::mem::take(&mut state.records),
        std::mem::take(&mut state.images),
    )
}

#[derive(Default)]
struct State {
    frame: Option<i64>,
    records: Vec<StageRecord>,
    images: Vec<StageImage>,
}

static STATE: Mutex<State> = Mutex::new(State {
    frame: None,
    records: Vec::new(),
    images: Vec::new(),
});

fn capture(stage: Stage, texture: impl FnOnce() -> anyhow::Result<*mut Texture>) {
    if !wants_eyes() || !crate::testing::visual::ready() {
        return;
    }
    let Some(frame) = crate::vr::capture_views().map(|v| v.predicted_display_time_ns) else {
        return;
    };
    let eye = draw_index();
    let mut state = STATE.lock();
    if state.frame != Some(frame) {
        state.frame = Some(frame);
        state.records.clear();
        state.images.clear();
    }
    if state
        .records
        .iter()
        .any(|r| r.stage == stage && r.eye == eye)
    {
        return;
    }
    let mut record = StageRecord {
        frame,
        stage,
        eye,
        source: None,
        viewports: Vec::new(),
        scissors: Vec::new(),
        scissor_enabled: None,
        means: None,
        png: None,
        raw: None,
        error: None,
    };
    let result = (|| {
        let context = EngineContext::get().context("graphics context unavailable")?;
        context.with_lock(|ctx| {
            capture_raster_state(ctx, &mut record);
            let texture = texture()?;
            // SAFETY: supplied by the live post-chain hook while the immediate context is locked.
            let texture = unsafe { texture.as_ref() }.context("stage texture unavailable")?;
            read_stage(ctx, texture, &mut record)
        })
    })();
    match result {
        Ok(artifact) => {
            let index = state.records.len();
            state.images.push(StageImage {
                record: index,
                artifact,
            });
        }
        Err(error) => record.error = Some(format!("screenshot stages: {error:#}")),
    }
    state.records.push(record);
}

fn capture_raster_state(ctx: &ID3D11DeviceContext, record: &mut StageRecord) {
    // SAFETY: the immediate context is locked; each count matches its writable array.
    unsafe {
        let mut count = 0;
        ctx.RSGetViewports(&mut count, None);
        let mut slots = vec![D3D11_VIEWPORT::default(); count as usize];
        if count > 0 {
            ctx.RSGetViewports(&mut count, Some(slots.as_mut_ptr()));
        }
        record.viewports = slots
            .into_iter()
            .take(count as usize)
            .map(|v| Viewport {
                x: v.TopLeftX,
                y: v.TopLeftY,
                width: v.Width,
                height: v.Height,
                min_depth: v.MinDepth,
                max_depth: v.MaxDepth,
            })
            .collect();
        ctx.RSGetScissorRects(&mut count, None);
        let mut slots = vec![RECT::default(); count as usize];
        if count > 0 {
            ctx.RSGetScissorRects(&mut count, Some(slots.as_mut_ptr()));
        }
        record.scissors = slots
            .into_iter()
            .take(count as usize)
            .map(|r| Scissor {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            })
            .collect();
        if let Ok(state) = ctx.RSGetState() {
            let mut desc = D3D11_RASTERIZER_DESC::default();
            state.GetDesc(&mut desc);
            record.scissor_enabled = Some(desc.ScissorEnable.as_bool());
        } else {
            // The D3D11 default rasterizer disables scissoring.
            record.scissor_enabled = Some(false);
        }
    }
}

fn read_stage(
    ctx: &ID3D11DeviceContext,
    texture: &Texture,
    record: &mut StageRecord,
) -> anyhow::Result<StageArtifact> {
    let texture: ID3D11Texture2D = texture
        .m_Texture
        .cast()
        .context("stage texture is not 2D")?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: the texture and context are live under the engine context lock.
    unsafe { texture.GetDesc(&mut desc) };
    record.source = Some(Source {
        width: desc.Width,
        height: desc.Height,
        format: format!("{:?}", desc.Format),
    });
    if desc.SampleDesc.Count != 1 {
        bail!(
            "unsupported stage sample count {} (only single-sampled textures are supported)",
            desc.SampleDesc.Count
        );
    }
    if desc.ArraySize != 1 {
        bail!("unsupported stage array size {}", desc.ArraySize);
    }
    if stage_channel_order(desc.Format).is_none() && desc.Format != DXGI_FORMAT_R11G11B10_FLOAT {
        bail!("unsupported stage format {:?}", desc.Format);
    }
    // SAFETY: the engine device is live on the render thread.
    let ge = unsafe { GraphicsEngine::get() }.context("graphics engine unavailable")?;
    let device = unsafe { ge.m_Device.as_ref() }.context("graphics device unavailable")?;
    // SAFETY: the context is locked, and the copy and mapping use its live device.
    unsafe {
        let staging = create_staging(&device.m_Device, &desc)?;
        ctx.CopyResource(&staging, &texture);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .context("mapping stage texture")?;
        let bytes = std::slice::from_raw_parts(
            mapped.pData.cast::<u8>(),
            mapped.RowPitch as usize * desc.Height as usize,
        );
        let result = {
            match split_mean(
                bytes,
                mapped.RowPitch as usize,
                desc.Width,
                desc.Height,
                desc.Format,
            ) {
                Ok(means) => record.means = Some(means),
                Err(error) => record.error = Some(format!("screenshot stages: {error:#}")),
            }
            stage_image(record.stage, bytes, mapped.RowPitch as usize, &desc)
        };
        ctx.Unmap(&staging, 0);
        result
    }
}

fn stage_image(
    stage: Stage,
    bytes: &[u8],
    pitch: usize,
    desc: &D3D11_TEXTURE2D_DESC,
) -> anyhow::Result<StageArtifact> {
    if stage == Stage::MainColor {
        if desc.Format != DXGI_FORMAT_R11G11B10_FLOAT {
            bail!("unsupported MainColor raw format {:?}", desc.Format);
        }
        let row_bytes = desc.Width as usize * 4;
        if desc.Width == 0 || desc.Height == 0 || pitch < row_bytes {
            bail!("invalid MainColor raw dimensions");
        }
        let mut packed = Vec::with_capacity(row_bytes * desc.Height as usize);
        for y in 0..desc.Height as usize {
            packed.extend_from_slice(
                bytes
                    .get(y * pitch..y * pitch + row_bytes)
                    .context("short MainColor raw row")?,
            );
        }
        return Ok(StageArtifact::R11G11B10(packed));
    }
    let order = stage_channel_order(desc.Format)
        .with_context(|| format!("unsupported stage PNG format {:?}", desc.Format))?;
    Image::from_rows(bytes, pitch, desc.Width, desc.Height, order)
        .context("invalid stage PNG rows")
        .map(StageArtifact::Png)
}

fn split_mean(
    bytes: &[u8],
    pitch: usize,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
) -> anyhow::Result<SplitMean> {
    if width < 2 || height == 0 || pitch < width as usize * 4 {
        bail!("invalid split-mean texture dimensions");
    }
    let order = stage_channel_order(format);
    if order.is_none() && format != DXGI_FORMAT_R11G11B10_FLOAT {
        bail!("unsupported split-mean format {format:?}");
    }
    let mut sums = [[0.0; 3]; 2];
    let mut counts = [0u64; 2];
    for y in 0..height as usize {
        let row = bytes
            .get(y * pitch..y * pitch + width as usize * 4)
            .context("short stage row")?;
        for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
            let rgb = match order {
                Some(ChannelOrder::Rgba) => [
                    pixel[0] as f64 / 255.0,
                    pixel[1] as f64 / 255.0,
                    pixel[2] as f64 / 255.0,
                ],
                Some(ChannelOrder::Bgra) => [
                    pixel[2] as f64 / 255.0,
                    pixel[1] as f64 / 255.0,
                    pixel[0] as f64 / 255.0,
                ],
                None => decode_r11g11b10(u32::from_le_bytes(*pixel)),
            };
            let half = usize::from(x >= width as usize / 2);
            for (sum, value) in sums[half].iter_mut().zip(rgb) {
                if !value.is_finite() {
                    bail!("non-finite stage channel");
                }
                *sum += value;
            }
            counts[half] += 1;
        }
    }
    for (sum, count) in sums.iter_mut().zip(counts) {
        for channel in sum {
            *channel /= count as f64;
        }
    }
    Ok(SplitMean {
        left: sums[0],
        right: sums[1],
    })
}

fn stage_channel_order(format: DXGI_FORMAT) -> Option<ChannelOrder> {
    match format {
        DXGI_FORMAT_R8G8B8A8_TYPELESS => Some(ChannelOrder::Rgba),
        DXGI_FORMAT_B8G8R8A8_TYPELESS => Some(ChannelOrder::Bgra),
        _ => ChannelOrder::of(format),
    }
}

fn decode_r11g11b10(pixel: u32) -> [f64; 3] {
    fn channel(bits: u32, mantissa: u32) -> f64 {
        let exponent = bits >> mantissa;
        let fraction = (bits & ((1 << mantissa) - 1)) as f64 / (1u32 << mantissa) as f64;
        match exponent {
            0 => fraction * 2f64.powi(-14),
            31 => f64::NAN,
            _ => (1.0 + fraction) * 2f64.powi(exponent as i32 - 15),
        }
    }
    [
        channel(pixel & 0x7ff, 6),
        channel((pixel >> 11) & 0x7ff, 6),
        channel(pixel >> 22, 5),
    ]
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

    use super::*;

    #[test]
    fn split_means_exclude_padding_and_alpha_and_handle_odd_widths() {
        let bytes = [
            0, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 0, 255, 255, 255, 255,
        ];
        assert_eq!(
            split_mean(&bytes, 16, 3, 1, DXGI_FORMAT_B8G8R8A8_UNORM).unwrap(),
            SplitMean {
                left: [1.0, 0.0, 0.0],
                right: [0.0, 0.0, 1.0],
            }
        );
        assert!(split_mean(&bytes[..8], 16, 3, 1, DXGI_FORMAT_B8G8R8A8_UNORM).is_err());
    }

    #[test]
    fn typeless_stage_means_preserve_byte_channel_order() {
        for (format, expected) in [
            (DXGI_FORMAT_R8G8B8A8_TYPELESS, [1.0, 0.0, 0.0]),
            (DXGI_FORMAT_B8G8R8A8_TYPELESS, [0.0, 0.0, 1.0]),
        ] {
            assert_eq!(
                split_mean(&[255, 0, 0, 255, 0, 0, 0, 255], 8, 2, 1, format).unwrap(),
                SplitMean {
                    left: expected,
                    right: [0.0; 3],
                }
            );
        }
    }

    #[test]
    fn raw_hdr_artifact_preserves_every_bit_without_row_padding() {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: 2,
            Height: 2,
            Format: DXGI_FORMAT_R11G11B10_FLOAT,
            ..Default::default()
        };
        let bytes: Vec<u8> = (0..24).collect();
        let StageArtifact::R11G11B10(packed) =
            stage_image(Stage::MainColor, &bytes, 12, &desc).unwrap()
        else {
            panic!("expected raw HDR");
        };
        assert_eq!(packed, [&bytes[..8], &bytes[12..20]].concat());
        assert!(stage_image(Stage::MainColor, &bytes[..19], 12, &desc).is_err());
        assert!(stage_image(Stage::MainColor, &bytes, 4, &desc).is_err());
        assert!(stage_image(Stage::PreAa, &bytes, 12, &desc).is_err());
    }

    #[test]
    fn aa_artifacts_accept_rgba_and_bgra_typeless_rows() {
        for format in [DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_B8G8R8A8_TYPELESS] {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: 2,
                Height: 2,
                Format: format,
                ..Default::default()
            };
            for stage in [Stage::PreAa, Stage::PostAa] {
                let StageArtifact::Png(image) = stage_image(stage, &[0; 24], 12, &desc).unwrap()
                else {
                    panic!("expected PNG");
                };
                assert_eq!((image.width(), image.height()), (2, 2));
                assert!(stage_image(stage, &[0; 19], 12, &desc).is_err());
            }
        }
    }

    #[test]
    fn hdr_mean_retains_linear_values_and_black_half() {
        let one = (15u32 << 6) | ((15u32 << 6) << 11) | ((15u32 << 5) << 22);
        let mut bytes = one.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0; 4]);
        assert_eq!(
            split_mean(&bytes, 8, 2, 1, DXGI_FORMAT_R11G11B10_FLOAT).unwrap(),
            SplitMean {
                left: [1.0; 3],
                right: [0.0; 3],
            }
        );
        assert_eq!(decode_r11g11b10(1)[0], 2f64.powi(-20));
        assert!(decode_r11g11b10(31 << 6)[0].is_nan());
    }
}
