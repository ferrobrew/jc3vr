//! The terrain domain shaders' camera rows, read from each eye's `cb13` slot.
//!
//! The terrain domain shaders build clip as `(world − camera) · m_OffsetViewProjection` from their
//! own constant buffer (`cb1`), whose `m_OffsetViewProjection` and `m_CameraPosition` rows the block
//! type copies from the same render-context values the vertex globals carry. Reprojecting the
//! centre's clip by `M_eye` lands in the eye only in real arithmetic; in floats each patch's surface
//! moves by a few to a few hundred floats of depth. Reading the eye's own copy of those rows instead
//! makes the shader do the arithmetic an eye rendered on its own does.
//!
//! The rows are chosen once in a prologue: `movc` picks the eye's `cb13` rows while the selector row
//! ([`EYE_ROWS_SELECT_ROW`]) is set, and the shader's own `cb1` rows otherwise, so outside the routed
//! range (the reflection passes that reuse these shaders, or any frame `cb13` only mirrors the
//! current view) the shader reads exactly what it did before. Every later read of those `cb1` rows
//! reads the chosen temps.

use crate::{
    container::DxbcError,
    rewrite::common::{
        EYE_ROWS_SELECT_ROW, OPERAND_CB_2D_IMM, OPERAND_TEMP_SELECT_X, STEREO_CB_REGISTER,
    },
    tokens::{IDX_IMM32_PLUS_RELATIVE, Instruction, OperandKind, TokenStream, parse_operand},
};

/// How many temps the chosen rows occupy: the four offset view-projection rows and the camera
/// position.
pub(super) const EYE_ROW_TEMPS: u32 = 5;

/// Where the eye's slot of `cb13` starts.
#[derive(Clone, Copy)]
pub(super) enum RowBase {
    /// The first slot, which a per-eye re-issue pins to the eye it draws.
    Pinned,
    /// The slot whose first row index a temp's `.x` holds (`5 * eye`).
    Temp(u32),
}

/// Whether the shader reads any of the terrain constants' offset view-projection rows, the shape
/// this rewrite exists for.
pub(super) fn reads_terrain_view_projection(stream: &TokenStream) -> Result<bool, DxbcError> {
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            continue;
        }
        for operand in insn.operands() {
            if let OperandKind::ConstantBuffer {
                register: TERRAIN_CB_REGISTER,
                element: 1..=4,
            } = operand?.kind
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Emits `movc rRows+k.xyzw, cb13[select].xxxx, cb13[base + k].xyzw, cb1[source(k)].xyzw` for the
/// five rows: the eye's copy while the selector is set, the shader's own row otherwise.
pub(super) fn emit_select_rows(out: &mut Vec<u32>, rows_temp: u32, base: RowBase) {
    for k in 0..EYE_ROW_TEMPS {
        let eye_row: &[u32] = match base {
            RowBase::Pinned => &[OPERAND_CB_2D_IMM, STEREO_CB_REGISTER, k],
            RowBase::Temp(r) => &[
                OPERAND_CB_2D_IMM | (IDX_IMM32_PLUS_RELATIVE << 25),
                STEREO_CB_REGISTER,
                k,
                OPERAND_TEMP_SELECT_X,
                r,
            ],
        };
        let len = 1 + 2 + 3 + eye_row.len() as u32 + 3;
        out.extend_from_slice(&[
            (len << 24) | OPCODE_MOVC,
            OPERAND_TEMP_MASK_XYZW,
            rows_temp + k,
            OPERAND_CB_2D_IMM_SWIZZLE_XXXX,
            STEREO_CB_REGISTER,
            EYE_ROWS_SELECT_ROW,
        ]);
        out.extend_from_slice(eye_row);
        out.extend_from_slice(&[OPERAND_CB_2D_IMM, TERRAIN_CB_REGISTER, source_row(k)]);
    }
}

/// Re-serializes one instruction, turning every read of a terrain camera row (`cb1[1..5]`) into a
/// read of the temp the prologue chose it into, keeping the operand's swizzle and modifiers. The
/// operand loses its second index token, so the instruction's length is recomputed.
pub(super) fn rewrite_rows_instruction(
    tokens: &[u32],
    insn: &Instruction<'_>,
    rows_temp: u32,
    out: &mut Vec<u32>,
) -> Result<(), DxbcError> {
    let opcode_at = out.len();
    out.extend_from_slice(&tokens[insn.start..insn.operands_start()]);
    let mut pos = insn.operands_start();
    while pos < insn.end {
        let (operand, next) = parse_operand(tokens, pos)?;
        let k = match operand.kind {
            OperandKind::ConstantBuffer {
                register: TERRAIN_CB_REGISTER,
                element,
            } => eye_row_of(element),
            _ => None,
        };
        let Some(k) = k else {
            out.extend_from_slice(&tokens[pos..next]);
            pos = next;
            continue;
        };
        let tok = tokens[pos];
        if (tok >> 22) & 0x7 != 0 || (tok >> 25) & 0x7 != 0 {
            return Err(DxbcError::UnsupportedOperandEncoding);
        }
        let mut extended_end = pos + 1;
        if (tok >> 31) & 1 == 1 {
            // Extended operand tokens chain through their own bit 31.
            while tokens
                .get(extended_end)
                .ok_or(DxbcError::UnexpectedEndOfTokens)?
                >> 31
                & 1
                == 1
            {
                extended_end += 1;
            }
            extended_end += 1;
        }
        // CONSTANT_BUFFER, two immediate indices -> TEMP, one immediate index; the component
        // selection, swizzle, and extended (modifier) bit stay.
        out.push((tok & !(0xFF << 12) & !(0x3 << 20)) | (1 << 20));
        out.extend_from_slice(&tokens[pos + 1..extended_end]);
        out.push(rows_temp + k);
        pos = next;
    }
    let new_len = (out.len() - opcode_at) as u32;
    if new_len > 0x7F {
        return Err(DxbcError::InstructionTooLong);
    }
    out[opcode_at] = (out[opcode_at] & !(0x7F << 24)) | (new_len << 24);
    Ok(())
}

/// The terrain constants' register (`cb1`) in the terrain domain shaders.
const TERRAIN_CB_REGISTER: u32 = 1;

/// `movc`.
const OPCODE_MOVC: u32 = 0x37;
/// `rN.xyzw` as a destination: temp register, 1D immediate index, write mask `.xyzw`.
const OPERAND_TEMP_MASK_XYZW: u32 = 0x0010_00F2;
/// `cbN[a][b].xxxx`: [`OPERAND_CB_2D_IMM`] with the swizzle replicating `.x`.
const OPERAND_CB_2D_IMM_SWIZZLE_XXXX: u32 = 0x0020_8006;

/// The `cb13` slot row (`k`) a terrain constants row maps to: the four offset view-projection rows
/// are the slot's first four, the camera position its fifth. `None` for any other row.
fn eye_row_of(element: u32) -> Option<u32> {
    match element {
        1..=4 => Some(element - 1),
        5 => Some(4),
        _ => None,
    }
}

/// The inverse of [`eye_row_of`]: the terrain constants row slot row `k` mirrors.
fn source_row(k: u32) -> u32 {
    k + 1
}
