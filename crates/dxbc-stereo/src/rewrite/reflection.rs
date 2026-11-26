//! Trace the screen-space reflections over each eye's half of a double-wide target in one draw.
//!
//! The reflection ray march (`screenspaceraytracereflection`, and the simpler `screenspacereflection`
//! permutation) draws one quad over the block's half-resolution reflection target. Each pixel
//! rebuilds its world position from the packed linear depth along a view ray its vertex shader
//! interpolates (`TEXCOORD1`), reflects the view vector about the normal, and marches the reflected
//! ray through the depth buffer. The tracer projects the ray's two ends with the view and projection
//! the block stages in `cbTechConstants`, steps between them in the target's pixels, whose size it
//! also stages there, and samples the depth along the way; at a hit it turns the hit back into a world
//! position with the camera transform, and looks the reflected colour up in the previous frame's scene
//! through the previous view-projection, and the reflective water planes through the current one, both
//! staged in `cbConstants`:
//!
//! ```text
//! mad r0.xzw, r0.xxxx, v2.xxyz, cb0[4].xxyz            // the world position, along the view ray
//! mad r2.xyz, r0.xxxx, cb2[4].xyzx, r2.xyzx            // into view space
//! mad r2.xyz, r0.wwww, cb2[2].xywx, r2.xyzx            // projected
//! mul r6.xy, r3.xyxx, cb2[12].xyxx                     // into the target's pixels
//! sample_l r5.w, r10.xyxx, t2.yzwx, s0, l(0)           // a step's depth, at `pixel / size`
//! mad r1.xyzw, r1.wwww, cb2[10].xyzw, r2.xyzw          // the hit, back into the world
//! mad r0.yzw, r1.wwww, cb1[7].xxyw, r0.yyzw            // the previous frame's clip position
//! ```
//!
//! Under a double-wide target one view ray basis, one view, one projection, and one target size serve
//! both eyes. [`localize_screen_space_reflection`] makes each half trace what an eye-sized target
//! traces:
//!
//! - The eye is the half `TEXCOORD0.x` falls in. Each eye's view, projection, camera transform, and
//!   the two view-projections are read from a mod-owned constant buffer,
//!   [`SCREEN_SPACE_REFLECTION_CB_REGISTER`], at [`SCREEN_SPACE_REFLECTION_ROWS_PER_EYE`] rows per eye
//!   in the order [`ScreenSpaceReflectionRow`] names, indexed by that eye.
//! - The view ray is rebuilt per pixel from three of the eye's rows,
//!   `TEXCOORD0.x * a + TEXCOORD0.y * b + c`, in place of the interpolated one, so the caller folds the
//!   eye's half of the target into them.
//! - The target size the tracer reads is halved horizontally, so the march runs in the eye's own pixels
//!   and stops at the eye's edge as it does at the edge of an eye-sized target, and every depth sample
//!   along the march is moved into the eye's half of the depth texture. The pixel position the march
//!   jitters its start by is made eye-local the same way.
//!
//! The simpler permutation projects with the current view-projection and reads no target size, so its
//! march runs over the double-wide target's own UV and its depth samples are left where they are. Its
//! `cbTechConstants` holds tuning rather than matrices and is left alone.
//!
//! The pass is found by the names its resource table binds the linear depth and the previous scene
//! under and the two constant arrays. A shader without them, or one whose view ray and constants
//! are read in a shape this rewrite does not recognise, is declined.

use crate::{
    container::{Dxbc, DxbcError},
    isa::{
        IDX_IMM32_PLUS_RELATIVE, LENGTH_MASK, MODIFIER_NEG, OPCODE_ADD, OPCODE_AND,
        OPCODE_DCL_CONSTANT_BUFFER, OPCODE_DCL_TEMPS, OPCODE_GE, OPCODE_MAD, OPCODE_MOV,
        OPCODE_MUL, OPCODE_SAMPLE, OPCODE_SAMPLE_L, OPERAND_TEMP_SWIZZLE_XYXX,
        OPERAND_TYPE_CONSTANT_BUFFER, OPERAND_TYPE_INPUT, OPERAND_TYPE_RESOURCE, OPERAND_TYPE_TEMP,
        SELECTION_SWIZZLE, W, X, Y, Z, dst, dst_mask, imm, imm4, input, instruction, select, src,
        src_mod, src_xyzw,
    },
    rewrite::{
        common::{
            CB_ACCESS_DYNAMIC_INDEXED, OPERAND_CB_2D_IMM, declared_register, reassemble,
            signature_register,
        },
        planar::{BINDING_TEXTURE, binding, constant_location},
    },
    tokens::{Instruction, ShaderStage, TokenStream},
};

/// The constant-buffer slot the rewritten shader reads each eye's rows from. Free across the game's
/// pixel shaders, and bound by the payload only for the length of the reflection block.
pub const SCREEN_SPACE_REFLECTION_CB_REGISTER: u32 = 13;

/// The rows of one eye's block in [`SCREEN_SPACE_REFLECTION_CB_REGISTER`], by their first row. The
/// matrices are laid out as the engine stages them, one engine row per constant row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ScreenSpaceReflectionRow {
    /// The eye's projection (`cbTechConstants` rows 0..3).
    Projection = 0,
    /// The eye's world-to-view matrix (`cbTechConstants` rows 4..7).
    View = 4,
    /// The eye's world transform, the inverse of the view (`cbTechConstants` rows 8..11).
    CameraTransform = 8,
    /// The eye's world-to-clip view-projection, remapped onto the eye's half of the target
    /// (`cbConstants` rows 0..3).
    ViewProjection = 12,
    /// The previous frame's view-projection, remapped onto the eye's half of the target
    /// (`cbConstants` rows 4..7).
    PreviousViewProjection = 16,
    /// The view ray's `TEXCOORD0.x` coefficient, then its `TEXCOORD0.y` coefficient, then its
    /// constant term, in `.xyz`.
    ViewRay = 20,
}

/// The rows each eye occupies in [`SCREEN_SPACE_REFLECTION_CB_REGISTER`].
pub const SCREEN_SPACE_REFLECTION_ROWS_PER_EYE: u32 = 23;

/// Rewrite a screen-space reflection ray-march pixel shader so each half of a double-wide target traces
/// its own eye. Returns [`DxbcError::NoScreenSpaceReflection`] for any other shader, or for one whose
/// shape this rewrite does not recognise.
pub fn localize_screen_space_reflection(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let rdef = chunk(&dxbc, blob, b"RDEF").ok_or(DxbcError::NoScreenSpaceReflection)?;
    let depth =
        binding(rdef, DEPTH_TEXTURE, BINDING_TEXTURE).ok_or(DxbcError::NoScreenSpaceReflection)?;
    binding(rdef, PREVIOUS_SCENE_TEXTURE, BINDING_TEXTURE)
        .ok_or(DxbcError::NoScreenSpaceReflection)?;
    let constants = constant_location(rdef, CONSTANTS).ok_or(DxbcError::NoScreenSpaceReflection)?;
    let tech = constant_location(rdef, TECH_CONSTANTS).ok_or(DxbcError::NoScreenSpaceReflection)?;
    if (
        constants.register,
        constants.component,
        tech.register,
        tech.component,
    ) != (0, 0, 0, 0)
    {
        return Err(DxbcError::NoScreenSpaceReflection);
    }
    let isgn = chunk(&dxbc, blob, b"ISGN").ok_or(DxbcError::MissingInputSignature)?;
    let (texcoord, _) =
        signature_register(isgn, "TEXCOORD", 0).ok_or(DxbcError::NoScreenSpaceReflection)?;
    let (ray, _) =
        signature_register(isgn, "TEXCOORD", 1).ok_or(DxbcError::NoScreenSpaceReflection)?;
    let position = signature_register(isgn, "SV_Position", 0).map(|(register, _)| register);
    let stream =
        TokenStream::new(chunk(&dxbc, blob, b"SHEX").ok_or(DxbcError::UnsupportedShaderModel)?)?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoScreenSpaceReflection);
    }
    let tokens = stream.tokens();

    let mut temps = None;
    let mut reads_size = false;
    for insn in stream.instructions() {
        let insn = insn?;
        match insn.opcode {
            OPCODE_DCL_TEMPS => temps = tokens.get(insn.start + 1).copied(),
            OPCODE_DCL_CONSTANT_BUFFER
                if declared_register(tokens, &insn)? == SCREEN_SPACE_REFLECTION_CB_REGISTER =>
            {
                return Err(DxbcError::Cb13AlreadyDeclared);
            }
            _ if insn.is_declaration() => {}
            _ => {
                for operand in insn.operands() {
                    let operand = &tokens[operand?.token_offset..];
                    if constant_row(operand) == Some((tech.slot, TARGET_SIZE_ROW)) {
                        reads_size = true;
                    }
                }
            }
        }
    }
    let first = temps.ok_or(DxbcError::NoScreenSpaceReflection)?;
    let mut rewriter = Rewriter {
        tokens,
        registers: Registers {
            eye: first,
            ray: first + 1,
            position: first + 2,
            coordinate: first + 3,
            size: first + 4,
        },
        inputs: Inputs {
            texcoord,
            ray,
            position,
            depth,
            constants: constants.slot,
            tech: tech.slot,
        },
        eye_pixels: reads_size,
        out: tokens[..2].to_vec(),
        counts: Counts::default(),
    };
    let mut prologue_emitted = false;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            if insn.opcode == OPCODE_DCL_TEMPS {
                rewriter
                    .out
                    .extend([tokens[insn.start], first + ADDED_TEMPS]);
            } else {
                rewriter
                    .out
                    .extend_from_slice(&tokens[insn.start..insn.end]);
            }
            continue;
        }
        if !prologue_emitted {
            rewriter.emit_prologue();
            prologue_emitted = true;
        }
        rewriter.rewrite(&insn)?;
    }
    let counts = rewriter.counts;
    let recognised = counts.ray > 0
        && counts.view_projection > 0
        && counts.previous > 0
        && (!reads_size || (counts.tech > 0 && counts.march_samples > 0));
    if !recognised {
        return Err(DxbcError::NoScreenSpaceReflection);
    }
    let mut out = rewriter.out;
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        blob,
        isgn.to_vec(),
        chunk(&dxbc, blob, b"OSGN")
            .ok_or(DxbcError::MissingOutputSignature)?
            .to_vec(),
        out.iter().flat_map(|v| v.to_le_bytes()).collect(),
        false,
    )
}

/// The body of a chunk.
fn chunk<'a>(dxbc: &Dxbc, blob: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    dxbc.chunk(tag).map(|c| c.body(blob))
}

/// The names the ray march binds its inputs under.
const DEPTH_TEXTURE: &[u8] = b"LinearDepthTexture";
const PREVIOUS_SCENE_TEXTURE: &[u8] = b"PrevSceneTexture";
const CONSTANTS: &[u8] = b"Constants";
const TECH_CONSTANTS: &[u8] = b"TechConstants";

/// The `cbTechConstants` row holding the target size `(width, height, 1 / width, 1 / height)`.
const TARGET_SIZE_ROW: u32 = 12;

/// The temporaries the rewrite adds after the shader's own.
const ADDED_TEMPS: u32 = 5;

/// The register numbers the rewrite writes.
#[derive(Clone, Copy)]
struct Registers {
    /// `.x` all ones in the right eye's half, `.y` the eye's first row in the constant buffer, `.z`
    /// the eye's first UV (`0` or `0.5`), `.w` the eye index as a float.
    eye: u32,
    /// The eye's view ray.
    ray: u32,
    /// The eye-local pixel position.
    position: u32,
    /// A march sample's coordinate in the eye's half.
    coordinate: u32,
    /// The eye's target size.
    size: u32,
}

/// Where the shader's own inputs live.
#[derive(Clone, Copy)]
struct Inputs {
    /// The screen UV input.
    texcoord: u32,
    /// The interpolated view ray input.
    ray: u32,
    /// The pixel position input, when the shader declares one.
    position: Option<u32>,
    /// The linear depth texture's register.
    depth: u32,
    /// The slots `cbConstants` and `cbTechConstants` are bound to.
    constants: u32,
    tech: u32,
}

#[derive(Default, Clone, Copy)]
struct Counts {
    ray: usize,
    view_projection: usize,
    previous: usize,
    tech: usize,
    march_samples: usize,
}

struct Rewriter<'a> {
    tokens: &'a [u32],
    registers: Registers,
    inputs: Inputs,
    /// Whether the march runs in the target's pixels, so its depth samples need moving into the eye's
    /// half.
    eye_pixels: bool,
    out: Vec<u32>,
    counts: Counts,
}

impl Rewriter<'_> {
    fn emit_prologue(&mut self) {
        let Registers {
            eye,
            ray,
            position,
            size,
            ..
        } = self.registers;
        let texcoord = self.inputs.texcoord;
        self.out.extend([
            (4 << 24) | CB_ACCESS_DYNAMIC_INDEXED | OPCODE_DCL_CONSTANT_BUFFER,
            OPERAND_CB_2D_IMM,
            SCREEN_SPACE_REFLECTION_CB_REGISTER,
            2 * SCREEN_SPACE_REFLECTION_ROWS_PER_EYE,
        ]);
        self.emit(
            OPCODE_GE,
            &[&dst(eye, X), &input(texcoord, X), &imm(0.5f32.to_bits())],
        );
        self.emit(
            OPCODE_AND,
            &[
                &dst(eye, Y),
                &src(eye, X),
                &imm(SCREEN_SPACE_REFLECTION_ROWS_PER_EYE),
            ],
        );
        self.emit(
            OPCODE_AND,
            &[&dst(eye, Z), &src(eye, X), &imm(0.5f32.to_bits())],
        );
        self.emit(
            OPCODE_AND,
            &[&dst(eye, W), &src(eye, X), &imm(1f32.to_bits())],
        );
        let ray_row = ScreenSpaceReflectionRow::ViewRay as u32;
        self.emit(
            OPCODE_MUL,
            &[
                &dst_mask(ray, XYZ),
                &input(texcoord, X),
                &eye_row(ray_row, eye),
            ],
        );
        self.emit(
            OPCODE_MAD,
            &[
                &dst_mask(ray, XYZ),
                &input(texcoord, Y),
                &eye_row(ray_row + 1, eye),
                &src_xyzw(ray),
            ],
        );
        self.emit(
            OPCODE_ADD,
            &[
                &dst_mask(ray, XYZ),
                &src_xyzw(ray),
                &eye_row(ray_row + 2, eye),
            ],
        );
        if self.eye_pixels {
            self.emit(
                OPCODE_MUL,
                &[
                    &dst_mask(size, XYZW),
                    &[OPERAND_CB_2D_IMM, self.inputs.tech, TARGET_SIZE_ROW],
                    &imm4([0.5, 1.0, 2.0, 1.0]),
                ],
            );
        }
        if let Some(pixel) = self.inputs.position {
            if self.eye_pixels {
                self.emit(
                    OPCODE_MAD,
                    &[
                        &dst(position, X),
                        &src_mod(eye, W, MODIFIER_NEG),
                        &src(size, X),
                        &input(pixel, X),
                    ],
                );
            } else {
                self.emit(OPCODE_MOV, &[&dst(position, X), &input(pixel, X)]);
            }
            self.emit(OPCODE_MOV, &[&dst(position, Y), &input(pixel, Y)]);
        }
    }

    fn rewrite(&mut self, insn: &Instruction<'_>) -> Result<(), DxbcError> {
        let tokens = self.tokens;
        let offsets: Vec<usize> = insn
            .operands()
            .map(|operand| operand.map(|operand| operand.token_offset))
            .collect::<Result<_, _>>()?;
        let span = |index: usize| {
            let end = offsets.get(index + 1).copied().unwrap_or(insn.end);
            offsets[index]..end
        };
        let reads_depth = (0..offsets.len())
            .any(|index| resource_register(&tokens[span(index)]) == Some(self.inputs.depth));
        let mut coordinate = None;
        if reads_depth && self.eye_pixels {
            if !matches!(insn.opcode, OPCODE_SAMPLE | OPCODE_SAMPLE_L) || offsets.len() < 4 {
                return Err(DxbcError::NoScreenSpaceReflection);
            }
            coordinate = self.emit_march_coordinate(&tokens[span(1)])?;
        }

        let start = self.out.len();
        self.out
            .extend_from_slice(&tokens[insn.start..insn.operands_start()]);
        for index in 0..offsets.len() {
            if index == 1
                && let Some(moved) = coordinate
            {
                self.out.extend(moved);
                continue;
            }
            self.rewrite_operand(&tokens[span(index)]);
        }
        let length = self.out.len() - start;
        self.out[start] = (self.out[start] & !LENGTH_MASK) | instruction(0, length)?;
        Ok(())
    }

    /// Move a march sample's coordinate into the eye's half of the depth texture, returning the
    /// coordinate operand the sample reads instead, or `None` for the sample at the pixel's own screen
    /// UV, which already addresses the double-wide texture.
    fn emit_march_coordinate(&mut self, operand: &[u32]) -> Result<Option<[u32; 2]>, DxbcError> {
        let Registers {
            eye, coordinate, ..
        } = self.registers;
        let source = plain_source(operand).ok_or(DxbcError::NoScreenSpaceReflection)?;
        match source.kind {
            SourceKind::Input if source.register == self.inputs.texcoord => return Ok(None),
            SourceKind::Input => return Err(DxbcError::NoScreenSpaceReflection),
            SourceKind::Temp => {}
        }
        let [u, v] = source.components;
        self.emit(
            OPCODE_MAD,
            &[
                &dst(coordinate, X),
                &src(source.register, u),
                &imm(0.5f32.to_bits()),
                &src(eye, Z),
            ],
        );
        self.emit(OPCODE_MOV, &[&dst(coordinate, Y), &src(source.register, v)]);
        self.counts.march_samples += 1;
        Ok(Some([OPERAND_TEMP_SWIZZLE_XYXX, coordinate]))
    }

    /// Copy an operand, redirecting the view ray and the pixel position to their per-eye temporaries,
    /// the target size to its halved copy, and the per-eye matrix rows to the eye's block of the
    /// mod-owned constant buffer.
    fn rewrite_operand(&mut self, operand: &[u32]) {
        let token = operand[0];
        let extended = (token >> 31) as usize;
        let operand_type = (token >> 12) & 0xff;
        let dimensions = (token >> 20) & 0x3;
        let representations = (token >> 22) & 0x1ff;
        let indices = &operand[1 + extended..];
        if operand_type == OPERAND_TYPE_INPUT && dimensions == 1 && representations == 0 {
            let temp = match indices {
                [register] if *register == self.inputs.ray => {
                    self.counts.ray += 1;
                    Some(self.registers.ray)
                }
                [register] if Some(*register) == self.inputs.position => {
                    Some(self.registers.position)
                }
                _ => None,
            };
            if let Some(temp) = temp {
                self.out.push(token & !(0xff << 12));
                self.out.extend_from_slice(&operand[1..1 + extended]);
                self.out.push(temp);
                return;
            }
        }
        if let Some((slot, row)) = constant_row(operand) {
            if self.eye_pixels && slot == self.inputs.tech && row == TARGET_SIZE_ROW {
                self.out.push((token & !(0xff << 12 | 0x3 << 20)) | 1 << 20);
                self.out.extend_from_slice(&operand[1..1 + extended]);
                self.out.push(self.registers.size);
                return;
            }
            let eye_row = if self.eye_pixels && slot == self.inputs.tech && row < TARGET_SIZE_ROW {
                self.counts.tech += 1;
                Some(ScreenSpaceReflectionRow::Projection as u32 + row)
            } else if slot == self.inputs.constants && row < 4 {
                self.counts.view_projection += 1;
                Some(ScreenSpaceReflectionRow::ViewProjection as u32 + row)
            } else if slot == self.inputs.constants && (4..8).contains(&row) {
                self.counts.previous += 1;
                Some(ScreenSpaceReflectionRow::PreviousViewProjection as u32 + row - 4)
            } else {
                None
            };
            if let Some(eye_row) = eye_row {
                self.out.push(token | IDX_IMM32_PLUS_RELATIVE << 25);
                self.out.extend_from_slice(&operand[1..1 + extended]);
                self.out.extend([
                    SCREEN_SPACE_REFLECTION_CB_REGISTER,
                    eye_row,
                    select(OPERAND_TYPE_TEMP, Y),
                    self.registers.eye,
                ]);
                return;
            }
        }
        self.out.extend_from_slice(operand);
    }

    fn emit(&mut self, opcode: u32, operands: &[&[u32]]) {
        let length = 1 + operands.iter().map(|operand| operand.len()).sum::<usize>();
        self.out
            .push(instruction(opcode, length).expect("prologue instructions are short"));
        for operand in operands {
            self.out.extend_from_slice(operand);
        }
    }
}

/// The slot and row of an immediately indexed constant-buffer operand.
fn constant_row(operand: &[u32]) -> Option<(u32, u32)> {
    let token = *operand.first()?;
    let extended = (token >> 31) as usize;
    let operand_type = (token >> 12) & 0xff;
    let dimensions = (token >> 20) & 0x3;
    let representations = (token >> 22) & 0x1ff;
    if operand_type != OPERAND_TYPE_CONSTANT_BUFFER || dimensions != 2 || representations != 0 {
        return None;
    }
    match operand.get(1 + extended..3 + extended)? {
        [slot, row] => Some((*slot, *row)),
        _ => None,
    }
}

/// A plain register source: a temporary or an input, one immediate index, a swizzle, no modifier.
struct PlainSource {
    kind: SourceKind,
    register: u32,
    /// The components the swizzle reads into `.x` and `.y`.
    components: [u32; 2],
}

enum SourceKind {
    Temp,
    Input,
}

fn plain_source(operand: &[u32]) -> Option<PlainSource> {
    let [token, register] = operand else {
        return None;
    };
    let kind = match (token >> 12) & 0xff {
        OPERAND_TYPE_TEMP => SourceKind::Temp,
        OPERAND_TYPE_INPUT => SourceKind::Input,
        _ => return None,
    };
    let swizzled = token & 0x3 == 2 && (token >> 2) & 0x3 == SELECTION_SWIZZLE;
    let plain = (token >> 20) & 0x3 == 1 && (token >> 22) & 0x7 == 0;
    (swizzled && plain).then(|| PlainSource {
        kind,
        register: *register,
        components: [(token >> 4) & 0x3, (token >> 6) & 0x3],
    })
}

/// The register of a resource operand.
fn resource_register(operand: &[u32]) -> Option<u32> {
    let [token, index, ..] = operand else {
        return None;
    };
    ((token >> 12) & 0xff == OPERAND_TYPE_RESOURCE && token >> 31 == 0).then_some(*index)
}

/// Write masks.
const XYZ: u32 = 0x7;
const XYZW: u32 = 0xf;

/// `cb13[rEye.y + row].xyzw` as a source: the eye's copy of `row`.
fn eye_row(row: u32, eye: u32) -> [u32; 5] {
    [
        OPERAND_CB_2D_IMM | IDX_IMM32_PLUS_RELATIVE << 25,
        SCREEN_SPACE_REFLECTION_CB_REGISTER,
        row,
        select(OPERAND_TYPE_TEMP, Y),
        eye,
    ]
}
