use std::fmt::Write as _;

use jc3gi::graphics_engine::{graphics_engine::GraphicsEngine, texture::Texture};
use windows::{
    Win32::Graphics::{
        Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
        Direct3D11::{
            D3D11_DEPTH_STENCIL_VIEW_DESC, D3D11_SAMPLER_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC,
            D3D11_TEXTURE2D_DESC, ID3D11DepthStencilView, ID3D11DeviceContext, ID3D11SamplerState,
            ID3D11ShaderResourceView, ID3D11Texture2D,
        },
    },
    core::Interface,
};

use crate::{
    screenshot::harness::bound_buffer::{
        Readback, ShaderStage, Slot, WATER_READBACK_ROWS, read_bound_constant_buffer,
    },
    stereo::single_pass::water_gpu_probe::{ProbeContext, pointer},
};

/// A domain-shader resource slot the displaced world-position path reads.
#[derive(Clone, Copy)]
#[repr(u32)]
enum SrvSlot {
    T0 = 0,
    T3 = 3,
    T9 = 9,
    T10 = 10,
    T11 = 11,
    T12 = 12,
}

impl SrvSlot {
    const ALL: [Self; 6] = [
        Self::T0,
        Self::T3,
        Self::T9,
        Self::T10,
        Self::T11,
        Self::T12,
    ];

    fn index(self) -> u32 {
        self as u32
    }

    fn name(self) -> &'static str {
        match self {
            Self::T0 => "t0",
            Self::T3 => "t3",
            Self::T9 => "t9",
            Self::T10 => "t10",
            Self::T11 => "t11",
            Self::T12 => "t12",
        }
    }
}

pub(super) unsafe fn capture(ctx: &ID3D11DeviceContext, context: ProbeContext) {
    log_depth_target(ctx, context);
    let mut screen_row = None;
    for (stage, slot) in CAPTURED_BINDINGS {
        // SAFETY: `ctx` is the live immediate context.
        match read_bound_constant_buffer(ctx, stage, slot.index(), WATER_READBACK_ROWS) {
            Ok(readback) => {
                if stage == ShaderStage::Pixel && slot == Slot::GLOBALS {
                    screen_row = readback_row(&readback, PIXEL_GLOBALS_SCREEN_ROW);
                }
                log(context, stage, slot, &readback)
            }
            Err(error) => tracing::warn!(
                target: "water_probe",
                "water probe: constant readback pass={} eye={} invocation={} \
                 predicted_display_time_ns={} generation={} stage={} slot={}: {error:#}",
                context.pass,
                context.eye,
                context.invocation,
                context.frame.display_time(),
                context.frame.visual_generation,
                stage.name(),
                slot.index(),
            ),
        }
    }
    log_srvs(ctx, context);
    log_ps_srvs(ctx, context, screen_row);
    log_ps_samplers(ctx, context);
}

const CAPTURED_BINDINGS: [(ShaderStage, Slot); 12] = [
    (ShaderStage::Vertex, Slot::GLOBALS),
    (ShaderStage::Vertex, Slot::MVP),
    (ShaderStage::Vertex, Slot::BOX),
    (ShaderStage::Vertex, Slot::PATCH),
    (ShaderStage::Vertex, Slot::DISPLACEMENT),
    (ShaderStage::Vertex, Slot::STEREO),
    (ShaderStage::Hull, Slot::PATCH),
    (ShaderStage::Domain, Slot::MVP),
    (ShaderStage::Domain, Slot::DISPLACEMENT),
    (ShaderStage::Pixel, Slot::GLOBALS),
    (ShaderStage::Pixel, Slot::MVP),
    (ShaderStage::Pixel, Slot::BOX),
];

fn log(context: ProbeContext, stage: ShaderStage, slot: Slot, readback: &Readback) {
    let text = match &readback.values {
        Some(values) => format_values(values),
        None => "none".to_owned(),
    };
    let range_error = match readback.range_error {
        Some(error) => format!("{error:?}"),
        None => "none".to_owned(),
    };
    tracing::info!(
        target: "water_probe",
        "water probe: constant readback pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} stage={} slot={} pointer={:#x} byte_width={} range_source={} \
         first_constant={} num_constants={} values={text} range_error={range_error}",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
        stage.name(),
        slot.index(),
        readback.pointer,
        readback.byte_width,
        range_source(readback),
        readback.first_constant,
        readback.num_constants,
    );
}

/// Formats a readback's floats in scientific notation with ten significant digits, so every
/// value parses back to the same `f32` and a coefficient below `1e-6` stays visible.
fn format_values(values: &[f32]) -> String {
    let mut text = String::new();
    for row in values.as_chunks::<4>().0 {
        let _ = write!(
            text,
            "[{:.9e} {:.9e} {:.9e} {:.9e}]",
            row[0], row[1], row[2], row[3]
        );
    }
    let tail = values.as_chunks::<4>().1;
    if !tail.is_empty() {
        let _ = write!(text, "{tail:?}");
    }
    text
}

/// Names where a readback's range came from, so a whole-buffer fallback is never read as the
/// range the draw actually binds.
fn range_source(readback: &Readback) -> &'static str {
    if readback.pointer == 0 {
        "none"
    } else if readback.range_known {
        "ctx1"
    } else {
        "base-unsupported"
    }
}

fn log_srvs(ctx: &ID3D11DeviceContext, context: ProbeContext) {
    let mut text = String::new();
    for slot in SrvSlot::ALL {
        let mut view: Option<ID3D11ShaderResourceView> = None;
        // SAFETY: the getter only reads this live immediate context's bound state.
        unsafe { ctx.DSGetShaderResources(slot.index(), Some(std::slice::from_mut(&mut view))) };
        let _ = write!(text, " {}={:#x}", slot.name(), pointer(&view));
    }
    tracing::info!(
        target: "water_probe",
        "water probe: ds inputs pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} srv{text}",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
    );
}

const PS_SRV_SLOTS: [u32; 17] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 44, 45];

/// The PS slots the visible water shader samples through screen-space UVs.
const SCREEN_SPACE_PS_SRV_SLOTS: [u32; 7] = [0, 1, 2, 3, 4, 11, 12];

/// The `GlobalConstants` row holding the reciprocals of the display width and height, which the water
/// shaders scale `SV_Position` by to build those lookups.
const PIXEL_GLOBALS_SCREEN_ROW: u32 = 8;

fn log_ps_srvs(ctx: &ID3D11DeviceContext, context: ProbeContext, screen_row: Option<[f32; 4]>) {
    let mut text = String::new();
    for slot in PS_SRV_SLOTS {
        let mut view: Option<ID3D11ShaderResourceView> = None;
        // SAFETY: the getter only reads this live immediate context's bound state.
        unsafe { ctx.PSGetShaderResources(slot, Some(std::slice::from_mut(&mut view))) };
        let _ = write!(text, " t{slot}={:#x}", pointer(&view));
        if SCREEN_SPACE_PS_SRV_SLOTS.contains(&slot) {
            let _ = write!(text, "({})", srv_dimensions(view.as_ref()));
        }
    }
    let screen_row = match screen_row {
        Some(row) => format!(
            "[{:.9e} {:.9e} {:.9e} {:.9e}]",
            row[0], row[1], row[2], row[3]
        ),
        None => "none".to_owned(),
    };
    tracing::info!(
        target: "water_probe",
        "water probe: ps inputs pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} cb0row8={screen_row} srv{text}",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
    );
}

/// The bound view and the texture under it: the resource identity, the base texture's dimensions, the
/// view's own format and dimension, and the mip window a `Texture2D` view selects, so a screen-space
/// lookup can be checked against the size and mip the shader's UV scale assumes.
fn srv_dimensions(view: Option<&ID3D11ShaderResourceView>) -> String {
    let Some(view) = view else {
        return "none".to_owned();
    };
    let mut view_desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
    // SAFETY: `view` is live; `GetDesc` only writes the out parameter.
    unsafe { view.GetDesc(&mut view_desc) };
    // SAFETY: the resource getter only reads this live view.
    let Ok(resource) = (unsafe { view.GetResource() }) else {
        return "unknown".to_owned();
    };
    let resource_pointer = resource.as_raw() as usize;
    let mips = if view_desc.ViewDimension == D3D_SRV_DIMENSION_TEXTURE2D {
        // SAFETY: the union holds the `Texture2D` variant for this dimension.
        let texture2d = unsafe { view_desc.Anonymous.Texture2D };
        format!(" mip={}+{}", texture2d.MostDetailedMip, texture2d.MipLevels)
    } else {
        String::new()
    };
    let Ok(texture) = resource.cast::<ID3D11Texture2D>() else {
        return format!(
            "res={resource_pointer:#x} view={:?}/{:?}{mips} not-2d",
            view_desc.Format, view_desc.ViewDimension
        );
    };
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: `texture` is live; `GetDesc` only writes the out parameter.
    unsafe { texture.GetDesc(&mut desc) };
    format!(
        "res={resource_pointer:#x} tex={}x{}/{:?} view={:?}/{:?}{mips}",
        desc.Width, desc.Height, desc.Format, view_desc.Format, view_desc.ViewDimension
    )
}

/// One float4 row of a readback by the constant index the shader addresses. The decoded window
/// starts at the range the draw bound, and D3D11.1 rebases that window to register 0, so the shader's
/// index is the window offset rather than an index into the whole buffer.
fn readback_row(readback: &Readback, row: u32) -> Option<[f32; 4]> {
    if row >= readback.num_constants {
        return None;
    }
    let values = readback.values.as_ref()?;
    let offset = row as usize * 4;
    Some([
        *values.get(offset)?,
        *values.get(offset + 1)?,
        *values.get(offset + 2)?,
        *values.get(offset + 3)?,
    ])
}

const PS_SAMPLER_SLOTS: [u32; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

fn log_ps_samplers(ctx: &ID3D11DeviceContext, context: ProbeContext) {
    let mut text = String::new();
    for slot in PS_SAMPLER_SLOTS {
        let mut sampler: Option<ID3D11SamplerState> = None;
        unsafe { ctx.PSGetSamplers(slot, Some(std::slice::from_mut(&mut sampler))) };
        let _ = write!(text, " s{slot}={:#x}", pointer(&sampler));
        let Some(sampler) = sampler else {
            continue;
        };
        let mut desc = D3D11_SAMPLER_DESC::default();
        unsafe { sampler.GetDesc(&mut desc) };
        let _ = write!(
            text,
            " filter={:?} address={:?}/{:?}/{:?}",
            desc.Filter, desc.AddressU, desc.AddressV, desc.AddressW
        );
    }
    tracing::info!(
        target: "water_probe",
        "water probe: ps samplers pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} samplers{text}",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
    );
}

/// Logs the identity of the depth-stencil target the draw is bound against, next to the graphics
/// engine's own depth textures, so a capture can tell whether the depth bracket generated against
/// the same resource the draw reads.
fn log_depth_target(ctx: &ID3D11DeviceContext, context: ProbeContext) {
    let mut dsv: Option<ID3D11DepthStencilView> = None;
    // SAFETY: the getter only reads this live immediate context's bound state.
    unsafe { ctx.OMGetRenderTargets(None, Some(&mut dsv)) };
    let Some(dsv) = dsv else {
        tracing::info!(
            target: "water_probe",
            "water probe: depth target pass={} eye={} invocation={} predicted_display_time_ns={} \
             generation={} dsv=none",
            context.pass,
            context.eye,
            context.invocation,
            context.frame.display_time(),
            context.frame.visual_generation,
        );
        return;
    };
    let dsv_pointer = dsv.as_raw() as usize;
    let mut view = D3D11_DEPTH_STENCIL_VIEW_DESC::default();
    // SAFETY: `dsv` is live; `GetDesc` only writes the out parameter.
    unsafe { dsv.GetDesc(&mut view) };
    let resource = unsafe { dsv.GetResource() }.ok();
    let resource_pointer = resource.as_ref().map_or(0, |r| r.as_raw() as usize);
    let texture = resource
        .as_ref()
        .and_then(|r| r.cast::<ID3D11Texture2D>().ok());
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    if let Some(texture) = &texture {
        // SAFETY: `texture` is live; `GetDesc` only writes the out parameter.
        unsafe { texture.GetDesc(&mut desc) };
    }
    let (main_depth, render_depth) = typed_depth_targets();
    tracing::info!(
        target: "water_probe",
        "water probe: depth target pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} dsv={:#x} dsv_format={:?} dsv_dimension={:?} dsv_flags={:#x} \
         resource={:#x} texture2d={} width={} height={} format={:?} mips={} array_size={} \
         sample_count={} sample_quality={} main_depth_texture={:#x} main_depth_resource={:#x} \
         main_depth_dsv={:#x} main_depth_format={:?} render_depth_texture={:#x} \
         render_depth_resource={:#x} matches_main_depth={} matches_render_depth={}",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
        dsv_pointer,
        view.Format,
        view.ViewDimension,
        view.Flags,
        resource_pointer,
        texture.is_some(),
        desc.Width,
        desc.Height,
        desc.Format,
        desc.MipLevels,
        desc.ArraySize,
        desc.SampleDesc.Count,
        desc.SampleDesc.Quality,
        main_depth.texture,
        main_depth.resource,
        main_depth.dsv,
        main_depth.format,
        render_depth.texture,
        render_depth.resource,
        resource_pointer != 0 && resource_pointer == main_depth.resource,
        resource_pointer != 0 && resource_pointer == render_depth.resource,
    );
}

/// The graphics engine's depth textures, reduced to the raw identity of the COM objects they hold.
#[derive(Clone, Copy, Default)]
struct TypedDepthTarget {
    texture: usize,
    resource: usize,
    dsv: usize,
    format: u32,
}

fn typed_depth_targets() -> (TypedDepthTarget, TypedDepthTarget) {
    let Some(ge) = (unsafe { GraphicsEngine::get() }) else {
        return (TypedDepthTarget::default(), TypedDepthTarget::default());
    };
    // SAFETY: the engine's texture pointers are only read here, and the raw interface pointers are
    // borrowed for the duration of this call.
    unsafe {
        (
            typed_depth_target(ge.m_MainDepthTexture),
            typed_depth_target(ge.m_RenderContext.m_DepthBufferTexture),
        )
    }
}

/// # Safety
///
/// `texture` must be null or a live engine texture.
unsafe fn typed_depth_target(texture: *mut Texture) -> TypedDepthTarget {
    let Some(texture) = (unsafe { texture.as_ref() }) else {
        return TypedDepthTarget::default();
    };
    TypedDepthTarget {
        texture: texture as *const Texture as usize,
        resource: texture.m_Texture.as_raw() as usize,
        dsv: texture.m_DSV.as_raw() as usize,
        format: texture.m_Format,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screenshot::harness::bound_buffer::{RangeError, decode_range};

    fn bytes_of(values: &[f32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    #[test]
    fn capture_reads_the_pixel_globals_and_mvp() {
        assert!(
            CAPTURED_BINDINGS.contains(&(ShaderStage::Pixel, Slot::GLOBALS)),
            "the pixel globals carry the sun, camera, and screen size rows"
        );
        assert!(
            CAPTURED_BINDINGS.contains(&(ShaderStage::Pixel, Slot::MVP)),
            "the pixel type constants carry the specular colour and power rows"
        );
    }

    #[test]
    fn screen_space_slots_are_all_in_the_identity_log() {
        for slot in SCREEN_SPACE_PS_SRV_SLOTS {
            assert!(
                PS_SRV_SLOTS.contains(&slot),
                "t{slot} needs an identity line beside its dimensions"
            );
        }
    }

    #[test]
    fn screen_row_is_the_shader_index_inside_the_bound_window() {
        let readback = Readback {
            pointer: 1,
            byte_width: 1024,
            first_constant: 4,
            num_constants: 16,
            range_known: true,
            values: Some((0..64).map(|value| value as f32).collect()),
            range_error: None,
        };
        assert_eq!(
            readback_row(&readback, PIXEL_GLOBALS_SCREEN_ROW),
            Some([32.0, 33.0, 34.0, 35.0]),
            "D3D11.1 rebases the bound window to register 0, so the shader's index addresses the \
             decoded window"
        );
        assert_eq!(
            readback_row(&readback, 0),
            Some([0.0, 1.0, 2.0, 3.0]),
            "constant 0 of the window is the first decoded float4"
        );
        assert_eq!(
            readback_row(&readback, 16),
            None,
            "a constant past the bound range is absent even though the decoded floats reach it"
        );
        assert_eq!(
            readback_row(&readback, 40),
            None,
            "a constant past the decoded prefix is absent"
        );
    }

    #[test]
    fn decode_range_starts_at_the_bound_first_constant() {
        let values = [
            0.0f32, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0,
        ];
        assert_eq!(
            decode_range(&bytes_of(&values), 1, 1),
            Ok(vec![1.0, 2.0, 3.0, 4.0]),
            "the range begins at the first bound constant, not the buffer beginning"
        );
    }

    #[test]
    fn decode_range_caps_the_prefix_at_the_readback_float_count() {
        let values: Vec<f32> = (0..96).map(|value| value as f32).collect();
        let decoded = decode_range(&bytes_of(&values), 0, 96).expect("decoded");
        assert_eq!(
            decoded,
            (0..(WATER_READBACK_ROWS * 4) as i32)
                .map(|v| v as f32)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn decode_range_counts_constants_as_four_floats() {
        let values: Vec<f32> = (0..20).map(|value| value as f32).collect();
        let decoded = decode_range(&bytes_of(&values), 0, 5).expect("decoded");
        assert_eq!(
            decoded,
            (0..20).map(|v| v as f32).collect::<Vec<_>>(),
            "an 80-byte buffer's five constants decode as twenty floats"
        );
    }

    #[test]
    fn format_values_keeps_a_small_coefficient_and_roundtrips() {
        let values = [
            1.0e-7f32,
            -2.5e-8,
            1.0,
            -0.5,
            f32::MAX,
            f32::MIN_POSITIVE,
            f32::EPSILON,
            0.0,
        ];
        let text = format_values(&values);
        let first = text
            .split(['[', ' '])
            .nth(1)
            .expect("the first formatted value");
        assert_ne!(
            first, "0.000000000e0",
            "a coefficient below 1e-6 must not format as zero: {text}"
        );
        let parsed = text
            .split(['[', ']', ' '])
            .filter(|token| !token.is_empty())
            .map(|token| token.parse::<f32>().expect("a formatted value parses"))
            .collect::<Vec<_>>();
        assert_eq!(parsed.len(), values.len(), "{text}");
        for (parsed, original) in parsed.iter().zip(values) {
            assert_eq!(parsed.to_bits(), original.to_bits(), "{text}");
        }
    }

    #[test]
    fn decode_range_decodes_short_ranges_in_full() {
        let values: Vec<f32> = (0..20).map(|value| value as f32).collect();
        assert_eq!(
            decode_range(&bytes_of(&values), 2, 2),
            Ok(vec![8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0]),
            "two constants from the third one decode as eight floats"
        );
    }

    #[test]
    fn decode_range_reports_a_truncated_readback() {
        let values = [1.0f32, 2.0, 3.0, 4.0];
        assert_eq!(
            decode_range(&bytes_of(&values)[..8], 0, 4),
            Err(RangeError::Truncated {
                needed: 64,
                available: 8,
            })
        );
    }

    #[test]
    fn decode_range_reports_a_range_past_the_buffer() {
        assert_eq!(
            decode_range(&bytes_of(&[1.0f32; 4]), 8, 1),
            Err(RangeError::OutOfRange {
                offset: 128,
                available: 16,
            })
        );
    }

    #[test]
    fn decode_range_accepts_an_empty_range_at_the_buffer_end() {
        assert_eq!(decode_range(&bytes_of(&[1.0f32; 4]), 1, 0), Ok(Vec::new()));
    }
}
