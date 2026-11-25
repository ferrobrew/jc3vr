//! The eye-pinned `M_eye` reprojection for single-pass stereo.
//!
//! Some pipelines cannot carry an eye index to the stage that writes `SV_Position`: a vertex shader
//! that already consumes `SV_InstanceID` for its own purposes, or a domain shader whose vertex shader
//! has no free interpolant lane to ride the eye through the hull stage. Their draws are instead
//! submitted once per eye, and for each submission the payload pins both `cb13` eye slots to that
//! eye. A shader rewritten here reads eye 0's `M_eye` block unconditionally, which under that pin is
//! whichever eye is being drawn, and post-multiplies its own clip position by it before each `ret`.
//! No interface changes: no `SV_InstanceID` input and no `SV_ViewportArrayIndex` output, because the
//! re-issue binds the eye's viewport itself.

use crate::{
    container::{Dxbc, DxbcError},
    rewrite::{
        common::{
            CB_ACCESS_DYNAMIC_INDEXED, OPCODE_DCL_CONSTANT_BUFFER, OPCODE_DCL_OUTPUT_SIV,
            OPCODE_DCL_TEMPS, OPCODE_MOV, OPCODE_RET, OPCODE_RETC, OPERAND_CB_2D_IMM,
            OPERAND_IMM32_SCALAR, OPERAND_TEMP_MASK_X, SB_NAME_POSITION, STEREO_CB_REGISTER,
            STEREO_REPROJ_CB_ROWS, STEREO_SELECT_CB_ROWS, declared_register, emit_meye_epilogue,
            reassemble, rewrite_reproject_instruction,
        },
        domain_rows::{
            EYE_ROW_TEMPS, RowBase, emit_select_rows, reads_terrain_view_projection,
            rewrite_rows_instruction,
        },
    },
    tokens::{ShaderStage, TokenStream},
};

/// Rewrites a vertex or domain shader to post-multiply its own `SV_Position` by the `M_eye` block of
/// `cb13`'s first eye slot. Every write to the position output is renamed to a temp `rClip`, and
/// `o<pos> = M_eye · rClip` is emitted before each `ret`, exactly as [`reproject_vertex_shader`] does,
/// but with the row base held at zero rather than derived from an instance parity.
///
/// The shader renders the correct eye only while `cb13` is pinned to that eye for its draw, which is
/// the payload's per-eye re-issue contract. Outside the routed range `cb13`'s `M_eye` block is the
/// identity, so the rewrite is a no-op there.
///
/// [`reproject_vertex_shader`]: crate::reproject_vertex_shader
pub fn reproject_pinned_eye(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    if dxbc.chunk(b"SHEX").is_none() && dxbc.chunk(b"SHDR").is_some() {
        return Err(DxbcError::UnsupportedShaderModel);
    }
    let shex = dxbc.chunk(b"SHEX").ok_or(DxbcError::NoShaderChunk)?;
    let stream = TokenStream::new(shex.body(blob))?;
    if !matches!(stream.stage(), ShaderStage::Vertex | ShaderStage::Domain) {
        return Err(DxbcError::NoPositionOutput);
    }
    let isgn = dxbc
        .chunk(b"ISGN")
        .ok_or(DxbcError::MissingInputSignature)?;
    let osgn = dxbc
        .chunk(b"OSGN")
        .ok_or(DxbcError::MissingOutputSignature)?;
    let plan = plan_pinned(&stream)?;
    let new_shex = rewrite_pinned(&stream, &plan)?;
    reassemble(
        &dxbc,
        blob,
        isgn.body(blob).to_vec(),
        osgn.body(blob).to_vec(),
        new_shex,
        false,
    )
}

/// What the pinned-reprojection declaration scan learned.
struct PinnedPlan {
    /// The output register `SV_Position` is declared at (`dcl_output_siv oN, position`).
    pos_register: u32,
    /// The first of the two temps this rewrite claims: `rBase` (held at zero) at `temp_base`, `rClip`
    /// (the saved clip position) at `temp_base + 1`.
    temp_base: u32,
    /// The token index of the existing `dcl_temps`, if any (its count is bumped by two).
    dcl_temps_start: Option<usize>,
    /// The token index of the first non-declaration instruction, where the declarations and the
    /// prologue are injected.
    inject_before: usize,
}

/// Scans the declarations: the `SV_Position` output, the temp count and injection point, and the
/// preconditions (`cb13` unclaimed, a position output present, no `retc`).
fn plan_pinned(stream: &TokenStream) -> Result<PinnedPlan, DxbcError> {
    let tokens = stream.tokens();
    let mut temps: u32 = 0;
    let mut dcl_temps_start = None;
    let mut inject_before = None;
    let mut pos_register = None;

    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_OUTPUT_SIV if tokens[insn.end - 1] == SB_NAME_POSITION => {
                    pos_register = Some(declared_register(tokens, &insn)?);
                }
                OPCODE_DCL_TEMPS => {
                    temps = *tokens
                        .get(insn.start + 1)
                        .ok_or(DxbcError::UnexpectedEndOfTokens)?;
                    dcl_temps_start = Some(insn.start);
                }
                OPCODE_DCL_CONSTANT_BUFFER
                    if declared_register(tokens, &insn)? == STEREO_CB_REGISTER =>
                {
                    return Err(DxbcError::Cb13AlreadyDeclared);
                }
                _ => {}
            }
        } else {
            if inject_before.is_none() {
                inject_before = Some(insn.start);
            }
            // A conditional early return would need the epilogue on that path too.
            if insn.opcode == OPCODE_RETC {
                return Err(DxbcError::UnsupportedControlFlow);
            }
        }
    }

    Ok(PinnedPlan {
        pos_register: pos_register.ok_or(DxbcError::NoPositionOutput)?,
        temp_base: temps,
        dcl_temps_start,
        inject_before: inject_before.unwrap_or(tokens.len()),
    })
}

/// Rebuilds the shader chunk: injects the `cb13` declaration and the zero row base, renames every
/// `SV_Position` write to `rClip`, and emits the `M_eye · rClip` `dp4` chain before each `ret`.
fn rewrite_pinned(stream: &TokenStream, plan: &PinnedPlan) -> Result<Vec<u8>, DxbcError> {
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len() + 64);
    out.extend_from_slice(&tokens[..2]);

    let mut injected = false;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.start == plan.inject_before {
            emit_pinned_injection(&mut out, plan);
            injected = true;
        }
        if Some(insn.start) == plan.dcl_temps_start {
            out.push(tokens[insn.start]);
            out.push(plan.temp_base + 2);
            continue;
        }
        if insn.is_declaration() {
            out.extend_from_slice(&tokens[insn.start..insn.end]);
            continue;
        }
        if insn.opcode == OPCODE_RET {
            emit_meye_epilogue(&mut out, plan.pos_register, plan.temp_base);
            out.extend_from_slice(&tokens[insn.start..insn.end]);
            continue;
        }
        rewrite_reproject_instruction(tokens, &insn, plan.pos_register, plan.temp_base, &mut out)?;
    }
    if !injected {
        emit_pinned_injection(&mut out, plan);
    }

    let length = out.len() as u32;
    out[1] = length;
    Ok(out.iter().flat_map(|t| t.to_le_bytes()).collect())
}

/// Emits `dcl_constantbuffer cb13[18], dynamicIndexed`, a `dcl_temps` when the shader had none, and
/// `mov rBase.x, l(0)`: the first eye slot's `M_eye` block, which the payload's pin fills with the eye
/// being drawn.
fn emit_pinned_injection(out: &mut Vec<u32>, plan: &PinnedPlan) {
    let r = plan.temp_base;
    out.extend_from_slice(&[
        (4 << 24) | CB_ACCESS_DYNAMIC_INDEXED | OPCODE_DCL_CONSTANT_BUFFER,
        OPERAND_CB_2D_IMM,
        STEREO_CB_REGISTER,
        STEREO_REPROJ_CB_ROWS,
    ]);
    if plan.dcl_temps_start.is_none() {
        out.extend_from_slice(&[(2 << 24) | OPCODE_DCL_TEMPS, r + 2]);
    }
    out.extend_from_slice(&[
        (5 << 24) | OPCODE_MOV,
        OPERAND_TEMP_MASK_X,
        r,
        OPERAND_IMM32_SCALAR,
        0,
    ]);
}

/// Rewrites a terrain domain shader drawn once per eye to read its camera rows (the offset
/// view-projection and camera position of its `cb1` terrain constants) from `cb13`'s first eye slot,
/// which the payload's per-eye re-issue pins to the eye it draws, instead of reprojecting its
/// `SV_Position` by `M_eye`. The shader then builds the eye's clip position, distances, and fades with
/// the arithmetic of an eye rendered on its own. The rows are chosen in a prologue by
/// [`EYE_ROWS_SELECT_ROW`](crate::EYE_ROWS_SELECT_ROW), so outside the routed range the shader reads
/// its own constants. No interface changes, and no control-flow restriction: nothing is emitted
/// before a `ret`.
pub fn rebase_pinned_domain_shader(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    if dxbc.chunk(b"SHEX").is_none() && dxbc.chunk(b"SHDR").is_some() {
        return Err(DxbcError::UnsupportedShaderModel);
    }
    let shex = dxbc.chunk(b"SHEX").ok_or(DxbcError::NoShaderChunk)?;
    let stream = TokenStream::new(shex.body(blob))?;
    if stream.stage() != ShaderStage::Domain {
        return Err(DxbcError::NotDomainShader);
    }
    if !reads_terrain_view_projection(&stream)? {
        return Err(DxbcError::NoTerrainCameraRows);
    }
    let isgn = dxbc
        .chunk(b"ISGN")
        .ok_or(DxbcError::MissingInputSignature)?;
    let osgn = dxbc
        .chunk(b"OSGN")
        .ok_or(DxbcError::MissingOutputSignature)?;
    let plan = plan_rebase(&stream)?;
    let new_shex = rewrite_rebase(&stream, &plan)?;
    reassemble(
        &dxbc,
        blob,
        isgn.body(blob).to_vec(),
        osgn.body(blob).to_vec(),
        new_shex,
        false,
    )
}

/// What the pinned rebase's declaration scan learned.
struct RebasePlan {
    /// The first of the [`EYE_ROW_TEMPS`] temps the chosen rows occupy (the old temp count).
    rows_temp: u32,
    /// The token index of the existing `dcl_temps`, if any.
    dcl_temps_start: Option<usize>,
    /// The token index of the first non-declaration instruction.
    inject_before: usize,
}

fn plan_rebase(stream: &TokenStream) -> Result<RebasePlan, DxbcError> {
    let tokens = stream.tokens();
    let mut temps = 0;
    let mut dcl_temps_start = None;
    let mut inject_before = None;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_TEMPS => {
                    temps = *tokens
                        .get(insn.start + 1)
                        .ok_or(DxbcError::UnexpectedEndOfTokens)?;
                    dcl_temps_start = Some(insn.start);
                }
                OPCODE_DCL_CONSTANT_BUFFER
                    if declared_register(tokens, &insn)? == STEREO_CB_REGISTER =>
                {
                    return Err(DxbcError::Cb13AlreadyDeclared);
                }
                _ => {}
            }
        } else if inject_before.is_none() {
            inject_before = Some(insn.start);
        }
    }
    Ok(RebasePlan {
        rows_temp: temps,
        dcl_temps_start,
        inject_before: inject_before.unwrap_or(tokens.len()),
    })
}

fn rewrite_rebase(stream: &TokenStream, plan: &RebasePlan) -> Result<Vec<u8>, DxbcError> {
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len() + 96);
    out.extend_from_slice(&tokens[..2]);
    let mut injected = false;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.start == plan.inject_before {
            emit_rebase_injection(&mut out, plan);
            injected = true;
        }
        if Some(insn.start) == plan.dcl_temps_start {
            out.push(tokens[insn.start]);
            out.push(plan.rows_temp + EYE_ROW_TEMPS);
            continue;
        }
        if insn.is_declaration() {
            out.extend_from_slice(&tokens[insn.start..insn.end]);
            continue;
        }
        rewrite_rows_instruction(tokens, &insn, plan.rows_temp, &mut out)?;
    }
    if !injected {
        emit_rebase_injection(&mut out, plan);
    }
    let length = out.len() as u32;
    out[1] = length;
    Ok(out.iter().flat_map(|t| t.to_le_bytes()).collect())
}

/// Emits `dcl_constantbuffer cb13[25], dynamicIndexed`, a `dcl_temps` when the shader had none, and
/// the row-selecting prologue over the first eye slot.
fn emit_rebase_injection(out: &mut Vec<u32>, plan: &RebasePlan) {
    out.extend_from_slice(&[
        (4 << 24) | CB_ACCESS_DYNAMIC_INDEXED | OPCODE_DCL_CONSTANT_BUFFER,
        OPERAND_CB_2D_IMM,
        STEREO_CB_REGISTER,
        STEREO_SELECT_CB_ROWS,
    ]);
    if plan.dcl_temps_start.is_none() {
        out.extend_from_slice(&[(2 << 24) | OPCODE_DCL_TEMPS, plan.rows_temp + EYE_ROW_TEMPS]);
    }
    emit_select_rows(out, plan.rows_temp, RowBase::Pinned);
}
