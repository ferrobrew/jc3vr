//! Shade each half of a double-wide target from its own eye's camera position.
//!
//! The engine stages the camera world position into the fragment `GlobalConstants` (`cb0`) once per
//! pass, and pixel shaders measure everything view-dependent from it: the view vector of their
//! specular, Fresnel, sheen, and environment terms, camera distances, and the origin the sun-shadow
//! cascade lookup subtracts before applying the cascade transform, whose translation row is baked
//! relative to that same position:
//!
//! ```text
//! add r1.xyz, v5.xyzx, -cb0[5].xyzx        // the view vector, from the interpolated world position
//! add r5.xyz, v5.xyzx, -cb0[4].xyzx        // the cascade lookup's camera-relative position
//! mad r8.yzw, r5.zzzz, cb0[47].xxyz, r8.yyzw
//! add r8.yzw, r8.yyzw, cb0[48].xxyz        // the cascade translation
//! ```
//!
//! A draw that covers both eye halves of a double-wide target in one submission reads one set of
//! rows for both. [`localize_eye_camera`] makes each pixel read its own eye's: the eye is the half of
//! the target the pixel lies in, found by comparing `SV_Position.x` against a boundary held in a
//! mod-owned constant buffer ([`EYE_CAMERA_CB_REGISTER`]), and every read of one of the camera rows
//! ([`EYE_CAMERA_SOURCE_ROWS`]) becomes a read of that eye's copy of the row. The buffer holds
//! [`EYE_CAMERA_ROWS_PER_EYE`] rows per eye, in the order of [`EYE_CAMERA_SOURCE_ROWS`], then the
//! boundary in `.x` of [`EYE_CAMERA_BOUNDARY_ROW`]. The boundary is a pixel coordinate the caller
//! writes for the bound target, so the rewrite needs no size constant; a boundary no pixel reaches,
//! with both eyes' rows equal to the staged ones, makes the rewritten shader read exactly what the
//! original reads.
//!
//! A shader is declined when it reads none of the camera rows, already declares the buffer's slot,
//! has no `SV_Position` input to select its eye by, or indexes `cb0` dynamically from a base that could
//! reach one of the camera rows.

use crate::{
    container::{Dxbc, DxbcError},
    rewrite::common::{
        CB_ACCESS_DYNAMIC_INDEXED, OPCODE_AND, OPCODE_DCL_CONSTANT_BUFFER, OPCODE_DCL_TEMPS,
        OPERAND_CB_2D_IMM, OPERAND_IMM32_SCALAR, OPERAND_INPUT_MASK_X, OPERAND_INPUT_SELECT_X,
        OPERAND_TEMP_MASK_X, OPERAND_TEMP_SELECT_X, SB_NAME_POSITION, declared_register,
        reassemble,
    },
    tokens::{
        IDX_IMM32_PLUS_RELATIVE, Instruction, OperandKind, ShaderStage, TokenStream, parse_operand,
    },
};

/// The pixel-stage constant-buffer slot the rewritten shader reads each eye's camera rows and the
/// eye boundary from. No game pixel shader declares it.
pub const EYE_CAMERA_CB_REGISTER: u32 = 11;
/// The fragment `GlobalConstants` rows the rewrite gives each eye its own copy of, in the order each
/// eye's block lays them out: the two camera world position rows, then the sun-shadow cascade
/// transform's translation row, which is baked relative to the camera position.
pub const EYE_CAMERA_SOURCE_ROWS: [u32; 3] = [4, 5, 48];
/// The rows each eye occupies in [`EYE_CAMERA_CB_REGISTER`].
pub const EYE_CAMERA_ROWS_PER_EYE: u32 = EYE_CAMERA_SOURCE_ROWS.len() as u32;
/// The row whose `.x` holds the eye boundary: the first pixel coordinate of the right eye's half.
pub const EYE_CAMERA_BOUNDARY_ROW: u32 = 2 * EYE_CAMERA_ROWS_PER_EYE;
/// The size of [`EYE_CAMERA_CB_REGISTER`] in float4 rows.
pub const EYE_CAMERA_CB_ROWS: u32 = EYE_CAMERA_BOUNDARY_ROW + 1;

/// Rewrite a pixel shader so every read of the camera rows takes the eye of the target half the pixel
/// lies in (see the module docs). Returns [`DxbcError::NoCameraPosition`] for a shader that reads
/// none of them, and another error for one this rewrite cannot take.
pub fn localize_eye_camera(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let shex = dxbc
        .chunk(b"SHEX")
        .ok_or(DxbcError::UnsupportedShaderModel)?;
    let stream = TokenStream::new(shex.body(blob))?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoCameraPosition);
    }
    let plan = plan(&stream)?;
    let isgn = dxbc
        .chunk(b"ISGN")
        .ok_or(DxbcError::MissingInputSignature)?
        .body(blob);
    let position = position_input(isgn).ok_or(DxbcError::NoPixelPosition)?;
    let new_isgn = mark_position_read(isgn, position.record)?;
    let new_shex = rewrite(&stream, &plan, position.register)?;
    let osgn = dxbc
        .chunk(b"OSGN")
        .ok_or(DxbcError::MissingOutputSignature)?
        .body(blob)
        .to_vec();
    reassemble(&dxbc, blob, new_isgn, osgn, new_shex, false)
}

/// What the scan over the shader learned.
struct Plan {
    /// The temp holding the eye's first row in the mod-owned buffer (the old `dcl_temps` count).
    eye: u32,
    /// Whether the shader declares any temps (an absent `dcl_temps` has to be added).
    has_temps: bool,
    /// The token index of the `SV_Position` input declaration and its component mask, when the
    /// shader declares it.
    position_declaration: Option<(usize, u32)>,
    /// The token index of the first executable instruction.
    first_instruction: usize,
}

/// Scan the declarations and the camera-row reads, and check the preconditions.
fn plan(stream: &TokenStream) -> Result<Plan, DxbcError> {
    let tokens = stream.tokens();
    let mut temps = None;
    let mut position_declaration = None;
    let mut first_instruction = None;
    let mut reads_camera = false;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_TEMPS => temps = tokens.get(insn.start + 1).copied(),
                OPCODE_DCL_CONSTANT_BUFFER
                    if declared_register(tokens, &insn)? == EYE_CAMERA_CB_REGISTER =>
                {
                    return Err(DxbcError::EyeCameraSlotTaken);
                }
                OPCODE_DCL_INPUT_PS_SIV if tokens.get(insn.end - 1) == Some(&SB_NAME_POSITION) => {
                    let operand = *tokens
                        .get(insn.start + 1)
                        .ok_or(DxbcError::UnexpectedEndOfTokens)?;
                    position_declaration = Some((insn.start, operand));
                }
                _ => {}
            }
            continue;
        }
        first_instruction.get_or_insert(insn.start);
        for operand in insn.operands() {
            match operand?.kind {
                OperandKind::ConstantBuffer {
                    register: 0,
                    element,
                } if EYE_CAMERA_SOURCE_ROWS.contains(&element) => reads_camera = true,
                OperandKind::ConstantBufferDynamic { register: 0 } => {
                    check_dynamic_base(tokens, &insn)?;
                }
                _ => {}
            }
        }
    }
    if !reads_camera {
        return Err(DxbcError::NoCameraPosition);
    }
    Ok(Plan {
        eye: temps.unwrap_or(0),
        has_temps: temps.is_some(),
        position_declaration,
        first_instruction: first_instruction.unwrap_or(tokens.len()),
    })
}

/// Decline an instruction that indexes `cb0` dynamically unless every such index starts past the
/// last camera row. The compiler emits a dynamic index as an immediate array base plus a non-negative
/// element offset, so a base past the last camera row cannot reach one; a purely relative index, or a
/// base at or before it, could.
fn check_dynamic_base(tokens: &[u32], insn: &Instruction<'_>) -> Result<(), DxbcError> {
    let last = *EYE_CAMERA_SOURCE_ROWS.iter().max().expect("non-empty");
    for operand in insn.operands() {
        let operand = operand?;
        if !matches!(
            operand.kind,
            OperandKind::ConstantBufferDynamic { register: 0 }
        ) {
            continue;
        }
        let tok = tokens[operand.token_offset];
        let extended = ((tok >> 31) & 1) as usize;
        let base = ((tok >> 25) & 0x7 == IDX_IMM32_PLUS_RELATIVE)
            .then(|| tokens.get(operand.token_offset + 2 + extended).copied())
            .flatten();
        if !base.is_some_and(|base| base > last) {
            return Err(DxbcError::DynamicCameraAccess);
        }
    }
    Ok(())
}

/// The `SV_Position` input's register, and the byte offset of its signature record.
struct PositionInput {
    register: u32,
    record: usize,
}

/// Find the `SV_Position` element of an input signature.
fn position_input(isgn: &[u8]) -> Option<PositionInput> {
    let count = u32::from_le_bytes(isgn.get(0..4)?.try_into().ok()?) as usize;
    let table = u32::from_le_bytes(isgn.get(4..8)?.try_into().ok()?) as usize;
    (0..count).find_map(|i| {
        let record = table.checked_add(i.checked_mul(SIGNATURE_ELEMENT_LEN)?)?;
        let fields = isgn.get(record..record + SIGNATURE_ELEMENT_LEN)?;
        let name = u32::from_le_bytes(fields[0..4].try_into().ok()?) as usize;
        let system_value = u32::from_le_bytes(fields[8..12].try_into().ok()?);
        let register = u32::from_le_bytes(fields[16..20].try_into().ok()?);
        let name = isgn.get(name..)?.split(|&b| b == 0).next()?;
        (system_value == SIGNATURE_SYSVALUE_POSITION && name.eq_ignore_ascii_case(b"SV_Position"))
            .then_some(PositionInput { register, record })
    })
}

/// Add `.x` to the read mask of the `SV_Position` element, so the signature agrees with the
/// declaration the rewrite reads it through.
fn mark_position_read(isgn: &[u8], record: usize) -> Result<Vec<u8>, DxbcError> {
    let mut out = isgn.to_vec();
    let read_mask = out
        .get_mut(record + 21)
        .ok_or(DxbcError::MalformedSignature)?;
    *read_mask |= 0x01;
    Ok(out)
}

/// Rebuild the shader chunk: declare the buffer, declare or widen the position input, bump the temp
/// count, select the eye before the first instruction, and point every camera-row read at the eye's
/// copy.
fn rewrite(stream: &TokenStream, plan: &Plan, position: u32) -> Result<Vec<u8>, DxbcError> {
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len() + 64);
    out.extend_from_slice(&tokens[..2]);
    let mut injected = false;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.start == plan.first_instruction {
            emit_injection(&mut out, plan, position);
            injected = true;
        }
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_TEMPS => out.extend([tokens[insn.start], plan.eye + 1]),
                _ if plan.position_declaration.map(|(at, _)| at) == Some(insn.start) => {
                    out.push(tokens[insn.start]);
                    out.push(tokens[insn.start + 1] | MASK_X);
                    out.extend_from_slice(&tokens[insn.start + 2..insn.end]);
                }
                _ => out.extend_from_slice(&tokens[insn.start..insn.end]),
            }
            continue;
        }
        rewrite_instruction(tokens, &insn, plan.eye, &mut out)?;
    }
    if !injected {
        emit_injection(&mut out, plan, position);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    Ok(out.iter().flat_map(|t| t.to_le_bytes()).collect())
}

/// Emit the new declarations and the eye selection: `ge` against the boundary gives all ones in the
/// right eye's half, and masking it leaves the eye's first row.
fn emit_injection(out: &mut Vec<u32>, plan: &Plan, position: u32) {
    let eye = plan.eye;
    out.extend([
        (4 << 24) | CB_ACCESS_DYNAMIC_INDEXED | OPCODE_DCL_CONSTANT_BUFFER,
        OPERAND_CB_2D_IMM,
        EYE_CAMERA_CB_REGISTER,
        EYE_CAMERA_CB_ROWS,
    ]);
    if plan.position_declaration.is_none() {
        out.extend([
            (4 << 24) | (INTERPOLATION_LINEAR_NOPERSPECTIVE << 11) | OPCODE_DCL_INPUT_PS_SIV,
            OPERAND_INPUT_MASK_X,
            position,
            SB_NAME_POSITION,
        ]);
    }
    if !plan.has_temps {
        out.extend([(2 << 24) | OPCODE_DCL_TEMPS, eye + 1]);
    }
    out.extend([
        (8 << 24) | OPCODE_GE,
        OPERAND_TEMP_MASK_X,
        eye,
        OPERAND_INPUT_SELECT_X,
        position,
        OPERAND_CB_2D_SELECT_X,
        EYE_CAMERA_CB_REGISTER,
        EYE_CAMERA_BOUNDARY_ROW,
    ]);
    out.extend([
        (7 << 24) | OPCODE_AND,
        OPERAND_TEMP_MASK_X,
        eye,
        OPERAND_TEMP_SELECT_X,
        eye,
        OPERAND_IMM32_SCALAR,
        EYE_CAMERA_ROWS_PER_EYE,
    ]);
}

/// Re-serialize one instruction, turning every camera-row operand `cb0[row]` into the eye's copy
/// `cbN[rEye.x + k]`, where `k` is the row's place in [`EYE_CAMERA_SOURCE_ROWS`]. The swizzle,
/// modifiers, and extended operand token carry over unchanged.
fn rewrite_instruction(
    tokens: &[u32],
    insn: &Instruction<'_>,
    eye: u32,
    out: &mut Vec<u32>,
) -> Result<(), DxbcError> {
    let opcode_at = out.len();
    out.extend_from_slice(&tokens[insn.start..insn.operands_start()]);
    let mut pos = insn.operands_start();
    while pos < insn.end {
        let (operand, next) = parse_operand(tokens, pos)?;
        let slot = match operand.kind {
            OperandKind::ConstantBuffer {
                register: 0,
                element,
            } => EYE_CAMERA_SOURCE_ROWS
                .iter()
                .position(|&row| row == element),
            _ => None,
        };
        let Some(slot) = slot else {
            out.extend_from_slice(&tokens[pos..next]);
            pos = next;
            continue;
        };
        let tok = tokens[pos];
        if (tok >> 22) & 0x7 != 0 || (tok >> 25) & 0x7 != 0 {
            return Err(DxbcError::UnsupportedOperandEncoding);
        }
        let extended = ((tok >> 31) & 1) as usize;
        out.push(tok | (IDX_IMM32_PLUS_RELATIVE << 25));
        out.extend_from_slice(&tokens[pos + 1..pos + 1 + extended]);
        out.push(EYE_CAMERA_CB_REGISTER);
        out.push(slot as u32);
        out.push(OPERAND_TEMP_SELECT_X);
        out.push(eye);
        pos = next;
    }
    let length = (out.len() - opcode_at) as u32;
    if length > 0x7F {
        return Err(DxbcError::InstructionTooLong);
    }
    out[opcode_at] = (out[opcode_at] & !(0x7F << 24)) | (length << 24);
    Ok(())
}

const OPCODE_GE: u32 = 0x1d;
const OPCODE_DCL_INPUT_PS_SIV: u32 = 0x64;
/// The interpolation mode `SV_Position` is declared with.
const INTERPOLATION_LINEAR_NOPERSPECTIVE: u32 = 4;
/// A constant-buffer operand with two immediate indices selecting its `.x`.
const OPERAND_CB_2D_SELECT_X: u32 = 0x0020_800A;
/// The `.x` bit of an operand's component mask.
const MASK_X: u32 = 1 << 4;
/// The signature system-value code of `SV_Position`.
const SIGNATURE_SYSVALUE_POSITION: u32 = 1;
/// The fixed size of a signature element record.
const SIGNATURE_ELEMENT_LEN: usize = 24;
