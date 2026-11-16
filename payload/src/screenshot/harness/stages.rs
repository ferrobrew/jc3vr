//! One-frame geometry and post-chain captures for the screenshot harness.

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
                DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_TYPELESS, DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_D16_UNORM,
                DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_D32_FLOAT,
                DXGI_FORMAT_D32_FLOAT_S8X24_UINT, DXGI_FORMAT_R8G8B8A8_SINT,
                DXGI_FORMAT_R8G8B8A8_SNORM, DXGI_FORMAT_R8G8B8A8_TYPELESS,
                DXGI_FORMAT_R8G8B8A8_UINT, DXGI_FORMAT_R8G8B8A8_UNORM,
                DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_FORMAT_R10G10B10A2_TYPELESS,
                DXGI_FORMAT_R10G10B10A2_UINT, DXGI_FORMAT_R10G10B10A2_UNORM,
                DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R16_TYPELESS,
                DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R16G16B16A16_SINT,
                DXGI_FORMAT_R16G16B16A16_SNORM, DXGI_FORMAT_R16G16B16A16_TYPELESS,
                DXGI_FORMAT_R16G16B16A16_UINT, DXGI_FORMAT_R16G16B16A16_UNORM,
                DXGI_FORMAT_R24G8_TYPELESS, DXGI_FORMAT_R32_TYPELESS,
                DXGI_FORMAT_R32G8X24_TYPELESS,
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
    GeometryDepth,
    #[serde(rename = "gbuffer0")]
    GBuffer0,
    #[serde(rename = "gbuffer1")]
    GBuffer1,
    #[serde(rename = "gbuffer2")]
    GBuffer2,
    #[serde(rename = "gbuffer3")]
    GBuffer3,
    MainColor,
    PreAtmosphere,
    PostAtmosphere,
    PostResolve,
    PreAa,
    PostAa,
}

impl Stage {
    pub(crate) fn is_hdr(self) -> bool {
        matches!(
            self,
            Self::MainColor | Self::PreAtmosphere | Self::PostAtmosphere | Self::PostResolve
        )
    }
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
    pub format_id: i32,
    pub bytes_per_pixel: Option<usize>,
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

pub(crate) fn capture_geometry() {
    for (stage, index) in [
        (Stage::GeometryDepth, None),
        (Stage::GBuffer0, Some(0)),
        (Stage::GBuffer1, Some(1)),
        (Stage::GBuffer2, Some(2)),
        (Stage::GBuffer3, Some(3)),
    ] {
        capture(stage, || {
            // SAFETY: the geometry range hook runs on the render thread with live textures.
            let ge = unsafe { GraphicsEngine::get() }.context("graphics engine unavailable")?;
            Ok(index.map_or(ge.m_MainDepthTexture, |i| ge.m_GBufferTexture[i]))
        });
    }
}

pub(crate) fn capture_main_color() {
    capture_hdr(HdrStage::MainColor);
}

pub(crate) fn capture_before_atmosphere() {
    capture_hdr(HdrStage::PreAtmosphere);
}

pub(crate) fn capture_after_atmosphere() {
    capture_hdr(HdrStage::PostAtmosphere);
}

pub(crate) fn capture_after_resolve() {
    capture_hdr(HdrStage::PostResolve);
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
    Raw(Vec<u8>),
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

#[derive(Debug, Clone, Copy)]
enum HdrStage {
    MainColor,
    PreAtmosphere,
    PostAtmosphere,
    PostResolve,
}

impl HdrStage {
    fn stage(self) -> Stage {
        match self {
            Self::MainColor => Stage::MainColor,
            Self::PreAtmosphere => Stage::PreAtmosphere,
            Self::PostAtmosphere => Stage::PostAtmosphere,
            Self::PostResolve => Stage::PostResolve,
        }
    }
}

fn capture_hdr(stage: HdrStage) {
    capture(stage.stage(), || {
        // SAFETY: the render hook runs on the render thread with live engine textures.
        unsafe { GraphicsEngine::get() }
            .map(|ge| ge.m_MainColorBuffer)
            .context("MainColor unavailable")
    });
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
            // SAFETY: supplied by a live render hook while the immediate context is locked.
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
        format_id: desc.Format.0,
        bytes_per_pixel: format_byte_width(desc.Format),
    });
    validate_desc(&desc)?;
    // SAFETY: the engine device is live on the render thread.
    let ge = unsafe { GraphicsEngine::get() }.context("graphics engine unavailable")?;
    let device = unsafe { ge.m_Device.as_ref() }.context("graphics device unavailable")?;
    let staging_desc = D3D11_TEXTURE2D_DESC {
        // Depth staging uses a copy-compatible typeless family, never a depth view format.
        Format: staging_format(desc.Format),
        ..desc
    };
    // SAFETY: the context is locked, and the copy and mapping use its live device.
    unsafe {
        let staging = create_staging(&device.m_Device, &staging_desc)?;
        ctx.CopyResource(&staging, &texture);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .context("mapping stage texture")?;
        let result = (|| {
            let len = (mapped.RowPitch as usize)
                .checked_mul(desc.Height as usize)
                .filter(|len| *len <= isize::MAX as usize)
                .context("stage mapping length overflow")?;
            if mapped.pData.is_null() {
                bail!("stage mapping has no data");
            }
            let bytes = std::slice::from_raw_parts(mapped.pData.cast::<u8>(), len);
            match stage_means(bytes, mapped.RowPitch as usize, &desc) {
                Ok(means) => record.means = means,
                Err(error) => record.error = Some(format!("screenshot stages: {error:#}")),
            }
            stage_image(record.stage, bytes, mapped.RowPitch as usize, &desc)
        })();
        ctx.Unmap(&staging, 0);
        result
    }
}

fn validate_desc(desc: &D3D11_TEXTURE2D_DESC) -> anyhow::Result<()> {
    if desc.Width == 0 || desc.Height == 0 {
        bail!("invalid stage texture dimensions");
    }
    if desc.SampleDesc.Count != 1 {
        bail!(
            "unsupported stage sample count {} (only single-sampled textures are supported)",
            desc.SampleDesc.Count
        );
    }
    if desc.ArraySize != 1 {
        bail!("unsupported stage array size {}", desc.ArraySize);
    }
    if format_byte_width(desc.Format).is_none() {
        bail!("unsupported stage format {:?}", desc.Format);
    }
    Ok(())
}

fn staging_format(format: DXGI_FORMAT) -> DXGI_FORMAT {
    match format {
        DXGI_FORMAT_D32_FLOAT_S8X24_UINT => DXGI_FORMAT_R32G8X24_TYPELESS,
        DXGI_FORMAT_D24_UNORM_S8_UINT => DXGI_FORMAT_R24G8_TYPELESS,
        DXGI_FORMAT_D32_FLOAT => DXGI_FORMAT_R32_TYPELESS,
        DXGI_FORMAT_D16_UNORM => DXGI_FORMAT_R16_TYPELESS,
        _ => format,
    }
}

fn format_byte_width(format: DXGI_FORMAT) -> Option<usize> {
    match format {
        DXGI_FORMAT_D16_UNORM | DXGI_FORMAT_R16_TYPELESS => Some(2),
        DXGI_FORMAT_R8G8B8A8_TYPELESS
        | DXGI_FORMAT_R8G8B8A8_UNORM
        | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        | DXGI_FORMAT_R8G8B8A8_UINT
        | DXGI_FORMAT_R8G8B8A8_SNORM
        | DXGI_FORMAT_R8G8B8A8_SINT
        | DXGI_FORMAT_B8G8R8A8_TYPELESS
        | DXGI_FORMAT_B8G8R8A8_UNORM
        | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        | DXGI_FORMAT_R10G10B10A2_TYPELESS
        | DXGI_FORMAT_R10G10B10A2_UNORM
        | DXGI_FORMAT_R10G10B10A2_UINT
        | DXGI_FORMAT_R11G11B10_FLOAT
        | DXGI_FORMAT_D24_UNORM_S8_UINT
        | DXGI_FORMAT_R24G8_TYPELESS
        | DXGI_FORMAT_D32_FLOAT
        | DXGI_FORMAT_R32_TYPELESS => Some(4),
        DXGI_FORMAT_D32_FLOAT_S8X24_UINT
        | DXGI_FORMAT_R32G8X24_TYPELESS
        | DXGI_FORMAT_R16G16B16A16_TYPELESS
        | DXGI_FORMAT_R16G16B16A16_FLOAT
        | DXGI_FORMAT_R16G16B16A16_UNORM
        | DXGI_FORMAT_R16G16B16A16_UINT
        | DXGI_FORMAT_R16G16B16A16_SNORM
        | DXGI_FORMAT_R16G16B16A16_SINT => Some(8),
        _ => None,
    }
}

fn stage_image(
    stage: Stage,
    bytes: &[u8],
    pitch: usize,
    desc: &D3D11_TEXTURE2D_DESC,
) -> anyhow::Result<StageArtifact> {
    if !matches!(stage, Stage::PreAa | Stage::PostAa) {
        if stage.is_hdr() && desc.Format != DXGI_FORMAT_R11G11B10_FLOAT {
            bail!("unsupported {stage:?} raw format {:?}", desc.Format);
        }
        let pixel_bytes = format_byte_width(desc.Format)
            .with_context(|| format!("unsupported stage raw format {:?}", desc.Format))?;
        let row_bytes = (desc.Width as usize)
            .checked_mul(pixel_bytes)
            .context("stage raw row size overflow")?;
        if desc.Width == 0 || desc.Height == 0 || pitch < row_bytes {
            bail!("invalid stage raw dimensions");
        }
        let required = (desc.Height as usize - 1)
            .checked_mul(pitch)
            .and_then(|offset| offset.checked_add(row_bytes))
            .context("stage raw mapping size overflow")?;
        if bytes.len() < required {
            bail!("short stage raw row");
        }
        let mut packed = Vec::with_capacity(row_bytes * desc.Height as usize);
        for y in 0..desc.Height as usize {
            packed.extend_from_slice(&bytes[y * pitch..y * pitch + row_bytes]);
        }
        return Ok(StageArtifact::Raw(packed));
    }
    let order = stage_channel_order(desc.Format)
        .with_context(|| format!("unsupported stage PNG format {:?}", desc.Format))?;
    Image::from_rows(bytes, pitch, desc.Width, desc.Height, order)
        .context("invalid stage PNG rows")
        .map(StageArtifact::Png)
}

fn stage_means(
    bytes: &[u8],
    pitch: usize,
    desc: &D3D11_TEXTURE2D_DESC,
) -> anyhow::Result<Option<SplitMean>> {
    // Depth and packed material data have no RGB interpretation; a single column has no split.
    if desc.Width < 2
        || (stage_channel_order(desc.Format).is_none()
            && desc.Format != DXGI_FORMAT_R11G11B10_FLOAT)
    {
        return Ok(None);
    }
    split_mean(bytes, pitch, desc.Width, desc.Height, desc.Format).map(Some)
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
        let wrong_format = D3D11_TEXTURE2D_DESC {
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            ..desc
        };
        for stage in [
            Stage::MainColor,
            Stage::PreAtmosphere,
            Stage::PostAtmosphere,
            Stage::PostResolve,
        ] {
            let StageArtifact::Raw(packed) = stage_image(stage, &bytes, 12, &desc).unwrap() else {
                panic!("expected raw HDR");
            };
            assert_eq!(packed, [&bytes[..8], &bytes[12..20]].concat());
            assert!(stage_image(stage, &bytes[..19], 12, &desc).is_err());
            assert!(stage_image(stage, &bytes, 4, &desc).is_err());
            assert!(stage_image(stage, &bytes, 12, &wrong_format).is_err());
        }
        assert!(stage_image(Stage::PreAa, &bytes, 12, &desc).is_err());
    }

    #[test]
    fn atmosphere_stages_serialize_as_snake_case_and_are_hdr() {
        for (stage, name) in [
            (Stage::MainColor, "main_color"),
            (Stage::PreAtmosphere, "pre_atmosphere"),
            (Stage::PostAtmosphere, "post_atmosphere"),
            (Stage::PostResolve, "post_resolve"),
        ] {
            assert_eq!(serde_json::to_value(stage).unwrap(), name);
            assert!(stage.is_hdr());
        }
        for stage in [Stage::GeometryDepth, Stage::PreAa, Stage::PostAa] {
            assert!(!stage.is_hdr());
        }
    }

    #[test]
    fn geometry_raw_rows_preserve_depth_and_material_bits() {
        for (stage, format, pixel_bytes) in [
            (Stage::GeometryDepth, DXGI_FORMAT_R32G8X24_TYPELESS, 8),
            (Stage::GeometryDepth, DXGI_FORMAT_D32_FLOAT_S8X24_UINT, 8),
            (Stage::GBuffer0, DXGI_FORMAT_R8G8B8A8_TYPELESS, 4),
            (Stage::GBuffer1, DXGI_FORMAT_R10G10B10A2_UNORM, 4),
            (Stage::GBuffer2, DXGI_FORMAT_R16G16B16A16_FLOAT, 8),
            (Stage::GBuffer3, DXGI_FORMAT_B8G8R8A8_UNORM, 4),
        ] {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: 2,
                Height: 2,
                Format: format,
                ..Default::default()
            };
            let row_bytes = 2 * pixel_bytes;
            let pitch = row_bytes + 4;
            let bytes: Vec<u8> = (0..2 * pitch as u8).collect();
            let end = pitch + row_bytes;
            let StageArtifact::Raw(packed) =
                stage_image(stage, &bytes[..end], pitch, &desc).unwrap()
            else {
                panic!("expected raw geometry");
            };
            assert_eq!(packed, [&bytes[..row_bytes], &bytes[pitch..end]].concat());
            assert_eq!(packed[3], 3);
            assert_eq!(packed[row_bytes + 3], (pitch + 3) as u8);
            assert!(stage_image(stage, &bytes[..end - 1], pitch, &desc).is_err());
            assert!(stage_image(stage, &bytes, row_bytes - 1, &desc).is_err());
        }
    }

    #[test]
    fn descriptors_reject_unknown_multisampled_and_array_textures() {
        let mut desc = D3D11_TEXTURE2D_DESC {
            Width: 2,
            Height: 2,
            ArraySize: 1,
            Format: DXGI_FORMAT_R32G8X24_TYPELESS,
            ..Default::default()
        };
        desc.SampleDesc.Count = 1;
        assert!(validate_desc(&desc).is_ok());
        assert_eq!(format_byte_width(desc.Format), Some(8));
        for count in [0, 2, 4] {
            desc.SampleDesc.Count = count;
            assert!(validate_desc(&desc).is_err());
        }
        desc.SampleDesc.Count = 1;
        for size in [0, 2] {
            desc.ArraySize = size;
            assert!(validate_desc(&desc).is_err());
        }
        desc.ArraySize = 1;
        desc.Format = DXGI_FORMAT(0);
        assert!(validate_desc(&desc).is_err());
        assert!(stage_image(Stage::GBuffer0, &[0; 16], 8, &desc).is_err());
    }

    #[test]
    fn depth_staging_uses_only_copy_compatible_typeless_families() {
        for (source, staging, size) in [
            (
                DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
                DXGI_FORMAT_R32G8X24_TYPELESS,
                8,
            ),
            (DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_R24G8_TYPELESS, 4),
            (DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_R32_TYPELESS, 4),
            (DXGI_FORMAT_D16_UNORM, DXGI_FORMAT_R16_TYPELESS, 2),
        ] {
            assert_eq!(staging_format(source), staging);
            assert_eq!(staging_format(staging), staging);
            assert_eq!(format_byte_width(source), Some(size));
            assert_eq!(format_byte_width(staging), Some(size));
        }
    }

    #[test]
    fn raw_formats_without_rgb_means_are_not_errors() {
        for format in [
            DXGI_FORMAT_R32G8X24_TYPELESS,
            DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
            DXGI_FORMAT_R10G10B10A2_UNORM,
            DXGI_FORMAT_R16G16B16A16_FLOAT,
        ] {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: 2,
                Height: 1,
                Format: format,
                ..Default::default()
            };
            assert_eq!(stage_means(&[0; 16], 16, &desc).unwrap(), None);
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: 1,
            Height: 1,
            Format: DXGI_FORMAT_R8G8B8A8_TYPELESS,
            ..Default::default()
        };
        assert_eq!(stage_means(&[0; 4], 4, &desc).unwrap(), None);
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
