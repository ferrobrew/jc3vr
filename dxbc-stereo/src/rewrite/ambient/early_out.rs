//! Skip the ambient-occlusion generation's taps for quads that lie wholly beyond its range.
//!
//! The generation pass tests the pixel's depth against its range constant near the top of the shader
//! but acts on the test only at the very end, after all of its depth taps, where a pixel beyond the
//! range writes a constant built from its centre depth and returns:
//!
//! ```text
//! ge r0.w, r0.z, cb1[3].z                // beyond the range
//! ...                                    // every tap, then the occlusion and its derivatives
//! if_nz r0.w
//!   mov o0.xw, l(1.000000,0,0,1.000000)
//!   mov o0.yz, r2.yyzy                   // the packed centre depth
//!   ret
//! endif
//! ```
//!
//! [`skip_far_ambient_occlusion`] copies that branch to just after the last write of anything it
//! reads, behind a test that is uniform over the 2x2 quad: the branch is taken only when every pixel
//! of the quad is beyond the range. A pixel's output is the same either way, since a far pixel's
//! output never depended on the taps, and the derivatives the shader takes later are quad-local, so a
//! quad that keeps running reads the same neighbours as before. Quads with any pixel in range run
//! exactly the original instructions.
//!
//! The quad-uniform test is built from fine derivatives of the per-pixel test as `0.0` or `1.0`:
//! `f * (1 - |ddx_fine(f)|)` is `1.0` exactly when both pixels of the pixel's row are far, and is the
//! same for both, and the same step down the columns makes it `1.0` exactly when all four are. No
//! pixel parity is involved, so the rewrite composes with rewrites that make the pixel position
//! eye-local.

use crate::{
    container::{Dxbc, DxbcError},
    rewrite::{
        ambient::{
            CONSTANTS, DEPTH_TEXTURE, NORMAL_TEXTURE, chunk,
            operands::{
                OPCODE_ADD, OPCODE_MOV, OPCODE_MUL, OPERAND_TYPE_CONSTANT_BUFFER,
                OPERAND_TYPE_INPUT, OPERAND_TYPE_TEMP, SELECTION_MASK, SELECTION_SELECT_1,
                SELECTION_SWIZZLE, X, Y, dst, imm, instruction, src, src_mod,
            },
        },
        common::{OPCODE_DCL_TEMPS, OPCODE_RET, reassemble},
        planar::{BINDING_TEXTURE, binding, constant_location},
    },
    tokens::{ShaderStage, TokenStream, parse_operand},
};

/// Rewrite the ambient-occlusion generation pixel shader (the original, or the output of
/// [`snap_ambient_occlusion_uv`](crate::snap_ambient_occlusion_uv) or
/// [`localize_ambient_occlusion`](crate::localize_ambient_occlusion)) so a quad whose four pixels are
/// all beyond the occlusion range writes its constant output without running the taps. The output
/// is bit-identical to the input shader's. Returns [`DxbcError::NoAmbientOcclusion`] for any other
/// shader, or for one whose far branch is not in the shape described in the module documentation.
pub fn skip_far_ambient_occlusion(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let rdef = chunk(&dxbc, blob, b"RDEF").ok_or(DxbcError::NoAmbientOcclusion)?;
    if binding(rdef, DEPTH_TEXTURE, BINDING_TEXTURE).is_none()
        || binding(rdef, NORMAL_TEXTURE, BINDING_TEXTURE).is_none()
        || constant_location(rdef, CONSTANTS).is_none()
    {
        return Err(DxbcError::NoAmbientOcclusion);
    }
    let stream =
        TokenStream::new(chunk(&dxbc, blob, b"SHEX").ok_or(DxbcError::UnsupportedShaderModel)?)?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoAmbientOcclusion);
    }
    let tokens = stream.tokens();
    let mut instructions = Vec::new();
    for insn in stream.instructions() {
        let insn = insn?;
        instructions.push(Span {
            opcode: insn.opcode,
            start: insn.start,
            end: insn.end,
            operands_start: insn.operands_start(),
            declaration: insn.is_declaration(),
        });
    }

    let temps = instructions
        .iter()
        .find(|span| span.opcode == OPCODE_DCL_TEMPS)
        .and_then(|span| tokens.get(span.start + 1).copied())
        .ok_or(DxbcError::NoAmbientOcclusion)?;
    let first_executable = instructions
        .iter()
        .position(|span| !span.declaration)
        .ok_or(DxbcError::NoAmbientOcclusion)?;

    let depths = nesting_depths(&instructions);
    let branch = far_branch(tokens, &instructions, &depths, first_executable)?;

    // Every temporary component the branch reads: its condition and its moves' sources.
    let mut needed = vec![branch.condition];
    for span in &instructions[branch.index + 1..branch.ret] {
        let mut operands = operand_spans(tokens, span)?.into_iter();
        let destination = operands.next().ok_or(DxbcError::NoAmbientOcclusion)?;
        let lanes = mask_of(tokens[destination.0]);
        for operand in operands {
            needed.extend(source_reads(tokens, operand, lanes)?);
        }
    }

    // Insert after the last write to any of them, at the top nesting level.
    let mut insert = first_executable;
    for (index, span) in instructions
        .iter()
        .enumerate()
        .take(branch.index)
        .skip(first_executable)
    {
        for (start, end) in operand_spans(tokens, span)? {
            if let Some((register, mask)) = temp_write(&tokens[start..end])
                && needed
                    .iter()
                    .any(|&(r, c)| r == register && mask & (1 << c) != 0)
            {
                insert = index + 1;
            }
        }
    }
    while insert < branch.index && depths[insert] != 0 {
        insert += 1;
    }

    // What the early-out skips must have no effect of its own beyond the temporaries, and must
    // contain the taps, or there is nothing to gain.
    let skipped = &instructions[insert..branch.index];
    let mut samples = 0;
    for span in skipped {
        if matches!(span.opcode, OPCODE_RET | OPCODE_RETC | OPCODE_DISCARD)
            || (OPCODE_FIRST_STORE..=OPCODE_LAST_ATOMIC).contains(&span.opcode)
        {
            return Err(DxbcError::NoAmbientOcclusion);
        }
        for (start, _) in operand_spans(tokens, span)? {
            if (tokens[start] >> 12) & 0xff == OPERAND_TYPE_OUTPUT {
                return Err(DxbcError::NoAmbientOcclusion);
            }
        }
        samples += usize::from(SAMPLE_OPCODES.contains(&span.opcode));
    }
    if samples == 0 {
        return Err(DxbcError::NoAmbientOcclusion);
    }

    let mut out = tokens[..2].to_vec();
    for (index, span) in instructions.iter().enumerate() {
        if index == insert {
            emit_early_out(
                &mut out,
                temps,
                &tokens[instructions[branch.index].operands_start..instructions[branch.index].end],
                &tokens[instructions[branch.index + 1].start..instructions[branch.ret].end],
            )?;
        }
        if span.opcode == OPCODE_DCL_TEMPS {
            out.extend([tokens[span.start], temps + 1]);
        } else {
            out.extend_from_slice(&tokens[span.start..span.end]);
        }
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;

    let shex: Vec<u8> = out.iter().flat_map(|token| token.to_le_bytes()).collect();
    let isgn = chunk(&dxbc, blob, b"ISGN")
        .ok_or(DxbcError::MissingInputSignature)?
        .to_vec();
    let osgn = chunk(&dxbc, blob, b"OSGN")
        .ok_or(DxbcError::NoAmbientOcclusion)?
        .to_vec();
    reassemble(&dxbc, blob, isgn, osgn, shex, false)
}

/// One instruction's place in the token stream.
struct Span {
    opcode: u32,
    start: usize,
    end: usize,
    operands_start: usize,
    declaration: bool,
}

/// The far branch: the `if_nz` at `index`, testing the temporary component `condition`, whose body
/// is moves into outputs up to the `ret` at `ret`, closed by the `endif` after it.
struct FarBranch {
    index: usize,
    ret: usize,
    condition: (u32, u32),
}

/// Each instruction's control-flow nesting depth: how many `if`, `loop`, and `switch` blocks enclose
/// it.
fn nesting_depths(instructions: &[Span]) -> Vec<u32> {
    let mut depth = 0u32;
    instructions
        .iter()
        .map(|span| {
            if matches!(
                span.opcode,
                OPCODE_ENDIF | OPCODE_ENDLOOP | OPCODE_ENDSWITCH
            ) {
                depth = depth.saturating_sub(1);
            }
            let here = depth;
            if matches!(span.opcode, OPCODE_IF | OPCODE_LOOP | OPCODE_SWITCH) {
                depth += 1;
            }
            here
        })
        .collect()
}

/// The last top-level `if_nz` on a temporary component whose body is only moves into outputs and a
/// `ret`, closed right after the `ret`.
fn far_branch(
    tokens: &[u32],
    instructions: &[Span],
    depths: &[u32],
    first_executable: usize,
) -> Result<FarBranch, DxbcError> {
    (first_executable..instructions.len())
        .rev()
        .find_map(|index| {
            let span = &instructions[index];
            if span.opcode != OPCODE_IF
                || depths[index] != 0
                || tokens[span.start] & TEST_NONZERO == 0
            {
                return None;
            }
            let operand = &tokens[span.operands_start..span.end];
            let condition = match operand {
                [token, register]
                    if (token >> 12) & 0xff == OPERAND_TYPE_TEMP
                        && (token >> 2) & 0x3 == SELECTION_SELECT_1
                        && token >> 31 == 0 =>
                {
                    (*register, (token >> 4) & 0x3)
                }
                _ => return None,
            };
            let ret = (index + 1..instructions.len())
                .find(|&next| instructions[next].opcode != OPCODE_MOV)?;
            let moves_into_outputs = instructions[index + 1..ret]
                .iter()
                .all(|span| (tokens[span.operands_start] >> 12) & 0xff == OPERAND_TYPE_OUTPUT);
            (ret > index + 1
                && moves_into_outputs
                && instructions[ret].opcode == OPCODE_RET
                && instructions.get(ret + 1).map(|span| span.opcode) == Some(OPCODE_ENDIF))
            .then_some(FarBranch {
                index,
                ret,
                condition,
            })
        })
        .ok_or(DxbcError::NoAmbientOcclusion)
}

/// The token ranges of an instruction's operands.
fn operand_spans(tokens: &[u32], span: &Span) -> Result<Vec<(usize, usize)>, DxbcError> {
    let mut spans = Vec::new();
    let mut position = span.operands_start;
    while position < span.end {
        let (_, next) = parse_operand(tokens, position)?;
        spans.push((position, next));
        position = next;
    }
    Ok(spans)
}

/// The write mask of a destination operand token.
fn mask_of(token: u32) -> u32 {
    (token >> 4) & 0xf
}

/// The temporary components a source operand reads in the destination's `lanes`, or an error for an
/// operand this rewrite cannot account for. Immediates, inputs, and constant-buffer rows read no
/// temporary.
fn source_reads(
    tokens: &[u32],
    (start, end): (usize, usize),
    lanes: u32,
) -> Result<Vec<(u32, u32)>, DxbcError> {
    let token = tokens[start];
    let operand_type = (token >> 12) & 0xff;
    if operand_type != OPERAND_TYPE_TEMP {
        return match operand_type {
            OPERAND_TYPE_IMMEDIATE32 | OPERAND_TYPE_INPUT | OPERAND_TYPE_CONSTANT_BUFFER => {
                Ok(Vec::new())
            }
            _ => Err(DxbcError::NoAmbientOcclusion),
        };
    }
    let register = *tokens[start..end]
        .last()
        .ok_or(DxbcError::NoAmbientOcclusion)?;
    let components: Vec<u32> = match (token >> 2) & 0x3 {
        SELECTION_SWIZZLE => (0..4)
            .filter(|lane| lanes & (1 << lane) != 0)
            .map(|lane| (token >> (4 + 2 * lane)) & 0x3)
            .collect(),
        SELECTION_SELECT_1 => vec![(token >> 4) & 0x3],
        SELECTION_MASK => (0..4).filter(|c| mask_of(token) & (1 << c) != 0).collect(),
        _ => return Err(DxbcError::NoAmbientOcclusion),
    };
    Ok(components.into_iter().map(|c| (register, c)).collect())
}

/// The register and mask of an operand that writes a temporary. Destinations always select their
/// components by mask, so every temporary in mask mode is counted; a source in that mode would only
/// move the insertion later, which is still exact.
fn temp_write(operand: &[u32]) -> Option<(u32, u32)> {
    let token = *operand.first()?;
    ((token >> 12) & 0xff == OPERAND_TYPE_TEMP && (token >> 2) & 0x3 == SELECTION_MASK)
        .then(|| operand.last().map(|register| (*register, mask_of(token))))
        .flatten()
}

/// Emits the quad-uniform early-out into `out`: the per-pixel test as a float, reduced over the
/// quad's rows and then its columns through fine derivatives into `rT.x`, and the far branch's body
/// behind it. `temp` is the added temporary `rT`, `condition` the far branch's condition operand, and
/// `body` its moves and `ret`.
fn emit_early_out(
    out: &mut Vec<u32>,
    temp: u32,
    condition: &[u32],
    body: &[u32],
) -> Result<(), DxbcError> {
    let one = 1.0f32.to_bits();
    let zero = 0.0f32.to_bits();
    let mut emit = |opcode: u32, operands: &[&[u32]]| -> Result<(), DxbcError> {
        let length = 1 + operands.iter().map(|operand| operand.len()).sum::<usize>();
        out.push(instruction(opcode, length)?);
        for operand in operands {
            out.extend_from_slice(operand);
        }
        Ok(())
    };
    // movc rT.x, <condition>, l(1.0), l(0.0)
    emit(
        OPCODE_MOVC,
        &[&dst(temp, X), condition, &imm(one), &imm(zero)],
    )?;
    for derivative in [OPCODE_DERIV_RTX_FINE, OPCODE_DERIV_RTY_FINE] {
        // deriv rT.y, rT.x; add rT.y, -|rT.y|, l(1.0); mul rT.x, rT.x, rT.y
        emit(derivative, &[&dst(temp, Y), &src(temp, X)])?;
        emit(
            OPCODE_ADD,
            &[
                &dst(temp, Y),
                &src_mod(temp, Y, MODIFIER_ABS_NEG),
                &imm(one),
            ],
        )?;
        emit(OPCODE_MUL, &[&dst(temp, X), &src(temp, X), &src(temp, Y)])?;
    }
    // if_nz rT.x
    out.push(instruction(OPCODE_IF, 3)? | TEST_NONZERO);
    out.extend(src(temp, X));
    out.extend_from_slice(body);
    out.push(instruction(OPCODE_ENDIF, 1)?);
    Ok(())
}

const OPCODE_DISCARD: u32 = 0x0d;
const OPCODE_ENDIF: u32 = 0x15;
const OPCODE_ENDLOOP: u32 = 0x16;
const OPCODE_ENDSWITCH: u32 = 0x17;
const OPCODE_IF: u32 = 0x1f;
const OPCODE_RETC: u32 = 0x25;
const OPCODE_LOOP: u32 = 0x30;
const OPCODE_MOVC: u32 = 0x37;
const OPCODE_SWITCH: u32 = 0x4c;
const OPCODE_DERIV_RTX_FINE: u32 = 0x7b;
const OPCODE_DERIV_RTY_FINE: u32 = 0x7d;
/// The SM5 UAV stores and atomics, `store_uav_typed` through `imm_atomic_umin`.
const OPCODE_FIRST_STORE: u32 = 0xa4;
const OPCODE_LAST_ATOMIC: u32 = 0xba;
/// The sampling instructions whose presence makes the early-out worth taking: `sample`, `sample_c`,
/// `sample_c_lz`, `sample_l`, `sample_d`, `sample_b`, and `ld`.
const SAMPLE_OPCODES: [u32; 7] = [0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x2d];

/// The test-nonzero bit of a conditional instruction's opcode token.
const TEST_NONZERO: u32 = 1 << 18;
const OPERAND_TYPE_OUTPUT: u32 = 2;
const OPERAND_TYPE_IMMEDIATE32: u32 = 4;
/// The extended operand token for a source taken as its negated absolute value.
const MODIFIER_ABS_NEG: u32 = 0xc1;
