use anyhow::{Context, Result};
use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
        D3D11_USAGE_STAGING, ID3D11Buffer, ID3D11DeviceContext, ID3D11DeviceContext1,
        ID3D11Resource,
    },
    core::Interface,
};

/// The longest prefix of a bound constant-buffer range a readback decodes, in floats.
pub(crate) const WATER_READBACK_ROWS: u32 = 16;
const MAX_READBACK_ROWS: u32 = 18;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShaderStage {
    Vertex,
    Hull,
    Domain,
    Pixel,
}

impl ShaderStage {
    pub(crate) fn name(self) -> &'static str {
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
pub(crate) struct Slot(pub(crate) u32);

impl Slot {
    pub(crate) const GLOBALS: Self = Self(0);
    pub(crate) const MVP: Self = Self(1);
    pub(crate) const BOX: Self = Self(2);
    pub(crate) const PATCH: Self = Self(3);
    pub(crate) const DISPLACEMENT: Self = Self(4);
    pub(crate) const STEREO: Self = Self(13);

    pub(crate) fn index(self) -> u32 {
        self.0
    }
}

pub(crate) fn read_bound_constant_buffer(
    ctx: &ID3D11DeviceContext,
    stage: ShaderStage,
    slot: u32,
    max_rows: u32,
) -> Result<Readback> {
    if max_rows == 0
        || (max_rows > MAX_READBACK_ROWS
            && !(stage == ShaderStage::Vertex && slot == 3 && max_rows == 640))
    {
        anyhow::bail!(
            "constant buffer readback: requested row limit is outside 1..={MAX_READBACK_ROWS}"
        );
    }
    let binding = unsafe { stage.binding(ctx, Slot(slot)) };
    read(ctx, binding.as_ref(), max_rows)
}

pub(crate) struct Readback {
    pub(crate) pointer: usize,
    pub(crate) byte_width: u32,
    pub(crate) first_constant: u32,
    pub(crate) num_constants: u32,
    pub(crate) range_known: bool,
    pub(crate) values: Option<Vec<f32>>,
    pub(crate) range_error: Option<RangeError>,
}

fn read(
    ctx: &ID3D11DeviceContext,
    binding: Option<&BufferBinding>,
    max_rows: u32,
) -> Result<Readback> {
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
    if mapped.pData.is_null() {
        unsafe { ctx.Unmap(&staging, 0) };
        anyhow::bail!("constant buffer mapping has no data");
    }
    let whole =
        unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), desc.ByteWidth as usize) };
    let decoded = decode_range_limited(
        whole,
        binding.first_constant,
        binding.num_constants,
        max_rows,
    );
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
pub(crate) enum RangeError {
    /// The readback is shorter than the bound range.
    Truncated { needed: usize, available: usize },
    /// The bound range starts past the end of the readback.
    OutOfRange { offset: usize, available: usize },
    /// The range's byte arithmetic overflowed.
    Overflow,
}

/// Decodes the floats of a bound constant-buffer range from a whole-buffer readback.
///
/// The range starts at `first_constant` (a 16-byte constant) and is capped at the requested rows.
/// Bytes before the range are not decoded.
#[cfg(test)]
pub(crate) fn decode_range(
    bytes: &[u8],
    first_constant: u32,
    num_constants: u32,
) -> Result<Vec<f32>, RangeError> {
    decode_range_limited(bytes, first_constant, num_constants, WATER_READBACK_ROWS)
}

fn decode_range_limited(
    bytes: &[u8],
    first_constant: u32,
    num_constants: u32,
    max_rows: u32,
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
        .min(max_rows as usize * 4);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eighteen_row_window_keeps_both_eye_matrices() {
        let bytes: Vec<u8> = (0..80_u32)
            .flat_map(|value| (value as f32).to_le_bytes())
            .collect();
        let values = decode_range_limited(&bytes, 1, 18, 18).unwrap();
        assert_eq!(values.len(), 72);
        assert_eq!(values[71], 75.0);
        assert_eq!(
            decode_range_limited(&bytes, 1, 18, WATER_READBACK_ROWS)
                .unwrap()
                .len(),
            64
        );
        assert!(decode_range_limited(&bytes[..64], 1, 18, 18).is_err());
    }
}
