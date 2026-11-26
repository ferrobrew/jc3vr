//! The lake permutation's refraction displacement, halved for a double-wide target.
//!
//! The WaveWorks lake pixel shader displaces its refraction lookup by a horizontal screen-UV offset
//! scaled by an immediate. On a target twice an eye's width that offset spans twice the eye-relative
//! distance, so the packed variant the per-eye water re-issue binds halves the horizontal scale and
//! leaves the vertical one alone.

use crate::{Dxbc, DxbcError, ShaderStage, TokenStream};

/// Halves the horizontal scale of the lake pixel shader's refraction displacement. Fails with
/// [`DxbcError::NoLakeRefraction`] unless the shader is a pixel shader holding exactly one
/// displacement `mul` in the shape the lake compiles to.
pub fn scale_lake_refraction(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    const MUL: [u32; 10] = [
        0x0a000038,
        0x00100032,
        2,
        0x00100046,
        2,
        0x00004002,
        (-5.0_f32).to_bits(),
        5.0_f32.to_bits(),
        0,
        0,
    ];
    let chunk = Dxbc::parse(blob)?
        .chunk(b"SHEX")
        .ok_or(DxbcError::NoShaderChunk)?;
    let stream = TokenStream::new(chunk.body(blob))?;
    let mut site = None;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoLakeRefraction);
    }
    for op in stream.instructions() {
        let op = op?;
        if stream.tokens()[op.start..op.end] == MUL && site.replace(op.start + 6).is_some() {
            return Err(DxbcError::NoLakeRefraction);
        }
    }
    let offset = chunk.body_offset + 4 * site.ok_or(DxbcError::NoLakeRefraction)?;
    let mut out = blob.to_vec();
    out[offset..offset + 4].copy_from_slice(&(-2.5_f32).to_le_bytes());
    crate::refresh_checksum(&mut out);
    Ok(out)
}
