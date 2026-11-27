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
//! vector the payload zeroes before this rewrite runs (the shadow PCF patch); a zero vector makes
//! the product a constant and is left alone.
//!
//! The rewrite composes after the eye-camera rewrite, which declines a shader already declaring the
//! buffer's slot: a slot declared at least [`EYE_CAMERA_CB_ROWS`] long is that rewrite's and is read
//! as it is; a shorter one is not, and the shader is declined. Other reads of the pixel position (a
//! texel fetch, a screen UV) address the double-wide target itself and are left alone.

use crate::{
    container::{Dxbc, DxbcError},
    isa::{
        INTERPOLATION_LINEAR_NOPERSPECTIVE, MODIFIER_NEG, OPCODE_ADD, OPCODE_AND,
        OPCODE_DCL_CONSTANT_BUFFER, OPCODE_DCL_INPUT_PS_SIV, OPCODE_DCL_OUTPUT,
        OPCODE_DCL_OUTPUT_SIV, OPCODE_DCL_TEMPS, OPCODE_DIV, OPCODE_DP2, OPCODE_GE, OPCODE_LT,
        OPCODE_MAD, OPCODE_MIN, OPCODE_MOV, OPCODE_MOVC, OPCODE_RCP, OPERAND_CB_2D_SELECT_X,
        OPERAND_CB_2D_SELECT_Z, OPERAND_EXTENDED, OPERAND_IMM32_SCALAR, OPERAND_IMM32_VECTOR,
        OPERAND_INPUT_MASK_XY, OPERAND_INPUT_SELECT_X, OPERAND_INPUT_SELECT_Y, OPERAND_TEMP_MASK_W,
        OPERAND_TEMP_MASK_X, OPERAND_TEMP_MASK_XY, OPERAND_TEMP_MASK_Y, OPERAND_TEMP_MASK_Z,
        OPERAND_TEMP_MASK_ZW, OPERAND_TEMP_SELECT_X, OPERAND_TEMP_SWIZZLE_XYXX,
        OPERAND_TEMP_SWIZZLE_ZWZZ, OPERAND_TYPE_INPUT, SB_NAME_POSITION, SELECTION_SWIZZLE,
    },
    rewrite::{
        camera::{EYE_CAMERA_BOUNDARY_ROW, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_CB_ROWS},
        common::{OPERAND_CB_2D_IMM, declared_register, reassemble, signature_register},
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
    out.extend([
        OPERAND_TEMP_SELECT_X | OPERAND_EXTENDED,
        MODIFIER_NEG,
        local,
    ]);
    out.extend([(5 << 24) | OPCODE_MOV, OPERAND_TEMP_MASK_Y, local]);
    out.extend([OPERAND_INPUT_SELECT_Y, position]);
}

/// Rewrite a pixel shader whose screen-door pattern is hashed from its own projected position,
/// rather than from `SV_Position`, to hash the eye-local pixel position instead.
///
/// The tree impostors' permutations build the hash's input from a clip position their vertex shader
/// passes down, divided by `w` and by twice the inverse target size in `cb0[8].zw`:
///
/// ```text
/// div r0.xy, v8.yzyy, v8.wwww                 // the projected position, in NDC
/// add r0.zw, cb0[8].zzzw, cb0[8].zzzw         // twice the inverse target size
/// div r0.xy, r0.xyxx, r0.zwzz                 // pixels from the target's centre
/// dp2 r0.x, r0.xyxx, l(0.467944, -0.703648, 0, 0)
/// ```
///
/// In an eye-sized render the projected position is the rasterized one, so that is the pixel's
/// offset from the target's centre. Under the double-wide collapse it is neither: the clip position
/// is the centred camera's, ahead of the reprojection the eye's `SV_Position` takes, and the target
/// is both eyes wide, so the offset comes out twice as wide and centred between the eyes.
///
/// [`localize_projected_screen_door`] points the `dp2` at the offset an eye-sized render computes,
/// built from the pixel position: `x` made eye-local as [`localize_screen_door`] does, less half the
/// eye's width, and half the target's height less `y`. The eye's width is the boundary an eye-split
/// pass publishes (the right eye's first pixel is the left eye's width). Everywhere else the boundary
/// lies past every pixel, and the `dp2` reads the engine's own offset. The original instructions
/// are left in place; the `dp2` alone reads the selected offset. Returns
/// [`DxbcError::NoScreenDoor`] for a shader that builds no such pattern.
pub fn localize_projected_screen_door(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
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
    let plan = projected_plan(&stream, position)?;
    let new_shex = projected_rewrite(&stream, &plan, position)?;
    let osgn = dxbc
        .chunk(b"OSGN")
        .ok_or(DxbcError::MissingOutputSignature)?
        .body(blob)
        .to_vec();
    reassemble(&dxbc, blob, isgn.to_vec(), osgn, new_shex, false)
}

/// What the scan over a projected-hash shader learned.
struct ProjectedPlan {
    /// The temp holding the eye-local offset (the old `dcl_temps` count).
    local: u32,
    /// Whether the eye-camera rewrite already declared the buffer the boundary is read from.
    has_buffer: bool,
    /// The token index of the position's input declaration, if the shader declares it.
    position_declaration: Option<usize>,
    /// The token index of the `dcl_temps` declaration.
    temps_declaration: usize,
    /// The token index of the declaration a new position declaration goes ahead of: the first
    /// output declaration, or `dcl_temps` in a shader declaring no outputs before it.
    inputs_end: usize,
    /// For every pattern site, the token index of its `dp2` and of the temp operand it reads.
    sites: Vec<(usize, usize)>,
}

/// Scan the declarations and the projected-hash sites, and check the preconditions.
fn projected_plan(stream: &TokenStream, position: u32) -> Result<ProjectedPlan, DxbcError> {
    let tokens = stream.tokens();
    let mut temps = None;
    let mut has_buffer = false;
    let mut global_rows = 0;
    let mut position_declaration = None;
    let mut first_output = None;
    let mut sites = Vec::new();
    let mut window: [Option<Instruction<'_>>; 2] = [None, None];
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_TEMPS => temps = Some(insn.start),
                OPCODE_DCL_OUTPUT..=OPCODE_DCL_OUTPUT_SIV => {
                    first_output.get_or_insert(insn.start);
                }
                OPCODE_DCL_CONSTANT_BUFFER => {
                    let register = declared_register(tokens, &insn)?;
                    let rows = *tokens
                        .get(insn.start + 3)
                        .ok_or(DxbcError::UnexpectedEndOfTokens)?;
                    if register == EYE_CAMERA_CB_REGISTER {
                        if rows < EYE_CAMERA_CB_ROWS {
                            return Err(DxbcError::ScreenDoorSlotTaken);
                        }
                        has_buffer = true;
                    } else if register == GLOBALS_CB_REGISTER {
                        global_rows = rows;
                    }
                }
                OPCODE_DCL_INPUT_PS_SIV
                    if tokens.get(insn.start + 2) == Some(&position)
                        && tokens
                            .get(insn.start + 1)
                            .is_some_and(|&tok| tok & 0x3 == 2 && (tok >> 2) & 0x3 == 0) =>
                {
                    position_declaration = Some(insn.start);
                }
                _ => {}
            }
            continue;
        }
        if let [Some(add), Some(div)] = &window
            && let Some(operand) = projected_site(tokens, add, div, &insn)
        {
            sites.push((insn.start, operand));
        }
        window = [window[1].take(), Some(insn)];
    }
    if sites.is_empty() {
        return Err(DxbcError::NoScreenDoor);
    }
    let temps_declaration = temps.ok_or(DxbcError::NoScreenDoor)?;
    if global_rows <= TARGET_SIZE_ROW {
        return Err(DxbcError::NoScreenDoor);
    }
    Ok(ProjectedPlan {
        local: tokens[temps_declaration + 1],
        has_buffer,
        position_declaration,
        temps_declaration,
        inputs_end: first_output
            .filter(|&at| at < temps_declaration)
            .unwrap_or(temps_declaration),
        sites,
    })
}

/// The token index of the temp operand a projected-hash `dp2` reads, when `add`, `div`, and `dp2`
/// are the last three instructions of the pattern in the [`localize_projected_screen_door`] docs:
/// twice the target's inverse size into a temp's `.zw`, the projected position divided by it into
/// another temp's `.xy`, and that temp's `.xy` hashed with the screen-door vector.
fn projected_site(
    tokens: &[u32],
    add: &Instruction<'_>,
    div: &Instruction<'_>,
    dp2: &Instruction<'_>,
) -> Option<usize> {
    let add = &tokens[add.start..add.end];
    let div = &tokens[div.start..div.end];
    let hash = &tokens[dp2.start..dp2.end];
    let [
        ADD_9,
        OPERAND_TEMP_MASK_ZW,
        size,
        CB_ZZZW,
        GLOBALS_CB_REGISTER,
        TARGET_SIZE_ROW,
        CB_ZZZW,
        GLOBALS_CB_REGISTER,
        TARGET_SIZE_ROW,
    ] = *add
    else {
        return None;
    };
    let [
        DIV_7,
        OPERAND_TEMP_MASK_XY,
        offset,
        OPERAND_TEMP_SWIZZLE_XYXX,
        numerator,
        OPERAND_TEMP_SWIZZLE_ZWZZ,
        denominator,
    ] = *div
    else {
        return None;
    };
    let [
        DP2_10,
        _,
        _,
        OPERAND_TEMP_SWIZZLE_XYXX,
        hashed,
        OPERAND_IMM32_VECTOR,
        SCREEN_DOOR_X,
        SCREEN_DOOR_Y,
        0,
        0,
    ] = *hash
    else {
        return None;
    };
    (numerator == offset && denominator == size && hashed == offset).then_some(dp2.start + 3)
}

/// Rebuild the shader chunk: widen (or add) the position's input declaration to `.xy`, declare the
/// buffer when the eye-camera rewrite has not, bump the temp count, build the eye-local offset before
/// each site, and point the site's `dp2` at it.
fn projected_rewrite(
    stream: &TokenStream,
    plan: &ProjectedPlan,
    position: u32,
) -> Result<Vec<u8>, DxbcError> {
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len() + 64 * plan.sites.len());
    out.extend_from_slice(&tokens[..2]);
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.start == plan.inputs_end && plan.position_declaration.is_none() {
            out.extend([
                (4 << 24) | (INTERPOLATION_LINEAR_NOPERSPECTIVE << 11) | OPCODE_DCL_INPUT_PS_SIV,
                OPERAND_INPUT_MASK_XY,
                position,
                SB_NAME_POSITION,
            ]);
        }
        if insn.start == plan.temps_declaration {
            if !plan.has_buffer {
                out.extend([
                    (4 << 24) | OPCODE_DCL_CONSTANT_BUFFER,
                    OPERAND_CB_2D_IMM,
                    EYE_CAMERA_CB_REGISTER,
                    EYE_CAMERA_CB_ROWS,
                ]);
            }
            out.extend([tokens[insn.start], plan.local + 1]);
            continue;
        }
        if Some(insn.start) == plan.position_declaration {
            out.push(tokens[insn.start]);
            out.push(tokens[insn.start + 1] | MASK_XY);
            out.extend_from_slice(&tokens[insn.start + 2..insn.end]);
            continue;
        }
        match plan.sites.iter().find(|&&(at, _)| at == insn.start) {
            Some(&(_, operand)) => {
                emit_projected_offset(&mut out, plan.local, tokens[operand + 1], position);
                out.extend_from_slice(&tokens[insn.start..=operand]);
                out.push(plan.local);
                out.extend_from_slice(&tokens[operand + 2..insn.end]);
            }
            None => out.extend_from_slice(&tokens[insn.start..insn.end]),
        }
    }
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    Ok(out.iter().flat_map(|t| t.to_le_bytes()).collect())
}

/// Emit the eye-local offset from the eye's centre into `local.xy`, or the engine's own offset
/// (`engine.xy`) where no eye boundary lies inside the target:
///
/// ```text
/// rcp  t.z, cb0[8].z               // the target's width
/// lt   t.w, boundary, t.z          // whether an eye boundary lies inside the target
/// ge   t.x, v0.x, boundary         // all ones at or past the boundary
/// and  t.x, t.x, boundary          // the boundary, or zero
/// add  t.x, v0.x, -t.x             // the eye-local x
/// min  t.z, t.z, boundary          // the eye's width
/// mad  t.x, t.z, l(-0.5), t.x      // x from the eye's centre
/// rcp  t.z, cb0[8].w               // the target's height
/// mad  t.y, t.z, l(0.5), -v0.y     // y from the centre, upwards
/// movc t.xy, t.wwww, t.xyxx, engine.xyxx
/// ```
///
/// Outside an eye-split pass (the shadow and reflection passes) the shader so hashes exactly what
/// the engine's does, whatever target size `cb0[8]` holds there.
fn emit_projected_offset(out: &mut Vec<u32>, local: u32, engine: u32, position: u32) {
    let boundary = [
        OPERAND_CB_2D_SELECT_X,
        EYE_CAMERA_CB_REGISTER,
        EYE_CAMERA_BOUNDARY_ROW,
    ];
    out.extend([(6 << 24) | OPCODE_RCP, OPERAND_TEMP_MASK_Z, local]);
    out.extend([OPERAND_CB_2D_SELECT_Z, GLOBALS_CB_REGISTER, TARGET_SIZE_ROW]);
    out.extend([(8 << 24) | OPCODE_LT, OPERAND_TEMP_MASK_W, local]);
    out.extend(boundary);
    out.extend([OPERAND_TEMP_SELECT_Z, local]);
    out.extend([(8 << 24) | OPCODE_GE, OPERAND_TEMP_MASK_X, local]);
    out.extend([OPERAND_INPUT_SELECT_X, position]);
    out.extend(boundary);
    out.extend([(8 << 24) | OPCODE_AND, OPERAND_TEMP_MASK_X, local]);
    out.extend([OPERAND_TEMP_SELECT_X, local]);
    out.extend(boundary);
    out.extend([(8 << 24) | OPCODE_ADD, OPERAND_TEMP_MASK_X, local]);
    out.extend([OPERAND_INPUT_SELECT_X, position]);
    out.extend([
        OPERAND_TEMP_SELECT_X | OPERAND_EXTENDED,
        MODIFIER_NEG,
        local,
    ]);
    out.extend([(8 << 24) | OPCODE_MIN, OPERAND_TEMP_MASK_Z, local]);
    out.extend([OPERAND_TEMP_SELECT_Z, local]);
    out.extend(boundary);
    out.extend([(9 << 24) | OPCODE_MAD, OPERAND_TEMP_MASK_X, local]);
    out.extend([OPERAND_TEMP_SELECT_Z, local]);
    out.extend([OPERAND_IMM32_SCALAR, (-0.5f32).to_bits()]);
    out.extend([OPERAND_TEMP_SELECT_X, local]);
    out.extend([(6 << 24) | OPCODE_RCP, OPERAND_TEMP_MASK_Z, local]);
    out.extend([OPERAND_CB_2D_SELECT_W, GLOBALS_CB_REGISTER, TARGET_SIZE_ROW]);
    out.extend([(10 << 24) | OPCODE_MAD, OPERAND_TEMP_MASK_Y, local]);
    out.extend([OPERAND_TEMP_SELECT_Z, local]);
    out.extend([OPERAND_IMM32_SCALAR, 0.5f32.to_bits()]);
    out.extend([
        OPERAND_INPUT_SELECT_Y | OPERAND_EXTENDED,
        MODIFIER_NEG,
        position,
    ]);
    out.extend([(9 << 24) | OPCODE_MOVC, OPERAND_TEMP_MASK_XY, local]);
    out.extend([OPERAND_TEMP_SWIZZLE_WWWW, local]);
    out.extend([OPERAND_TEMP_SWIZZLE_XYXX, local]);
    out.extend([OPERAND_TEMP_SWIZZLE_XYXX, engine]);
}

/// The engine's global constant buffer.
const GLOBALS_CB_REGISTER: u32 = 0;
/// The row of the engine's global constants whose `.zw` hold the target's inverse width and height.
const TARGET_SIZE_ROW: u32 = 8;
/// The screen-door vector's `x`, `0.467944`.
const SCREEN_DOOR_X: u32 = 0x3eef_965b;
/// The screen-door vector's `y`, `-0.703648`.
const SCREEN_DOOR_Y: u32 = 0xbf34_2246;
/// The opcode tokens of a nine-token `add`, a seven-token `div`, and a ten-token `dp2`.
const ADD_9: u32 = (9 << 24) | OPCODE_ADD;
const DIV_7: u32 = (7 << 24) | OPCODE_DIV;
const DP2_10: u32 = (10 << 24) | OPCODE_DP2;
/// The `.xy` component-mask bits of a masked operand token.
const MASK_XY: u32 = 0x30;
/// A temp register operand selecting `.z` and reading `.wwww`.
const OPERAND_TEMP_SELECT_Z: u32 = 0x0010_002a;
const OPERAND_TEMP_SWIZZLE_WWWW: u32 = 0x0010_0ff6;
/// A constant-buffer operand with two immediate indices reading `.zzzw` and selecting `.w`.
const CB_ZZZW: u32 = 0x0020_8ea6;
const OPERAND_CB_2D_SELECT_W: u32 = 0x0020_803a;
