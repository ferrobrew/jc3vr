use std::fmt::Write as _;

use anyhow::{Context, Result};
use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
        D3D11_USAGE_STAGING, ID3D11Buffer, ID3D11DeviceContext, ID3D11Resource,
        ID3D11ShaderResourceView,
    },
    core::Interface,
};

use crate::stereo::single_pass::water_gpu_probe::{ProbeContext, pointer};

/// The longest prefix of a bound constant buffer a readback decodes.
const READBACK_BYTES: u32 = 64;
const READBACK_FLOATS: usize = (READBACK_BYTES / 4) as usize;

#[derive(Clone, Copy)]
enum ShaderStage {
    Vertex,
    Domain,
}

impl ShaderStage {
    fn name(self) -> &'static str {
        match self {
            Self::Vertex => "vs",
            Self::Domain => "ds",
        }
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
                Self::Domain => ctx.DSGetConstantBuffers(slot.index(), Some(&mut buffers)),
            }
        }
        buffers[0].take()
    }
}

/// A constant-buffer slot bound by the water draw's shaders.
#[derive(Clone, Copy)]
struct Slot(u32);

impl Slot {
    const MVP: Self = Self(1);
    const DISPLACEMENT: Self = Self(4);

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
    for (stage, slot) in [
        (ShaderStage::Vertex, Slot::MVP),
        (ShaderStage::Domain, Slot::MVP),
        (ShaderStage::Domain, Slot::DISPLACEMENT),
    ] {
        // SAFETY: `ctx` is the live immediate context.
        let buffer = unsafe { stage.constant_buffer(ctx, slot) };
        match read(ctx, buffer.as_ref()) {
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
}

struct Readback {
    pointer: usize,
    byte_width: u32,
    values: Option<Vec<f32>>,
}

fn read(ctx: &ID3D11DeviceContext, buffer: Option<&ID3D11Buffer>) -> Result<Readback> {
    let Some(buffer) = buffer else {
        return Ok(Readback {
            pointer: 0,
            byte_width: 0,
            values: None,
        });
    };
    let pointer = buffer.as_raw() as usize;
    let mut desc = D3D11_BUFFER_DESC::default();
    // SAFETY: `buffer` is live; `GetDesc` only writes the out parameter.
    unsafe { buffer.GetDesc(&mut desc) };
    let floats = ((desc.ByteWidth / 4) as usize).min(READBACK_FLOATS);
    if floats == 0 {
        return Ok(Readback {
            pointer,
            byte_width: desc.ByteWidth,
            values: None,
        });
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
    let source: ID3D11Resource = buffer.cast().context("constant buffer is not a resource")?;
    let destination: ID3D11Resource = staging.cast().context("staging buffer is not a resource")?;
    // SAFETY: both resources are live and the copy covers the whole source.
    unsafe { ctx.CopyResource(&destination, &source) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: `staging` is a readback staging buffer.
    unsafe { ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
        .context("mapping the staging buffer")?;
    // The mapping is not guaranteed to be `f32`-aligned, so the readback goes through bytes.
    let mut bytes = [0u8; READBACK_BYTES as usize];
    let read = floats * 4;
    // SAFETY: the mapping exposes at least `read` bytes, and the copy and `Unmap` are both
    // infallible, so nothing can strand the mapping.
    unsafe {
        std::ptr::copy_nonoverlapping(mapped.pData.cast::<u8>(), bytes.as_mut_ptr(), read);
        ctx.Unmap(&staging, 0);
    }
    Ok(Readback {
        pointer,
        byte_width: desc.ByteWidth,
        values: decode_floats(&bytes[..read], floats),
    })
}

/// Decodes the first `count` little-endian `f32`s of a readback.
fn decode_floats(bytes: &[u8], count: usize) -> Option<Vec<f32>> {
    let bytes = bytes.get(..count.checked_mul(4)?)?;
    Some(
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect(),
    )
}

fn log(context: ProbeContext, stage: ShaderStage, slot: Slot, readback: &Readback) {
    let mut text = String::new();
    if let Some(values) = &readback.values {
        for row in values.as_chunks::<4>().0 {
            let _ = write!(
                text,
                "[{:.6} {:.6} {:.6} {:.6}]",
                row[0], row[1], row[2], row[3]
            );
        }
        let tail = values.as_chunks::<4>().1;
        if !tail.is_empty() {
            let _ = write!(text, "{tail:?}");
        }
    } else {
        text.push_str("none");
    }
    tracing::info!(
        target: "water_probe",
        "water probe: constant readback pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} stage={} slot={} pointer={:#x} byte_width={} values={text}",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
        stage.name(),
        slot.index(),
        readback.pointer,
        readback.byte_width,
    );
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
    fn decode_rejects_a_readback_shorter_than_the_requested_floats() {
        assert_eq!(decode_floats(&[], 1), None);
        assert_eq!(decode_floats(&[0u8; 63], READBACK_FLOATS), None);
        assert_eq!(decode_floats(&[0u8; 28], 8), None);
    }

    #[test]
    fn decode_ignores_bytes_past_the_requested_floats() {
        let mut bytes = bytes_of(&[1.0f32; READBACK_FLOATS]);
        bytes.extend_from_slice(&[0xAA; 8]);
        assert_eq!(decode_floats(&bytes, READBACK_FLOATS), Some(vec![1.0; 16]));
    }

    #[test]
    fn decode_reads_little_endian_floats() {
        let values = [
            1.0f32, -2.5, 0.0, 1e-8, 256.0, -0.5, 65536.0, 3.5, 0.25, -1.0, 2.0, 4.0, 8.0, 16.0,
            32.0, 0.125,
        ];
        assert_eq!(
            decode_floats(&bytes_of(&values), READBACK_FLOATS),
            Some(values.to_vec())
        );
    }

    #[test]
    fn decode_preserves_signed_zero() {
        let mut values = [0.0f32; READBACK_FLOATS];
        values[3] = -0.0;
        let decoded = decode_floats(&bytes_of(&values), READBACK_FLOATS).expect("decoded");
        assert_eq!(decoded[0].to_bits(), 0.0f32.to_bits());
        assert_eq!(decoded[3].to_bits(), (-0.0f32).to_bits());
        assert!(decoded[3].is_sign_negative());
    }

    #[test]
    fn decode_takes_the_first_floats_of_a_short_constant_buffer() {
        let values = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        assert_eq!(
            decode_floats(&bytes_of(&values), 8),
            Some(values.to_vec()),
            "a 32-byte constant buffer decodes its eight floats"
        );
    }
}
