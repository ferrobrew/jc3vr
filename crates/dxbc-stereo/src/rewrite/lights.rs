//! The local-light half of the eye-camera rewrite: light positions measured from each eye.
//!
//! The clustered-lighting frame constants (`LightingFrameConsts`) hold every point and spot light's
//! position relative to the camera position the engine built them from, and every projected spot
//! light's projection as a matrix that maps a camera-relative position. A pixel shader lights a
//! surface by subtracting its own camera-relative position from the light's, and applies the
//! projection to that same position:
//!
//! ```text
//! add r6.xyz, v5.xyzx, -cb0[4].xyzx                  // the pixel, relative to the camera
//! add r17.xyz, -r6.xyzx, cb3[r8.w + 0].xyzx          // the light vector
//! mad r21.xyw, cb3[r17.x + 1792].xyxw, r6.xxxx, cb3[r17.x + 1795].xyxw   // the projection
//! ```
//!
//! Once the eye-camera rewrite moves the camera row to the pixel's eye, the pixel position is
//! relative to that eye while the light data stays relative to the camera it was built from, and
//! every light vector is off by the eye's offset from that camera. [`plan`] finds the reads to
//! correct, and [`emit`] corrects each one in place: a light position read becomes a read of the
//! position minus the eye's offset ([`EYE_CAMERA_LIGHT_OFFSET_ROW`] of the eye's block), and a
//! projection's translation row becomes the row plus the projection's linear part applied to the
//! offset, which is the projection of the position relative to the camera the matrix was built for.
//!
//! A read is corrected only when the position it is measured against is relative to the pixel's eye:
//! a temp whose components the shader last wrote by subtracting a camera row of `cb0` (which the
//! camera rewrite moves to the eye), or the deferred resolve's depth times its interpolated view ray,
//! which the per-eye resolve builds from the eye's own basis. A position the vertex shader made
//! relative to its camera arrives as an interpolant and is left alone, since the light data and that
//! position then share the camera they were built from.

use std::collections::HashMap;

use crate::{
    container::DxbcError,
    isa::{
        IDX_IMM32_PLUS_RELATIVE, IDX_RELATIVE, MODIFIER_NEG, OPCODE_ADD, OPCODE_ENDLOOP,
        OPCODE_LOOP, OPCODE_MAD, OPCODE_MUL, OPERAND_EXTENDED, OPERAND_TEMP_MASK_XYZW,
        OPERAND_TEMP_SELECT_X, OPERAND_TYPE_CONSTANT_BUFFER, OPERAND_TYPE_INPUT, OPERAND_TYPE_TEMP,
        SELECTION_MASK, SELECTION_SELECT_1, SELECTION_SWIZZLE,
    },
    rewrite::{
        camera::{EYE_CAMERA_CB_REGISTER, EYE_CAMERA_LIGHT_OFFSET_ROW, EYE_CAMERA_SOURCE_ROWS},
        planar::constant_location,
    },
    tokens::{Instruction, OperandKind, TokenStream, parse_operand},
};

/// Where a shader's resource table places the light arrays the correction reads.
#[derive(Clone, Copy)]
pub(super) struct LightLayout {
    slot: u32,
    point_lights: u32,
    spot_lights: u32,
    projections: u32,
}

/// The light arrays of a shader that binds the clustered-lighting frame constants, or `None` for a
/// shader that does not bind all three in one buffer.
pub(super) fn layout(rdef: &[u8]) -> Option<LightLayout> {
    let [point, spot, projections] =
        [POINT_LIGHTS, SPOT_LIGHTS, PROJECTIONS].map(|name| constant_location(rdef, name));
    let (point, spot, projections) = (point?, spot?, projections?);
    let aligned = [&point, &spot, &projections]
        .iter()
        .all(|location| location.slot == point.slot && location.component == 0);
    aligned.then_some(LightLayout {
        slot: point.slot,
        point_lights: point.register,
        spot_lights: spot.register,
        projections: projections.register,
    })
}

/// One light read to correct: the instruction it is in and the operand that reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Correction {
    /// The token index of the instruction.
    pub(super) at: usize,
    /// The operand's place among the instruction's operands, the destination being 0.
    operand: usize,
    kind: CorrectionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CorrectionKind {
    /// The operand reads a point or spot light's camera-relative position.
    Position,
    /// The operand reads a projection's translation row, added to the projection of a
    /// camera-relative position.
    ProjectionTranslation,
}

/// Every light read in the shader that is measured against a position relative to the pixel's eye.
pub(super) fn plan(
    stream: &TokenStream,
    layout: LightLayout,
) -> Result<Vec<Correction>, DxbcError> {
    let tokens = stream.tokens();
    let mut instructions = Vec::new();
    for insn in stream.instructions() {
        let insn = insn?;
        if !insn.is_declaration() {
            instructions.push(insn);
        }
    }
    let loops = loop_bodies(&instructions);
    let mut writes = Vec::new();
    for insn in &instructions {
        for (register, mask) in written_temps(tokens, insn)? {
            writes.push((insn.start, register, mask));
        }
    }
    let mut origins: HashMap<u32, [Option<Origin>; 4]> = HashMap::new();
    let mut corrections = Vec::new();
    for insn in &instructions {
        if let Some(candidate) = candidate(tokens, insn, layout)? {
            let measured_from_eye = candidate.against.iter().all(|&(register, components)| {
                let known = origins.get(&register).is_some_and(|slots| {
                    (0..4)
                        .filter(|c| components & (1 << c) != 0)
                        .all(|c| slots[c].is_some_and(|origin| candidate.accepts.contains(&origin)))
                });
                known && !rewritten_later_in_loop(&loops, &writes, insn.start, register, components)
            });
            if measured_from_eye {
                corrections.push(candidate.correction);
            }
        }
        let origin = origin(tokens, insn)?;
        for (register, mask) in written_temps(tokens, insn)? {
            let slots = origins.entry(register).or_insert([None; 4]);
            for (c, slot) in slots.iter_mut().enumerate() {
                if mask & (1 << c) != 0 {
                    *slot = origin;
                }
            }
        }
    }
    Ok(corrections)
}

/// Emit the instructions that compute the corrected value into `scratch`, then the instruction's
/// tokens with the corrected operand replaced by `scratch` (same swizzle), for the caller to finish
/// re-serializing. `eye` holds the eye's first row of the eye-camera buffer.
pub(super) fn emit(
    tokens: &[u32],
    insn: &Instruction<'_>,
    correction: &Correction,
    eye: u32,
    scratch: u32,
    out: &mut Vec<u32>,
) -> Result<Vec<u32>, DxbcError> {
    let (start, end) = operand_span(tokens, insn, correction.operand)?;
    let source = &tokens[start..end];
    if source[0] & OPERAND_EXTENDED != 0 {
        return Err(DxbcError::UnsupportedOperandEncoding);
    }
    let full = |base_delta: i64| -> Result<Vec<u32>, DxbcError> {
        let mut operand = source.to_vec();
        operand[0] = (operand[0] & !SELECTION_BITS) | SWIZZLE_XYZW;
        if (operand[0] >> 25) & 7 == IDX_RELATIVE {
            // A relative index with no base: give it a zero base to offset.
            operand[0] = (operand[0] & !(7 << 25)) | (IDX_IMM32_PLUS_RELATIVE << 25);
            operand.insert(BASE_INDEX, 0);
        }
        let base = operand
            .get_mut(BASE_INDEX)
            .ok_or(DxbcError::UnexpectedEndOfTokens)?;
        *base = u32::try_from(i64::from(*base) + base_delta)
            .map_err(|_| DxbcError::UnsupportedOperandEncoding)?;
        Ok(operand)
    };
    let offset = |selection: u32, negate: bool| -> Vec<u32> {
        let mut operand = vec![OPERAND_CB_DYNAMIC | selection];
        if negate {
            operand[0] |= OPERAND_EXTENDED;
            operand.push(MODIFIER_NEG);
        }
        operand.extend([
            EYE_CAMERA_CB_REGISTER,
            EYE_CAMERA_LIGHT_OFFSET_ROW,
            OPERAND_TEMP_SELECT_X,
            eye,
        ]);
        operand
    };
    let scratch_destination = [OPERAND_TEMP_MASK_XYZW, scratch];
    let scratch_source = [OPERAND_TEMP_1D | SWIZZLE_XYZW, scratch];
    match correction.kind {
        CorrectionKind::Position => {
            let operands = [
                scratch_destination.to_vec(),
                full(0)?,
                offset(SWIZZLE_XYZW, true),
            ];
            push_instruction(out, OPCODE_ADD, &operands)?;
        }
        CorrectionKind::ProjectionTranslation => {
            for (row, selection) in [SWIZZLE_XXXX, SWIZZLE_YYYY, SWIZZLE_ZZZZ]
                .into_iter()
                .enumerate()
            {
                let addend = if row == 0 {
                    full(0)?
                } else {
                    scratch_source.to_vec()
                };
                let operands = [
                    scratch_destination.to_vec(),
                    full(row as i64 - TRANSLATION_ROW)?,
                    offset(selection, false),
                    addend,
                ];
                push_instruction(out, OPCODE_MAD, &operands)?;
            }
        }
    }
    let mut patched = tokens[insn.start..start].to_vec();
    patched.push(OPERAND_TEMP_1D | (source[0] & COMPONENT_AND_SELECTION_BITS));
    patched.push(scratch);
    patched.extend_from_slice(&tokens[end..insn.end]);
    Ok(patched)
}

const POINT_LIGHTS: &[u8] = b"PointLights";
const SPOT_LIGHTS: &[u8] = b"SpotLights";
const PROJECTIONS: &[u8] = b"Projections";

/// A projection's translation row, counted from its first row.
const TRANSLATION_ROW: i64 = 3;

/// An extended operand token's type field for a modifier, and its modifier field's negate and
/// absolute-negate values.
const EXTENDED_TYPE_MODIFIER: u32 = 1;
const MODIFIER_NEGATE: u32 = 1;
const MODIFIER_ABSOLUTE_NEGATE: u32 = 3;

/// The component-count, selection-mode, and swizzle bits of an operand token.
const COMPONENT_AND_SELECTION_BITS: u32 = 0xFFF;
/// The selection-mode and swizzle bits of an operand token.
const SELECTION_BITS: u32 = 0xFFC;
const SWIZZLE_XYZW: u32 = (SELECTION_SWIZZLE << 2) | (0xE4 << 4);
const SWIZZLE_XXXX: u32 = SELECTION_SWIZZLE << 2;
const SWIZZLE_YYYY: u32 = (SELECTION_SWIZZLE << 2) | (0x55 << 4);
const SWIZZLE_ZZZZ: u32 = (SELECTION_SWIZZLE << 2) | (0xAA << 4);
/// A four-component temp with a one-dimensional immediate index; the selection bits are added.
const OPERAND_TEMP_1D: u32 = 2 | (OPERAND_TYPE_TEMP << 12) | (1 << 20);
/// A four-component constant-buffer operand indexed by an immediate slot and an immediate plus a
/// relative element; the selection bits are added.
const OPERAND_CB_DYNAMIC: u32 = 2 | (OPERAND_TYPE_CONSTANT_BUFFER << 12) | (2 << 20) | (3 << 25);
/// The token index, within a non-extended dynamically indexed constant-buffer operand, of its
/// element base.
const BASE_INDEX: usize = 2;

/// How a temp component came by its value, as far as the correction cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// A position minus a camera row of `cb0`.
    CameraRelative,
    /// A product with an interpolated input, the deferred resolve's depth times its view ray.
    ViewRay,
}

/// A light read, and the positions it is measured against.
struct Candidate {
    correction: Correction,
    /// Each temp the read is measured against, with the components read from it.
    against: Vec<(u32, u32)>,
    /// The origins those components may have for the read to be corrected.
    accepts: &'static [Origin],
}

/// A decoded source or destination operand, as far as the analysis needs it.
#[derive(Clone, Copy)]
struct Decoded {
    kind: OperandKind,
    operand_type: u32,
    /// The first index, for a temp or input register.
    register: Option<u32>,
    /// The element base of a dynamically indexed constant-buffer read.
    base: Option<u32>,
    negated: bool,
    selection: u32,
    /// The mask, swizzle, or selected component bits.
    components: u32,
}

impl Decoded {
    /// The components this source reads, for an instruction writing `mask`.
    fn components_read(&self, mask: u32) -> u32 {
        match self.selection {
            SELECTION_SWIZZLE => (0..4)
                .filter(|c| mask & (1 << c) != 0)
                .fold(0, |read, c| read | 1 << ((self.components >> (2 * c)) & 3)),
            SELECTION_SELECT_1 => 1 << (self.components & 3),
            _ => self.components & 0xF,
        }
    }

    fn temp(&self) -> Option<u32> {
        (self.operand_type == OPERAND_TYPE_TEMP)
            .then_some(self.register)
            .flatten()
    }
}

fn decode(tokens: &[u32], insn: &Instruction<'_>) -> Result<Vec<Decoded>, DxbcError> {
    let mut decoded = Vec::new();
    for operand in insn.operands() {
        let operand = operand?;
        let at = operand.token_offset;
        let token = tokens[at];
        let extended = usize::from(token & OPERAND_EXTENDED != 0);
        let negated = extended == 1
            && tokens[at + 1] & 0x3F == EXTENDED_TYPE_MODIFIER
            && matches!(
                (tokens[at + 1] >> 6) & 0xFF,
                MODIFIER_NEGATE | MODIFIER_ABSOLUTE_NEGATE
            );
        let index_dimension = (token >> 20) & 3;
        let first_immediate = (token >> 22) & 7 == 0;
        let register = (index_dimension >= 1 && first_immediate)
            .then(|| tokens.get(at + 1 + extended).copied())
            .flatten();
        let base = match (operand.kind, (token >> 25) & 7) {
            (OperandKind::ConstantBufferDynamic { .. }, IDX_IMM32_PLUS_RELATIVE) => {
                tokens.get(at + 2 + extended).copied()
            }
            (OperandKind::ConstantBufferDynamic { .. }, IDX_RELATIVE) => Some(0),
            _ => None,
        };
        decoded.push(Decoded {
            kind: operand.kind,
            operand_type: (token >> 12) & 0xFF,
            register,
            base,
            negated,
            selection: (token >> 2) & 3,
            components: (token >> 4) & 0xFF,
        });
    }
    Ok(decoded)
}

/// The temps an instruction writes, with their masks. Every masked temp operand counts, which over-
/// approximates the writes of the rare instructions with a masked source; an extra write can only
/// keep a read from being corrected.
fn written_temps(tokens: &[u32], insn: &Instruction<'_>) -> Result<Vec<(u32, u32)>, DxbcError> {
    Ok(decode(tokens, insn)?
        .iter()
        .filter(|operand| operand.selection == SELECTION_MASK)
        .filter_map(|operand| Some((operand.temp()?, operand.components & 0xF)))
        .collect())
}

/// The origin an instruction gives the temp it writes.
fn origin(tokens: &[u32], insn: &Instruction<'_>) -> Result<Option<Origin>, DxbcError> {
    let operands = decode(tokens, insn)?;
    let sources = operands.get(1..).unwrap_or_default();
    Ok(match insn.opcode {
        OPCODE_ADD
            if sources.len() == 2
                && sources.iter().any(|source| {
                    source.negated
                        && matches!(
                            source.kind,
                            OperandKind::ConstantBuffer { register: 0, element }
                                if EYE_CAMERA_SOURCE_ROWS[..2].contains(&element)
                        )
                }) =>
        {
            Some(Origin::CameraRelative)
        }
        OPCODE_MUL
            if sources
                .iter()
                .any(|source| source.operand_type == OPERAND_TYPE_INPUT) =>
        {
            Some(Origin::ViewRay)
        }
        _ => None,
    })
}

/// A light read in `insn`, if it is one: an `add` or `mad` whose addend is a dynamically indexed read
/// of a light position or a projection's translation row, measured against a negated position (for a
/// position) or a multiplied one (for a projection).
fn candidate(
    tokens: &[u32],
    insn: &Instruction<'_>,
    layout: LightLayout,
) -> Result<Option<Candidate>, DxbcError> {
    let operands = decode(tokens, insn)?;
    let Some(destination) = operands.first() else {
        return Ok(None);
    };
    let mask = destination.components & 0xF;
    let light_base = |operand: &Decoded| match (operand.kind, operand.base) {
        (OperandKind::ConstantBufferDynamic { register }, Some(base))
            if register == layout.slot =>
        {
            Some(base)
        }
        _ => None,
    };
    let is_position = |base: u32| base == layout.point_lights || base == layout.spot_lights;
    let translation = layout.projections + TRANSLATION_ROW as u32;
    let candidate = match (insn.opcode, &operands[..]) {
        (OPCODE_ADD, [_, a, b]) => {
            [(1, a, b), (2, b, a)]
                .into_iter()
                .find_map(|(operand, light, other)| {
                    let base = light_base(light)?;
                    let register = other.temp()?;
                    (is_position(base) && other.negated).then(|| Candidate {
                        correction: Correction {
                            at: insn.start,
                            operand,
                            kind: CorrectionKind::Position,
                        },
                        against: vec![(register, other.components_read(mask))],
                        accepts: &[Origin::CameraRelative],
                    })
                })
        }
        (OPCODE_MAD, [_, a, b, addend]) => match light_base(addend) {
            Some(base) if is_position(base) => {
                let (depth, ray) = if a.negated { (a, b) } else { (b, a) };
                // The resolve's form: the light position minus its depth times the interpolated
                // view ray, which the per-eye resolve builds from the eye's own basis.
                (depth.negated && depth.temp().is_some() && ray.operand_type == OPERAND_TYPE_INPUT)
                    .then(|| Candidate {
                        correction: Correction {
                            at: insn.start,
                            operand: 3,
                            kind: CorrectionKind::Position,
                        },
                        against: Vec::new(),
                        accepts: &[],
                    })
            }
            Some(base) if base == translation => {
                let position = if light_base(a).is_some() { b } else { a };
                position.temp().map(|register| Candidate {
                    correction: Correction {
                        at: insn.start,
                        operand: 3,
                        kind: CorrectionKind::ProjectionTranslation,
                    },
                    against: vec![(register, position.components_read(mask))],
                    accepts: &[Origin::CameraRelative, Origin::ViewRay],
                })
            }
            _ => None,
        },
        _ => None,
    };
    Ok(candidate)
}

/// The token span `[start, end)` of an instruction's `index`th operand.
fn operand_span(
    tokens: &[u32],
    insn: &Instruction<'_>,
    index: usize,
) -> Result<(usize, usize), DxbcError> {
    let mut pos = insn.operands_start();
    for _ in 0..index {
        pos = parse_operand(tokens, pos)?.1;
    }
    Ok((pos, parse_operand(tokens, pos)?.1))
}

/// The token span of every outermost loop body, from its `loop` to its `endloop`.
fn loop_bodies(instructions: &[Instruction<'_>]) -> Vec<(usize, usize)> {
    let mut bodies = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for insn in instructions {
        match insn.opcode {
            OPCODE_LOOP => {
                if depth == 0 {
                    start = insn.start;
                }
                depth += 1;
            }
            OPCODE_ENDLOOP if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    bodies.push((start, insn.end));
                }
            }
            _ => {}
        }
    }
    bodies
}

/// Whether a read at `at` inside a loop could see a value written later in the same loop, which a
/// later iteration would read in place of the one the linear scan found.
fn rewritten_later_in_loop(
    loops: &[(usize, usize)],
    writes: &[(usize, u32, u32)],
    at: usize,
    register: u32,
    components: u32,
) -> bool {
    let Some(&(_, end)) = loops.iter().find(|&&(start, end)| start <= at && at < end) else {
        return false;
    };
    writes.iter().any(|&(write, written, mask)| {
        write > at && write < end && written == register && mask & components != 0
    })
}

/// Append one instruction made of an opcode and its operands.
fn push_instruction(
    out: &mut Vec<u32>,
    opcode: u32,
    operands: &[Vec<u32>],
) -> Result<(), DxbcError> {
    let length = 1 + operands.iter().map(Vec::len).sum::<usize>();
    let length = u32::try_from(length)
        .ok()
        .filter(|length| *length <= 0x7F)
        .ok_or(DxbcError::InstructionTooLong)?;
    out.push(opcode | (length << 24));
    for operand in operands {
        out.extend_from_slice(operand);
    }
    Ok(())
}
