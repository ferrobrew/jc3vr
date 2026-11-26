//! Write each half of a double-wide target's motion vectors in its own eye's screen space.
//!
//! The engine's velocity pixel shaders are handed two clip positions by their vertex shader, the
//! current one and the previous frame's, and encode the screen-space motion between them:
//!
//! ```text
//! mov r0.xy, v2.xyxx                   // the previous clip position
//! mov r0.zw, v1.xxxy                   // the current clip position
//! mov r1.x, v2.w
//! mov r1.z, v1.w
//! div r0.xyzw, r0.xyzw, r1.xxzz        // both, in NDC
//! mad r0.xyzw, r0.xyzw, l(0.5, -0.5, 0.5, -0.5), l(0.5, 0.5, 0.5, 0.5)
//! add r0.xy, -r0.xyxx, r0.zwzz         // current UV less previous UV
//! ```
//!
//! In an eye-sized render the current clip position is the one the vertex shader rasterizes, so its
//! NDC is the pixel's own, and the previous one comes through the render camera's previous
//! view-projection, which for an eye render is that eye's camera. Under the double-wide collapse
//! neither is in the eye's space. The current position is the eye's clip position only where the
//! vertex shader computes it from the per-eye rows the remap gives it, and the centred camera's where
//! the shader is reprojected after the fact (the reprojection moves `SV_Position` alone). The previous
//! position comes through the centred camera's previous view-projection over the double-wide target.
//!
//! [`localize_velocity`] replaces both right after the divide. The current NDC becomes the pixel's own
//! NDC in its eye's half, built from `SV_Position`: `x` scaled by the clip-space width of a pixel from
//! the left edge of the eye the pixel lies in (found against the eye boundary of
//! [`EYE_CAMERA_BOUNDARY_ROW`]), and `y` scaled from the target's top edge. The previous NDC is
//! rebuilt from the interpolated previous clip position taken through the eye's matrix from the
//! render camera's previous clip space to the eye's ([`VELOCITY_REPROJECTION_ROW`]), which the caller
//! builds from the eyes' rigid offsets from the camera.
//! Both are blended in by the weight in `.w` of [`VELOCITY_PIXEL_ROW`], one in an eye-split pass;
//! outside one the payload writes zeros, and an unbound buffer reads as zeros too, so the shader then
//! subtracts what the engine's does. The previous position is blended in clip space, before the
//! divide, so zero matrices cannot produce a NaN.
//!
//! ```text
//! ge   e.x, v0.x, boundary.x            // all ones in the right eye's half
//! movc m0, e.xxxx, eye1.row0, eye0.row0 // the eye's matrix rows (m1 and m3 likewise)
//! movc e.y, e.x, origin.y, origin.x     // the clip-space x of the eye's left edge
//! dp4  m0.x, m0, v2                     // the previous clip position in the eye's clip space
//! dp4  m0.y, m1, v2
//! dp4  m0.z, m3, v2
//! add  m0.xyz, m0.xyzw, -v2.xyww
//! mad  m0.xyz, pixel.wwww, m0.xyzw, v2.xyww
//! div  r0.xy, m0.xyxx, m0.zzzz          // the previous NDC
//! mad  e.x, v0.x, pixel.x, e.y          // the pixel's NDC x in its eye
//! mad  e.y, v0.y, pixel.y, pixel.z      // the pixel's NDC y
//! add  e.xy, e.xyxx, -r0.zwzz
//! mad  r0.zw, pixel.wwww, e.xxxy, r0.zzzw
//! ```
//!
//! The rewrite composes after the eye-camera and screen-door rewrites: a slot declared at least
//! [`EYE_CAMERA_CB_ROWS`] long is the eye-camera buffer and is read as it is; a shorter one is not,
//! and the shader is declined.

use crate::{
    container::{Dxbc, DxbcError},
    isa::{
        INTERPOLATION_LINEAR_NOPERSPECTIVE, MODIFIER_NEG, OPCODE_ADD, OPCODE_DCL_CONSTANT_BUFFER,
        OPCODE_DCL_INPUT_PS, OPCODE_DCL_INPUT_PS_SIV, OPCODE_DCL_TEMPS, OPCODE_DIV, OPCODE_DP4,
        OPCODE_GE, OPCODE_MAD, OPCODE_MOV, OPCODE_MOVC, OPERAND_CB_2D_SELECT_X,
        OPERAND_CB_2D_SELECT_Z, OPERAND_EXTENDED, OPERAND_IMM32_VECTOR, OPERAND_INPUT_MASK_XY,
        OPERAND_INPUT_SELECT_X, OPERAND_INPUT_SELECT_Y, OPERAND_INPUT_SWIZZLE_XYXX,
        OPERAND_TEMP_MASK_X, OPERAND_TEMP_MASK_XY, OPERAND_TEMP_MASK_XYZW, OPERAND_TEMP_MASK_Y,
        OPERAND_TEMP_MASK_Z, OPERAND_TEMP_MASK_ZW, OPERAND_TEMP_SELECT_X,
        OPERAND_TEMP_SWIZZLE_XYXX, OPERAND_TEMP_SWIZZLE_XYZW, OPERAND_TEMP_SWIZZLE_ZWZZ,
        SB_NAME_POSITION, SIGNATURE_SYSVALUE_POSITION,
    },
    rewrite::{
        camera::{
            EYE_CAMERA_BOUNDARY_ROW, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_CB_ROWS,
            VELOCITY_ORIGIN_ROW, VELOCITY_PIXEL_ROW, VELOCITY_REPROJECTION_ROW,
            VELOCITY_REPROJECTION_ROWS_PER_EYE,
        },
        common::{OPERAND_CB_2D_IMM, declared_register, reassemble, signature_register},
    },
    tokens::{Instruction, ShaderStage, TokenStream},
};

/// Rewrite a velocity pixel shader so the motion vector it writes is measured in the screen space of
/// the eye whose half the pixel lies in (see the module docs). Returns [`DxbcError::NoVelocity`] for
/// a shader that writes none, and another error for one this rewrite cannot take.
pub fn localize_velocity(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let shex = dxbc
        .chunk(b"SHEX")
        .ok_or(DxbcError::UnsupportedShaderModel)?;
    let stream = TokenStream::new(shex.body(blob))?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoVelocity);
    }
    let plan = plan(&stream)?;
    let isgn = dxbc
        .chunk(b"ISGN")
        .ok_or(DxbcError::MissingInputSignature)?
        .body(blob);
    let (position, _) =
        signature_register(isgn, "SV_Position", 0).ok_or(DxbcError::NoPixelPosition)?;
    let new_isgn = mark_inputs_read(isgn, &plan)?;
    let new_shex = rewrite(&stream, &plan, position)?;
    let osgn = dxbc
        .chunk(b"OSGN")
        .ok_or(DxbcError::MissingOutputSignature)?
        .body(blob)
        .to_vec();
    reassemble(&dxbc, blob, new_isgn, osgn, new_shex, false)
}

/// What the scan over the shader learned.
struct Plan {
    /// The temp the correction computes into (the old `dcl_temps` count).
    scratch: u32,
    /// Whether the shader declares any temps (an absent `dcl_temps` has to be added).
    has_temps: bool,
    /// The token index of an earlier rewrite's declaration of the eye-camera buffer.
    buffer_declaration: Option<usize>,
    /// The token index of the `SV_Position` input declaration, when the shader declares it.
    position_declaration: Option<usize>,
    /// The token index of the first executable instruction.
    first_instruction: usize,
    /// For every encoding site, where its correction goes and what it reads and writes.
    sites: Vec<Site>,
    /// The token index of each previous position's input declaration.
    previous_declarations: Vec<usize>,
}

/// One encoding site.
#[derive(Clone, Copy)]
struct Site {
    /// The token index of the instruction after the divide.
    at: usize,
    /// The temp holding the two positions.
    positions: u32,
    /// The input register the previous clip position is interpolated in.
    previous: u32,
}

/// Scan the declarations and the encoding sites, and check the preconditions.
fn plan(stream: &TokenStream) -> Result<Plan, DxbcError> {
    let tokens = stream.tokens();
    let mut temps = None;
    let mut buffer_declaration = None;
    let mut position_declaration = None;
    let mut input_declarations = Vec::new();
    let mut first_instruction = None;
    let mut sites = Vec::new();
    let mut previous_mov: Option<(u32, u32)> = None;
    let mut window: [Option<Instruction<'_>>; 2] = [None, None];
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
                        return Err(DxbcError::VelocitySlotTaken);
                    }
                    buffer_declaration = Some(insn.start);
                }
                OPCODE_DCL_INPUT_PS_SIV if tokens.get(insn.end - 1) == Some(&SB_NAME_POSITION) => {
                    position_declaration = Some(insn.start);
                }
                OPCODE_DCL_INPUT_PS => {
                    input_declarations.push((declared_register(tokens, &insn)?, insn.start));
                }
                _ => {}
            }
            continue;
        }
        first_instruction.get_or_insert(insn.start);
        if let [
            MOV_5,
            OPERAND_TEMP_MASK_XY,
            positions,
            OPERAND_INPUT_SWIZZLE_XYXX,
            input,
        ] = tokens[insn.start..insn.end]
        {
            previous_mov = Some((positions, input));
        }
        if let [Some(div), Some(mad)] = &window
            && let Some(positions) = encoding_site(tokens, div, mad, &insn)
        {
            let (_, previous) = previous_mov
                .filter(|&(temp, _)| temp == positions)
                .ok_or(DxbcError::NoVelocity)?;
            sites.push(Site {
                at: mad.start,
                positions,
                previous,
            });
        }
        window = [window[1].take(), Some(insn)];
    }
    if sites.is_empty() {
        return Err(DxbcError::NoVelocity);
    }
    let previous_declarations = sites
        .iter()
        .map(|site| {
            input_declarations
                .iter()
                .find(|&&(register, _)| register == site.previous)
                .map(|&(_, at)| at)
                .ok_or(DxbcError::NoVelocity)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Plan {
        scratch: temps.unwrap_or(0),
        has_temps: temps.is_some(),
        buffer_declaration,
        position_declaration,
        first_instruction: first_instruction.unwrap_or(tokens.len()),
        sites,
        previous_declarations,
    })
}

/// The temp holding the two positions, when `div`, `mad`, and `add` are the encoding's divide, its
/// mapping to UV, and its subtraction: a temp's `.xyzw` divided by `w`s in place, mapped by
/// `(0.5, -0.5)` and offset by `0.5`, and its `.xy` (the previous UV) subtracted from its `.zw` (the
/// current UV) into its `.xy`.
fn encoding_site(
    tokens: &[u32],
    div: &Instruction<'_>,
    mad: &Instruction<'_>,
    add: &Instruction<'_>,
) -> Option<u32> {
    let [
        DIV_7,
        OPERAND_TEMP_MASK_XYZW,
        positions,
        OPERAND_TEMP_SWIZZLE_XYZW,
        numerator,
        denominator,
        _,
    ] = tokens[div.start..div.end]
    else {
        return None;
    };
    if denominator & 0x000F_F00F != OPERAND_TEMP_SWIZZLE_XYZW & 0x000F_F00F
        || numerator != positions
    {
        return None;
    }
    let expected_mad = [
        MAD_15,
        OPERAND_TEMP_MASK_XYZW,
        positions,
        OPERAND_TEMP_SWIZZLE_XYZW,
        positions,
        OPERAND_IMM32_VECTOR,
        HALF,
        NEGATIVE_HALF,
        HALF,
        NEGATIVE_HALF,
        OPERAND_IMM32_VECTOR,
        HALF,
        HALF,
        HALF,
        HALF,
    ];
    let expected_add = [
        ADD_8,
        OPERAND_TEMP_MASK_XY,
        positions,
        OPERAND_TEMP_SWIZZLE_XYXX | OPERAND_EXTENDED,
        MODIFIER_NEG,
        positions,
        OPERAND_TEMP_SWIZZLE_ZWZZ,
        positions,
    ];
    (tokens[mad.start..mad.end] == expected_mad && tokens[add.start..add.end] == expected_add)
        .then_some(positions)
}

/// Mark the signature elements the rewrite reads through as read: `SV_Position`'s `.xy`, and every
/// component of each previous position's element, so the signature agrees with the declarations.
fn mark_inputs_read(isgn: &[u8], plan: &Plan) -> Result<Vec<u8>, DxbcError> {
    let field = |at: usize| {
        isgn.get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")) as usize)
            .ok_or(DxbcError::MalformedSignature)
    };
    let (count, table) = (field(0)?, field(4)?);
    let mut out = isgn.to_vec();
    let mut position = false;
    for i in 0..count {
        let record = table + i * SIGNATURE_ELEMENT_LEN;
        let fields = isgn
            .get(record..record + SIGNATURE_ELEMENT_LEN)
            .ok_or(DxbcError::MalformedSignature)?;
        let system_value = u32::from_le_bytes(fields[8..12].try_into().expect("4 bytes"));
        let register = u32::from_le_bytes(fields[16..20].try_into().expect("4 bytes"));
        if system_value == SIGNATURE_SYSVALUE_POSITION {
            out[record + 21] |= 0x03;
            position = true;
        } else if plan.sites.iter().any(|site| site.previous == register) {
            out[record + 21] |= fields[20];
        }
    }
    position.then_some(out).ok_or(DxbcError::NoPixelPosition)
}

/// Rebuild the shader chunk: declare the buffer when no earlier rewrite has, declare or widen the position input to `.xy`, widen each
/// previous position's input to `.xyzw`, bump the temp count, and correct the positions after every
/// divide.
fn rewrite(stream: &TokenStream, plan: &Plan, position: u32) -> Result<Vec<u8>, DxbcError> {
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len() + 96 * plan.sites.len());
    out.extend_from_slice(&tokens[..2]);
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.start == plan.first_instruction {
            emit_declarations(&mut out, plan, position);
        }
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_TEMPS => out.extend([tokens[insn.start], plan.scratch + SCRATCH_TEMPS]),
                _ if plan.position_declaration == Some(insn.start) => {
                    out.push(tokens[insn.start]);
                    out.push(tokens[insn.start + 1] | MASK_XY);
                    out.extend_from_slice(&tokens[insn.start + 2..insn.end]);
                }
                _ if plan.previous_declarations.contains(&insn.start) => {
                    out.push(tokens[insn.start]);
                    out.push(tokens[insn.start + 1] | MASK_XYZW);
                    out.extend_from_slice(&tokens[insn.start + 2..insn.end]);
                }
                _ => out.extend_from_slice(&tokens[insn.start..insn.end]),
            }
            continue;
        }
        if let Some(site) = plan.sites.iter().find(|site| site.at == insn.start) {
            emit_correction(&mut out, plan.scratch, *site, position);
        }
        out.extend_from_slice(&tokens[insn.start..insn.end]);
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    Ok(out.iter().flat_map(|t| t.to_le_bytes()).collect())
}

/// Emit the declarations the correction needs ahead of the first instruction: the buffer, the
/// position input, and the temps, each when the shader lacks it.
fn emit_declarations(out: &mut Vec<u32>, plan: &Plan, position: u32) {
    if plan.buffer_declaration.is_none() {
        out.extend([
            (4 << 24) | OPCODE_DCL_CONSTANT_BUFFER,
            OPERAND_CB_2D_IMM,
            EYE_CAMERA_CB_REGISTER,
            EYE_CAMERA_CB_ROWS,
        ]);
    }
    if plan.position_declaration.is_none() {
        out.extend([
            (4 << 24) | (INTERPOLATION_LINEAR_NOPERSPECTIVE << 11) | OPCODE_DCL_INPUT_PS_SIV,
            OPERAND_INPUT_MASK_XY,
            position,
            SB_NAME_POSITION,
        ]);
    }
    if !plan.has_temps {
        out.extend([(2 << 24) | OPCODE_DCL_TEMPS, plan.scratch + SCRATCH_TEMPS]);
    }
}

/// Emit the correction of the module docs for `site`, computing in the [`SCRATCH_TEMPS`] temps from
/// `scratch` on.
fn emit_correction(out: &mut Vec<u32>, scratch: u32, site: Site, position: u32) {
    let Site {
        positions,
        previous,
        ..
    } = site;
    let [eye, row0, row1, row3] = [scratch, scratch + 1, scratch + 2, scratch + 3];
    let cb = |select: u32, row: u32| [select, EYE_CAMERA_CB_REGISTER, row];
    let row = |eye: u32, row: u32| {
        VELOCITY_REPROJECTION_ROW + eye * VELOCITY_REPROJECTION_ROWS_PER_EYE + row
    };
    out.extend([(8 << 24) | OPCODE_GE, OPERAND_TEMP_MASK_X, eye]);
    out.extend([OPERAND_INPUT_SELECT_X, position]);
    out.extend(cb(OPERAND_CB_2D_SELECT_X, EYE_CAMERA_BOUNDARY_ROW));
    for (temp, r) in [(row0, 0), (row1, 1), (row3, 3)] {
        out.extend([(11 << 24) | OPCODE_MOVC, OPERAND_TEMP_MASK_XYZW, temp]);
        out.extend([OPERAND_TEMP_SWIZZLE_XXXX, eye]);
        out.extend(cb(OPERAND_CB_2D_IMM, row(1, r)));
        out.extend(cb(OPERAND_CB_2D_IMM, row(0, r)));
    }
    out.extend([(11 << 24) | OPCODE_MOVC, OPERAND_TEMP_MASK_Y, eye]);
    out.extend([OPERAND_TEMP_SELECT_X, eye]);
    out.extend(cb(OPERAND_CB_2D_SELECT_Y, VELOCITY_ORIGIN_ROW));
    out.extend(cb(OPERAND_CB_2D_SELECT_X, VELOCITY_ORIGIN_ROW));
    out.extend([(7 << 24) | OPCODE_DP4, OPERAND_TEMP_MASK_X, row0]);
    out.extend([
        OPERAND_TEMP_SWIZZLE_XYZW,
        row0,
        OPERAND_INPUT_SWIZZLE_XYZW,
        previous,
    ]);
    out.extend([(7 << 24) | OPCODE_DP4, OPERAND_TEMP_MASK_Y, row0]);
    out.extend([
        OPERAND_TEMP_SWIZZLE_XYZW,
        row1,
        OPERAND_INPUT_SWIZZLE_XYZW,
        previous,
    ]);
    out.extend([(7 << 24) | OPCODE_DP4, OPERAND_TEMP_MASK_Z, row0]);
    out.extend([
        OPERAND_TEMP_SWIZZLE_XYZW,
        row3,
        OPERAND_INPUT_SWIZZLE_XYZW,
        previous,
    ]);
    out.extend([(8 << 24) | OPCODE_ADD, OPERAND_TEMP_MASK_XYZ, row0]);
    out.extend([OPERAND_TEMP_SWIZZLE_XYZW, row0]);
    out.extend([
        OPERAND_INPUT_SWIZZLE_XYWW | OPERAND_EXTENDED,
        MODIFIER_NEG,
        previous,
    ]);
    out.extend([(10 << 24) | OPCODE_MAD, OPERAND_TEMP_MASK_XYZ, row0]);
    out.extend(cb(OPERAND_CB_2D_SWIZZLE_WWWW, VELOCITY_PIXEL_ROW));
    out.extend([
        OPERAND_TEMP_SWIZZLE_XYZW,
        row0,
        OPERAND_INPUT_SWIZZLE_XYWW,
        previous,
    ]);
    out.extend([(7 << 24) | OPCODE_DIV, OPERAND_TEMP_MASK_XY, positions]);
    out.extend([
        OPERAND_TEMP_SWIZZLE_XYXX,
        row0,
        OPERAND_TEMP_SWIZZLE_ZZZZ,
        row0,
    ]);
    out.extend([(10 << 24) | OPCODE_MAD, OPERAND_TEMP_MASK_X, eye]);
    out.extend([OPERAND_INPUT_SELECT_X, position]);
    out.extend(cb(OPERAND_CB_2D_SELECT_X, VELOCITY_PIXEL_ROW));
    out.extend([OPERAND_TEMP_SELECT_Y, eye]);
    out.extend([(11 << 24) | OPCODE_MAD, OPERAND_TEMP_MASK_Y, eye]);
    out.extend([OPERAND_INPUT_SELECT_Y, position]);
    out.extend(cb(OPERAND_CB_2D_SELECT_Y, VELOCITY_PIXEL_ROW));
    out.extend(cb(OPERAND_CB_2D_SELECT_Z, VELOCITY_PIXEL_ROW));
    out.extend([ADD_8, OPERAND_TEMP_MASK_XY, eye]);
    out.extend([OPERAND_TEMP_SWIZZLE_XYXX, eye]);
    out.extend([
        OPERAND_TEMP_SWIZZLE_ZWZZ | OPERAND_EXTENDED,
        MODIFIER_NEG,
        positions,
    ]);
    out.extend([(10 << 24) | OPCODE_MAD, OPERAND_TEMP_MASK_ZW, positions]);
    out.extend(cb(OPERAND_CB_2D_SWIZZLE_WWWW, VELOCITY_PIXEL_ROW));
    out.extend([
        OPERAND_TEMP_SWIZZLE_XXXY,
        eye,
        OPERAND_TEMP_SWIZZLE_ZZZW,
        positions,
    ]);
}

/// The temps the correction computes in: the eye and its NDC, then the eye's three matrix rows.
const SCRATCH_TEMPS: u32 = 4;

/// The opcode tokens of a five-token `mov`, a seven-token `div`, an eight-token `add`, and a
/// fifteen-token `mad`.
const MOV_5: u32 = (5 << 24) | OPCODE_MOV;
const DIV_7: u32 = (7 << 24) | OPCODE_DIV;
const ADD_8: u32 = (8 << 24) | OPCODE_ADD;
const MAD_15: u32 = (15 << 24) | OPCODE_MAD;
/// `0.5` and `-0.5` as immediate lanes.
const HALF: u32 = 0x3f00_0000;
const NEGATIVE_HALF: u32 = 0xbf00_0000;
/// The `.xy` and `.xyzw` component-mask bits of a masked operand token.
const MASK_XY: u32 = 0x30;
const MASK_XYZW: u32 = 0xf0;
/// Input register operands reading `.xyzw` and `.xyww`.
const OPERAND_INPUT_SWIZZLE_XYZW: u32 = 0x0010_1e46;
const OPERAND_INPUT_SWIZZLE_XYWW: u32 = 0x0010_1f46;
/// A temp register operand writing `.xyz`.
const OPERAND_TEMP_MASK_XYZ: u32 = 0x0010_0072;
/// Temp register operands selecting `.y` and reading `.xxxx`, `.zzzz`, `.xxxy`, and `.zzzw`.
const OPERAND_TEMP_SELECT_Y: u32 = 0x0010_001A;
const OPERAND_TEMP_SWIZZLE_XXXX: u32 = 0x0010_0006;
const OPERAND_TEMP_SWIZZLE_ZZZZ: u32 = 0x0010_0aa6;
const OPERAND_TEMP_SWIZZLE_XXXY: u32 = 0x0010_0406;
const OPERAND_TEMP_SWIZZLE_ZZZW: u32 = 0x0010_0ea6;
/// Constant-buffer operands with two immediate indices selecting `.y` and reading `.wwww`.
const OPERAND_CB_2D_SELECT_Y: u32 = 0x0020_801A;
const OPERAND_CB_2D_SWIZZLE_WWWW: u32 = 0x0020_8ff6;
/// The fixed size of a signature element record.
const SIGNATURE_ELEMENT_LEN: usize = 24;
