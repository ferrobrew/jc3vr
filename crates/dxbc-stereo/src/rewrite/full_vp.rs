//! Shaders that project through the full view-projection, read from the eye's slot of `cb13`.
//!
//! The full, translation-bearing view-projection (`cb0[0..3]`, the render context's
//! `m_ViewProjectionF`) is outside the `cb0` remap's per-eye rows, so a shader that projects an
//! absolute world position through it was reprojected: its centre clip position post-multiplied by
//! `M_eye`. That lands in the eye only in real arithmetic. `cb13` carries each eye's copy of those
//! four rows, as an eye rendered on its own stages them, and reading them in place of `cb0[0..3]`
//! makes the shader do the arithmetic that render does. Outside the routed range both copies mirror
//! the current `cb0` rows, so the substitution changes nothing there.

use crate::{
    container::{Dxbc, DxbcError},
    isa::OPCODE_DCL_CONSTANT_BUFFER,
    rewrite::common::{
        CB_ACCESS_DYNAMIC_INDEXED, FULL_VIEW_PROJECTION_ROW_BASE, OPERAND_CB_2D_IMM,
        STEREO_CB_REGISTER, STEREO_FULL_CB_ROWS, declared_register, reassemble,
    },
    tokens::{OperandKind, ShaderStage, TokenStream},
};

/// Rewrites a vertex shader drawn once per eye to read the full view-projection (`cb0[0..3]`) from
/// `cb13`'s first eye slot, which the payload's per-eye re-issue pins to the eye it draws, instead of
/// reprojecting its `SV_Position` by `M_eye`. Every `cb0[0..3]` operand becomes the matching `cb13`
/// row in place; nothing else changes, and no interface is added.
pub fn remap_pinned_full_view_projection(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    if dxbc.chunk(b"SHEX").is_none() && dxbc.chunk(b"SHDR").is_some() {
        return Err(DxbcError::UnsupportedShaderModel);
    }
    let shex = dxbc.chunk(b"SHEX").ok_or(DxbcError::NoShaderChunk)?;
    let stream = TokenStream::new(shex.body(blob))?;
    if stream.stage() != ShaderStage::Vertex {
        return Err(DxbcError::NotVertexShader);
    }
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len() + 4);
    out.extend_from_slice(&tokens[..2]);
    let mut injected = false;
    let mut remapped = 0;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            if insn.opcode == OPCODE_DCL_CONSTANT_BUFFER
                && declared_register(tokens, &insn)? == STEREO_CB_REGISTER
            {
                return Err(DxbcError::Cb13AlreadyDeclared);
            }
            out.extend_from_slice(&tokens[insn.start..insn.end]);
            continue;
        }
        if !injected {
            out.extend_from_slice(&[
                (4 << 24) | CB_ACCESS_DYNAMIC_INDEXED | OPCODE_DCL_CONSTANT_BUFFER,
                OPERAND_CB_2D_IMM,
                STEREO_CB_REGISTER,
                STEREO_FULL_CB_ROWS,
            ]);
            injected = true;
        }
        let base = out.len();
        out.extend_from_slice(&tokens[insn.start..insn.end]);
        for operand in insn.operands() {
            let operand = operand?;
            let OperandKind::ConstantBuffer {
                register: 0,
                element: element @ 0..=3,
            } = operand.kind
            else {
                continue;
            };
            let tok = tokens[operand.token_offset];
            if (tok >> 22) & 0x7 != 0 || (tok >> 25) & 0x7 != 0 {
                return Err(DxbcError::UnsupportedOperandEncoding);
            }
            let extended = ((tok >> 31) & 1) as usize;
            let at = base + (operand.token_offset - insn.start) + 1 + extended;
            out[at] = STEREO_CB_REGISTER;
            out[at + 1] = FULL_VIEW_PROJECTION_ROW_BASE + element;
            remapped += 1;
        }
    }
    if remapped == 0 {
        return Err(DxbcError::NoFullViewProjection);
    }
    let length = out.len() as u32;
    out[1] = length;
    let new_shex = out.iter().flat_map(|t| t.to_le_bytes()).collect();
    let isgn = dxbc
        .chunk(b"ISGN")
        .ok_or(DxbcError::MissingInputSignature)?;
    let osgn = dxbc
        .chunk(b"OSGN")
        .ok_or(DxbcError::MissingOutputSignature)?;
    reassemble(
        &dxbc,
        blob,
        isgn.body(blob).to_vec(),
        osgn.body(blob).to_vec(),
        new_shex,
        false,
    )
}
