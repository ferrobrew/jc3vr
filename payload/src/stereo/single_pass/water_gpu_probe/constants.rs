use std::fmt::Write as _;

use anyhow::{Context, Result};
use jc3gi::graphics_engine::{graphics_engine::GraphicsEngine, texture::Texture};
use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ, D3D11_DEPTH_STENCIL_VIEW_DESC, D3D11_MAP_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_SAMPLER_DESC, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
        ID3D11Buffer, ID3D11DepthStencilView, ID3D11DeviceContext, ID3D11DeviceContext1,
        ID3D11Resource, ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D,
    },
    core::Interface,
};

use crate::stereo::single_pass::water_gpu_probe::{ProbeContext, pointer};

/// The longest prefix of a bound constant-buffer range a readback decodes, in floats.
const READBACK_FLOATS: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShaderStage {
    Vertex,
    Hull,
    Domain,
    Pixel,
}

impl ShaderStage {
    fn name(self) -> &'static str {
        match self {
            Self::Vertex => "vs",
            Self::Hull => "hs",
            Self::Domain => "ds",
            Self::Pixel => "ps",
        }
    }

    /// Reads the constant-buffer binding at `slot`, including the range the draw binds.
    ///
    /// The bound range is only observable through the D3D11.1 context. When the context is older,
    /// the binding falls back to the whole buffer with `range_known` cleared, so a readback is
    /// never presented as the range the draw actually uses.
    ///
    /// # Safety
    ///
    /// `ctx` must be the live immediate context.
    unsafe fn binding(self, ctx: &ID3D11DeviceContext, slot: Slot) -> Option<BufferBinding> {
        if let Ok(ctx1) = ctx.cast::<ID3D11DeviceContext1>() {
            let mut buffers = [None];
            let mut first_constant = 0u32;
            let mut num_constants = 0u32;
            // SAFETY: per this function's contract, the getters only read bound state.
            unsafe {
                match self {
                    Self::Vertex => ctx1.VSGetConstantBuffers1(
                        slot.index(),
                        1,
                        Some(buffers.as_mut_ptr()),
                        Some(&mut first_constant),
                        Some(&mut num_constants),
                    ),
                    Self::Hull => ctx1.HSGetConstantBuffers1(
                        slot.index(),
                        1,
                        Some(buffers.as_mut_ptr()),
                        Some(&mut first_constant),
                        Some(&mut num_constants),
                    ),
                    Self::Domain => ctx1.DSGetConstantBuffers1(
                        slot.index(),
                        1,
                        Some(buffers.as_mut_ptr()),
                        Some(&mut first_constant),
                        Some(&mut num_constants),
                    ),
                    Self::Pixel => ctx1.PSGetConstantBuffers1(
                        slot.index(),
                        1,
                        Some(buffers.as_mut_ptr()),
                        Some(&mut first_constant),
                        Some(&mut num_constants),
                    ),
                }
            }
            let buffer = buffers[0].take()?;
            return Some(BufferBinding {
                buffer,
                first_constant,
                num_constants,
                range_known: true,
            });
        }
        // SAFETY: `ctx` is the live immediate context.
        let buffer = unsafe { self.constant_buffer(ctx, slot) }?;
        let mut desc = D3D11_BUFFER_DESC::default();
        // SAFETY: `buffer` is live; `GetDesc` only writes the out parameter.
        unsafe { buffer.GetDesc(&mut desc) };
        let num_constants = desc.ByteWidth / 16;
        if num_constants == 0 {
            return None;
        }
        Some(BufferBinding {
            buffer,
            first_constant: 0,
            num_constants,
            range_known: false,
        })
    }

    /// # Safety
    ///
    /// `ctx` must be the live immediate context.
    unsafe fn constant_buffer(self, ctx: &ID3D11DeviceContext, slot: Slot) -> Option<ID3D11Buffer> {
        let mut buffers = [None];
        // SAFETY: per this function's contract, the getter only reads bound state.
        unsafe {
            match self {
                Self::Vertex => ctx.VSGetConstantBuffers(slot.index(), Some(&mut buffers)),
                Self::Hull => ctx.HSGetConstantBuffers(slot.index(), Some(&mut buffers)),
                Self::Domain => ctx.DSGetConstantBuffers(slot.index(), Some(&mut buffers)),
                Self::Pixel => ctx.PSGetConstantBuffers(slot.index(), Some(&mut buffers)),
            }
        }
        buffers[0].take()
    }
}

/// A constant-buffer binding and the range of it the draw uses.
struct BufferBinding {
    buffer: ID3D11Buffer,
    first_constant: u32,
    num_constants: u32,
    /// Whether the range came from the D3D11.1 getter. When false, the context does not expose
    /// ranges and the values describe the whole buffer only.
    range_known: bool,
}

/// A constant-buffer slot bound by the water draw's shaders.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Slot(u32);

impl Slot {
    const GLOBALS: Self = Self(0);
    const MVP: Self = Self(1);
    const BOX: Self = Self(2);
    const PATCH: Self = Self(3);
    const DISPLACEMENT: Self = Self(4);
    const STEREO: Self = Self(13);

    fn index(self) -> u32 {
        self.0
    }
}

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
    for (stage, slot) in CAPTURED_BINDINGS {
        // SAFETY: `ctx` is the live immediate context.
        let binding = unsafe { stage.binding(ctx, slot) };
        match read(ctx, binding.as_ref()) {
            Ok(readback) => log(context, stage, slot, &readback),
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
    log_ps_srvs(ctx, context);
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

struct Readback {
    pointer: usize,
    byte_width: u32,
    first_constant: u32,
    num_constants: u32,
    range_known: bool,
    values: Option<Vec<f32>>,
    range_error: Option<RangeError>,
}

fn read(ctx: &ID3D11DeviceContext, binding: Option<&BufferBinding>) -> Result<Readback> {
    let Some(binding) = binding else {
        return Ok(Readback {
            pointer: 0,
            byte_width: 0,
            first_constant: 0,
            num_constants: 0,
            range_known: false,
            values: None,
            range_error: None,
        });
    };
    let pointer = binding.buffer.as_raw() as usize;
    let mut desc = D3D11_BUFFER_DESC::default();
    // SAFETY: `binding.buffer` is live; `GetDesc` only writes the out parameter.
    unsafe { binding.buffer.GetDesc(&mut desc) };
    let base = Readback {
        pointer,
        byte_width: desc.ByteWidth,
        first_constant: binding.first_constant,
        num_constants: binding.num_constants,
        range_known: binding.range_known,
        values: None,
        range_error: None,
    };
    if desc.ByteWidth == 0 {
        return Ok(base);
    }
    // SAFETY: `ctx` is the live immediate context; `GetDevice` only reads its device.
    let device = unsafe { ctx.GetDevice() }.context("getting the device")?;
    let staging_desc = D3D11_BUFFER_DESC {
        ByteWidth: desc.ByteWidth,
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut staging = None;
    // SAFETY: the descriptor is valid and `staging` is an out parameter.
    unsafe { device.CreateBuffer(&staging_desc, None, Some(&mut staging)) }
        .context("creating the staging buffer")?;
    let staging = staging.context("staging buffer not created")?;
    let source: ID3D11Resource = binding
        .buffer
        .cast()
        .context("constant buffer is not a resource")?;
    let destination: ID3D11Resource = staging.cast().context("staging buffer is not a resource")?;
    // SAFETY: both resources are live and the copy covers the whole source.
    unsafe { ctx.CopyResource(&destination, &source) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: `staging` is a readback staging buffer.
    unsafe { ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
        .context("mapping the staging buffer")?;
    // The whole buffer is mapped, so the decode can start at the bound range's first constant
    // rather than at the buffer's beginning.
    // SAFETY: the mapping exposes `ByteWidth` bytes, and it stays live until `Unmap` below.
    let whole =
        unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), desc.ByteWidth as usize) };
    let decoded = decode_range(whole, binding.first_constant, binding.num_constants);
    // SAFETY: the mapping was created above, and `Unmap` is infallible.
    unsafe { ctx.Unmap(&staging, 0) };
    Ok(match decoded {
        Ok(values) if !values.is_empty() => Readback {
            values: Some(values),
            ..base
        },
        Ok(_) => base,
        Err(range_error) => Readback {
            range_error: Some(range_error),
            ..base
        },
    })
}

/// Why a bound constant-buffer range could not be decoded from a readback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeError {
    /// The readback is shorter than the bound range.
    Truncated { needed: usize, available: usize },
    /// The bound range starts past the end of the readback.
    OutOfRange { offset: usize, available: usize },
    /// The range's byte arithmetic overflowed.
    Overflow,
}

/// Decodes the floats of a bound constant-buffer range from a whole-buffer readback.
///
/// The range starts at `first_constant` (a 16-byte constant) and is capped at [`READBACK_FLOATS`].
/// Bytes before the range are not decoded.
fn decode_range(
    bytes: &[u8],
    first_constant: u32,
    num_constants: u32,
) -> Result<Vec<f32>, RangeError> {
    let offset = (first_constant as usize)
        .checked_mul(16)
        .ok_or(RangeError::Overflow)?;
    if offset > bytes.len() {
        return Err(RangeError::OutOfRange {
            offset,
            available: bytes.len(),
        });
    }
    // The count is in 16-byte constants, so the range spans four floats per constant; the decode
    // is capped at the prefix a readback reports.
    let floats = (num_constants as usize)
        .checked_mul(4)
        .ok_or(RangeError::Overflow)?
        .min(READBACK_FLOATS);
    let needed = offset
        .checked_add(floats.checked_mul(4).ok_or(RangeError::Overflow)?)
        .ok_or(RangeError::Overflow)?;
    let range = bytes.get(offset..needed).ok_or(RangeError::Truncated {
        needed,
        available: bytes.len(),
    })?;
    Ok(range
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect())
}

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

fn log_ps_srvs(ctx: &ID3D11DeviceContext, context: ProbeContext) {
    let mut text = String::new();
    for slot in PS_SRV_SLOTS {
        let mut view: Option<ID3D11ShaderResourceView> = None;
        unsafe { ctx.PSGetShaderResources(slot, Some(std::slice::from_mut(&mut view))) };
        let _ = write!(text, " t{slot}={:#x}", pointer(&view));
    }
    tracing::info!(
        target: "water_probe",
        "water probe: ps inputs pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} srv{text}",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
    );
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
            (0..READBACK_FLOATS as i32)
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
