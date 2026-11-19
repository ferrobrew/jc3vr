//! Lay each half of a double-wide target's screen-door pattern out from that half's own origin.
//!
//! The engine's alpha-tested and fading materials discard a pixel by comparing a coverage value
//! against a per-pixel threshold built from the pixel's position:
//!
//! ```text
//! dp2 r0.x, v0.xyxx, l(0.467944, -0.703648, 0, 0)  // the threshold, from SV_Position
//! frc r0.x, r0.x
//! add r0.x, -r0.x, v6.w                            // the coverage (a distance fade, a LOD crossfade)
//! lt r0.x, r0.x, l(0)
//! discard_nz r0.x
//! ```
//!
//! The vector is not a whole number of cycles per pixel, so the pattern does not repeat at any
//! horizontal offset: in the right half of a double-wide target, where every pixel's `x` is the left
//! eye's width larger than in an eye-sized target, every threshold differs from the one an eye-sized
//! target computes, and a different set of pixels survives the fade. The same hash dithers the
//! volumetric fog's march, and rotates the ambient-occlusion spiral, whose own rewrite makes the
//! position eye-local first.
//!
//! [`localize_screen_door`] rewrites every such `dp2` to read an eye-local position instead: `x`
//! minus the eye boundary when the pixel lies at or past it, else `x` unchanged, with `y` as it was.
//! The boundary is the one the eye-camera rewrite reads (`.x` of [`EYE_CAMERA_BOUNDARY_ROW`] in
//! [`EYE_CAMERA_CB_REGISTER`]): the first pixel of the right eye's half while an eye-split pass is
//! drawing, and a value no pixel reaches everywhere else, where the rewritten shader computes the
//! engine's own pattern. An unbound buffer reads as a boundary of zero, which every pixel reaches,
//! and subtracting zero leaves the position unchanged as well.
//!
//! Any `dp2` of the pixel position with a fixed vector is such a hash and is rewritten. The other one
//! the game's shaders build is the sun-shadow filter's rotation, `l(12.9898, 78.233, 0, 0)`, whose
//! vector the payload zeroes before this rewrite runs (`patch_shadow_pcf_hash`); a zero vector makes
//! the product a constant and is left alone.
//!
//! The rewrite composes after the eye-camera rewrite, which declines a shader already declaring the
//! buffer's slot: a slot declared at least [`EYE_CAMERA_CB_ROWS`] long is that rewrite's and is read
//! as it is; a shorter one is not, and the shader is declined. Other reads of the pixel position (a
//! texel fetch, a screen UV) address the double-wide target itself and are left alone.

use crate::{
    container::{Dxbc, DxbcError},
    rewrite::{
        camera::{EYE_CAMERA_BOUNDARY_ROW, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_CB_ROWS},
        common::{
            OPCODE_AND, OPCODE_DCL_CONSTANT_BUFFER, OPCODE_DCL_TEMPS, OPCODE_MOV,
            OPERAND_CB_2D_IMM, OPERAND_INPUT_SELECT_X, OPERAND_TEMP_MASK_X, OPERAND_TEMP_SELECT_X,
            declared_register, reassemble, signature_register,
        },
    },
    tokens::{Instruction, ShaderStage, TokenStream, parse_operand},
};

/// Rewrite a pixel shader so every screen-door pattern it builds from its pixel position is laid out
/// from the origin of the eye half the pixel lies in (see the module docs). Returns
/// [`DxbcError::NoScreenDoor`] for a shader that builds none, and another error for one this rewrite
/// cannot take.
pub fn localize_screen_door(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let shex = dxbc
        .chunk(b"SHEX")
        .ok_or(DxbcError::UnsupportedShaderModel)?;
    let stream = TokenStream::new(shex.body(blob))?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoScreenDoor);
    }
    let isgn = dxbc
        .chunk(b"ISGN")
        .ok_or(DxbcError::MissingInputSignature)?
        .body(blob);
    let (position, _) =
        signature_register(isgn, "SV_Position", 0).ok_or(DxbcError::NoScreenDoor)?;
    let plan = plan(&stream, position)?;
    let new_shex = rewrite(&stream, &plan, position)?;
    let osgn = dxbc
        .chunk(b"OSGN")
        .ok_or(DxbcError::MissingOutputSignature)?
        .body(blob)
        .to_vec();
    reassemble(&dxbc, blob, isgn.to_vec(), osgn, new_shex, false)
}

/// What the scan over the shader learned.
struct Plan {
    /// The temp holding the eye-local position (the old `dcl_temps` count).
    local: u32,
    /// Whether the shader declares any temps (an absent `dcl_temps` has to be added).
    has_temps: bool,
    /// Whether the eye-camera rewrite already declared the buffer the boundary is read from.
    has_buffer: bool,
    /// The token index of the first executable instruction.
    first_instruction: usize,
    /// For every pattern site, the token index of its `dp2` and of the position operand it reads.
    sites: Vec<(usize, usize)>,
}

/// Scan the declarations and the pattern sites, and check the preconditions.
fn plan(stream: &TokenStream, position: u32) -> Result<Plan, DxbcError> {
    let tokens = stream.tokens();
    let mut temps = None;
    let mut has_buffer = false;
    let mut first_instruction = None;
    let mut sites = Vec::new();
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_TEMPS => temps = tokens.get(insn.start + 1).copied(),
                OPCODE_DCL_CONSTANT_BUFFER
                    if declared_register(tokens, &insn)? == EYE_CAMERA_CB_REGISTER =>
                {
                    let rows = tokens
                        .get(insn.start + 3)
                        .ok_or(DxbcError::UnexpectedEndOfTokens)?;
                    if *rows < EYE_CAMERA_CB_ROWS {
                        return Err(DxbcError::ScreenDoorSlotTaken);
                    }
                    has_buffer = true;
                }
                _ => {}
            }
            continue;
        }
        first_instruction.get_or_insert(insn.start);
        if let Some(operand) = pattern_position_operand(tokens, &insn, position)? {
            sites.push((insn.start, operand));
        }
    }
    if sites.is_empty() {
        return Err(DxbcError::NoScreenDoor);
    }
    Ok(Plan {
        local: temps.unwrap_or(0),
        has_temps: temps.is_some(),
        has_buffer,
        first_instruction: first_instruction.unwrap_or(tokens.len()),
        sites,
    })
}

/// The token index of the position operand of a screen-door `dp2`: one source the `SV_Position`
/// input register read through `.x` and `.y` only, the other an immediate vector whose `.xy` is not
/// zero (a zero vector makes the product a constant, which no eye's origin changes).
fn pattern_position_operand(
    tokens: &[u32],
    insn: &Instruction<'_>,
    position: u32,
) -> Result<Option<usize>, DxbcError> {
    if insn.opcode != OPCODE_DP2 {
        return Ok(None);
    }
    let operands = insn.operands().collect::<Result<Vec<_>, _>>()?;
    let [_, a, b] = operands[..] else {
        return Ok(None);
    };
    let is_position = |at: usize| {
        let tok = tokens[at];
        let extended = ((tok >> 31) & 1) as usize;
        (tok >> 12) & 0xFF == OPERAND_TYPE_INPUT
            && (tok >> 20) & 0x3 == 1
            && (tok >> 22) & 0x7 == 0
            && tok & 0x3 == 2
            && (tok >> 2) & 0x3 == SELECTION_SWIZZLE
            && (tok >> 4) & 0x3 <= 1
            && (tok >> 6) & 0x3 <= 1
            && tokens.get(at + 1 + extended) == Some(&position)
    };
    let is_vector_immediate = |at: usize| {
        tokens[at] == OPERAND_IMM32_VECTOR
            && tokens
                .get(at + 1..at + 3)
                .is_some_and(|lanes| lanes.iter().any(|&lane| f32::from_bits(lane) != 0.0))
    };
    Ok(
        if is_position(a.token_offset) && is_vector_immediate(b.token_offset) {
            Some(a.token_offset)
        } else if is_position(b.token_offset) && is_vector_immediate(a.token_offset) {
            Some(b.token_offset)
        } else {
            None
        },
    )
}

/// Rebuild the shader chunk: declare the buffer when the eye-camera rewrite has not, bump the temp
/// count, build the eye-local position before the first instruction, and point every pattern site at
/// it.
fn rewrite(stream: &TokenStream, plan: &Plan, position: u32) -> Result<Vec<u8>, DxbcError> {
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len() + 48);
    out.extend_from_slice(&tokens[..2]);
    let mut injected = false;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.start == plan.first_instruction {
            emit_injection(&mut out, plan, position);
            injected = true;
        }
        if insn.is_declaration() && insn.opcode == OPCODE_DCL_TEMPS {
            out.extend([tokens[insn.start], plan.local + 1]);
            continue;
        }
        match plan.sites.iter().find(|&&(at, _)| at == insn.start) {
            Some(&(_, operand)) => {
                let (_, next) = parse_operand(tokens, operand)?;
                let tok = tokens[operand];
                let extended = ((tok >> 31) & 1) as usize;
                out.extend_from_slice(&tokens[insn.start..operand]);
                out.push(tok & !(0xFF << 12));
                out.extend_from_slice(&tokens[operand + 1..operand + 1 + extended]);
                out.push(plan.local);
                out.extend_from_slice(&tokens[next..insn.end]);
            }
            None => out.extend_from_slice(&tokens[insn.start..insn.end]),
        }
    }
    if !injected {
        emit_injection(&mut out, plan, position);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    Ok(out.iter().flat_map(|t| t.to_le_bytes()).collect())
}

/// Emit the new declarations and the eye-local position: `ge` against the boundary gives all ones
/// at or past it, masking the boundary with that leaves the boundary or zero, and the difference is
/// the eye-local `x`; `y` is copied beside it.
fn emit_injection(out: &mut Vec<u32>, plan: &Plan, position: u32) {
    let local = plan.local;
    if !plan.has_buffer {
        out.extend([
            (4 << 24) | OPCODE_DCL_CONSTANT_BUFFER,
            OPERAND_CB_2D_IMM,
            EYE_CAMERA_CB_REGISTER,
            EYE_CAMERA_CB_ROWS,
        ]);
    }
    if !plan.has_temps {
        out.extend([(2 << 24) | OPCODE_DCL_TEMPS, local + 1]);
    }
    let boundary = [
        OPERAND_CB_2D_SELECT_X,
        EYE_CAMERA_CB_REGISTER,
        EYE_CAMERA_BOUNDARY_ROW,
    ];
    out.extend([(8 << 24) | OPCODE_GE, OPERAND_TEMP_MASK_X, local]);
    out.extend([OPERAND_INPUT_SELECT_X, position]);
    out.extend(boundary);
    out.extend([(8 << 24) | OPCODE_AND, OPERAND_TEMP_MASK_X, local]);
    out.extend([OPERAND_TEMP_SELECT_X, local]);
    out.extend(boundary);
    out.extend([(8 << 24) | OPCODE_ADD, OPERAND_TEMP_MASK_X, local]);
    out.extend([OPERAND_INPUT_SELECT_X, position]);
    out.extend([OPERAND_TEMP_SELECT_X | EXTENDED, MODIFIER_NEGATE, local]);
    out.extend([(5 << 24) | OPCODE_MOV, OPERAND_TEMP_MASK_Y, local]);
    out.extend([OPERAND_INPUT_SELECT_Y, position]);
}

const OPCODE_ADD: u32 = 0x00;
const OPCODE_DP2: u32 = 0x0f;
const OPCODE_GE: u32 = 0x1d;
/// The operand type of a shader input register.
const OPERAND_TYPE_INPUT: u32 = 1;
/// The swizzle selection mode of a four-component operand.
const SELECTION_SWIZZLE: u32 = 1;
/// A four-component 32-bit immediate operand, its values following inline.
const OPERAND_IMM32_VECTOR: u32 = 0x0000_4002;
/// A constant-buffer operand with two immediate indices selecting its `.x`.
const OPERAND_CB_2D_SELECT_X: u32 = 0x0020_800A;
/// A temp register operand writing `.y`.
const OPERAND_TEMP_MASK_Y: u32 = 0x0010_0022;
/// An input register operand selecting `.y`.
const OPERAND_INPUT_SELECT_Y: u32 = 0x0010_101A;
/// The bit of an operand token announcing an extended operand token.
const EXTENDED: u32 = 1 << 31;
/// An extended operand token carrying the negate modifier.
const MODIFIER_NEGATE: u32 = 0x41;
