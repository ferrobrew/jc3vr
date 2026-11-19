//! Run the ambient-occlusion generation, blur, and composite passes over each eye of a double-wide target.
//!
//! The SSAO generation pixel shaders (`ssao` and its NVIDIA twin `ssao_nvidia`) draw one quad over the
//! half-resolution occlusion target. Each pixel reads the packed linear depth (`CS_Z_buffer`) and the
//! normal buffer (`Normal_buffer`) at its screen UV `v1`, reconstructs a view-space position as
//! `z * (uv * basis.xy + basis.zw)` from one row of `cbConstants`, turns the normal into view space with
//! three more rows, and gathers occlusion from taps on a spiral around it:
//!
//! ```text
//! mad r1.xy, v1.xyxx, cb1[2].xyxx, cb1[2].zwzz      // the reconstruction basis
//! mad r3.xyw, cb1[4].xyxz, r3.xxxx, r4.xyxz         // the normal's view rotation, rows 4..6
//! dp2 r3.w, v0.xyxx, l(0.467944, -0.703648, 0, 0)   // the per-pixel spiral rotation
//! mad r7.xy, r8.xxxx, r7.xyxx, v1.xyxx              // a tap: the UV plus an isotropic UV offset
//! sample_l r8.w, r7.xyxx, t0.yzwx, s0, r5.y         // the tap's depth, at a radius-chosen mip
//! ftoi r1.zw, v0.xxxy                               // the pixel's quad parity
//! ```
//!
//! Under a double-wide target one basis and one rotation serve both eyes, the UV offsets reach twice as
//! far across the view horizontally as they do in an eye-sized target, taps near the seam read the
//! other eye's depth, and the right eye's pixel position, and with it its spiral and parity, is
//! offset by the eye's width. [`localize_ambient_occlusion`] makes each half compute what an eye-sized
//! target computes:
//!
//! - The eye is the half `v1.x` falls in. The basis and the three rotation rows are read from a
//!   mod-owned constant buffer, [`AMBIENT_OCCLUSION_CB_REGISTER`], at
//!   [`AMBIENT_OCCLUSION_ROWS_PER_EYE`] rows per eye (the basis, then the rotation rows), indexed by
//!   that eye.
//! - Every tap's horizontal offset from `v1.x` is halved, in place, so the tap lands where the
//!   eye-sized target puts it and the reconstruction that follows reads the same coordinate.
//! - Every depth and normal sample is clamped to the texel centres of the eye's half, which is what
//!   the clamping sampler does at the edges of an eye-sized target.
//! - The pixel position is made eye-local by subtracting the eye's half of the target width, measured
//!   from the screen UV's own derivative, so the rewrite needs no size constant.
//!
//! The pass is found by the names its resource table binds the two textures and the constant array
//! under. A shader without them, or one that touches either texture other than through a sample, is
//! declined.

use std::collections::HashSet;

use crate::{
    container::{Dxbc, DxbcError},
    rewrite::{
        common::{
            CB_ACCESS_DYNAMIC_INDEXED, OPCODE_DCL_CONSTANT_BUFFER, OPCODE_DCL_TEMPS,
            OPERAND_CB_2D_IMM, declared_register, reassemble, signature_register,
        },
        planar::{BINDING_TEXTURE, binding, constant_location},
    },
    tokens::{IDX_IMM32_PLUS_RELATIVE, Instruction, ShaderStage, TokenStream},
};

/// The constant-buffer slot the rewritten shader reads each eye's basis and normal rotation from.
/// Free across the game's pixel shaders.
pub const AMBIENT_OCCLUSION_CB_REGISTER: u32 = 13;
/// The rows each eye occupies in [`AMBIENT_OCCLUSION_CB_REGISTER`]: the reconstruction basis, then
/// the three rows of the normal's view rotation, in the layout the engine stages them in
/// `cbConstants`.
pub const AMBIENT_OCCLUSION_ROWS_PER_EYE: u32 = 4;

/// Rewrite the ambient-occlusion generation pixel shader so each half of a double-wide target is
/// computed for its own eye. Returns [`DxbcError::NoAmbientOcclusion`] for any other shader, or for
/// one whose shape this rewrite does not recognise.
pub fn localize_ambient_occlusion(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let rdef = chunk(&dxbc, blob, b"RDEF").ok_or(DxbcError::NoAmbientOcclusion)?;
    let depth =
        binding(rdef, DEPTH_TEXTURE, BINDING_TEXTURE).ok_or(DxbcError::NoAmbientOcclusion)?;
    let normal =
        binding(rdef, NORMAL_TEXTURE, BINDING_TEXTURE).ok_or(DxbcError::NoAmbientOcclusion)?;
    let constants = constant_location(rdef, CONSTANTS).ok_or(DxbcError::NoAmbientOcclusion)?;
    if (constants.register, constants.component) != (0, 0) {
        return Err(DxbcError::NoAmbientOcclusion);
    }
    let isgn = chunk(&dxbc, blob, b"ISGN").ok_or(DxbcError::MissingInputSignature)?;
    let (position, _) =
        signature_register(isgn, "SV_Position", 0).ok_or(DxbcError::NoAmbientOcclusion)?;
    rewrite_pass(
        &dxbc,
        blob,
        Pass::Generation {
            constants_slot: constants.slot,
            position,
        },
        &[depth, normal],
    )
}

/// Rewrite the ambient-occlusion pass's bilateral blur (`ssao_blur`) or its composite (`ssao_apply`)
/// so each half of a double-wide target reads only its own eye's occlusion. Returns
/// [`DxbcError::NoAmbientOcclusion`] for any other shader.
///
/// Both passes address their source by the screen UV, which is right under a double-wide target --
/// the blur steps one target texel through the screen-size constant, and the composite upsamples by
/// the UV -- except at the seam, where the blur's outer taps and the composite's bilinear footprint
/// reach into the other eye. Each sample of the source is clamped to the texel centres of the eye's
/// half, which is what the clamping sampler does at the edges of an eye-sized target. The blur is the
/// shader binding a texture named `Source` through `SamplerNearest` with a `Constants` array; the
/// composite is the shader binding `Src` and `AOVolumeSrc`, of which only `Src`, the occlusion, is
/// clamped.
pub fn localize_ambient_occlusion_filter(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let rdef = chunk(&dxbc, blob, b"RDEF").ok_or(DxbcError::NoAmbientOcclusion)?;
    let blur = binding(rdef, BLUR_SOURCE, BINDING_TEXTURE).filter(|_| {
        binding(rdef, BLUR_SAMPLER, BINDING_SAMPLER).is_some()
            && constant_location(rdef, CONSTANTS).is_some()
    });
    let composite = binding(rdef, COMPOSITE_SOURCE, BINDING_TEXTURE)
        .filter(|_| binding(rdef, COMPOSITE_VOLUME, BINDING_TEXTURE).is_some());
    let source = blur.or(composite).ok_or(DxbcError::NoAmbientOcclusion)?;
    rewrite_pass(&dxbc, blob, Pass::Filter { source }, &[source])
}

/// The body of a chunk.
fn chunk<'a>(dxbc: &Dxbc, blob: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    dxbc.chunk(tag).map(|c| c.body(blob))
}

/// Which pass a rewrite is localizing, and what that pass alone needs.
#[derive(Clone, Copy)]
enum Pass {
    /// The generation pass: its basis and rotation rows move to the mod-owned buffer, its pixel
    /// position becomes eye-local, and its taps are halved.
    Generation { constants_slot: u32, position: u32 },
    /// The blur or the composite: only the samples of `source` are clamped.
    Filter { source: u32 },
}

/// Rewrite a pass whose samples of `textures` are clamped to the eye's half.
fn rewrite_pass(
    dxbc: &Dxbc,
    blob: &[u8],
    pass: Pass,
    textures: &[u32],
) -> Result<Vec<u8>, DxbcError> {
    let isgn = chunk(dxbc, blob, b"ISGN").ok_or(DxbcError::MissingInputSignature)?;
    let (texcoord, _) =
        signature_register(isgn, "TEXCOORD", 0).ok_or(DxbcError::NoAmbientOcclusion)?;
    let stream =
        TokenStream::new(chunk(dxbc, blob, b"SHEX").ok_or(DxbcError::UnsupportedShaderModel)?)?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoAmbientOcclusion);
    }
    let tokens = stream.tokens();

    let mut temps = None;
    for insn in stream.instructions() {
        let insn = insn?;
        match insn.opcode {
            OPCODE_DCL_TEMPS => temps = tokens.get(insn.start + 1).copied(),
            OPCODE_DCL_CONSTANT_BUFFER
                if declared_register(tokens, &insn)? == AMBIENT_OCCLUSION_CB_REGISTER =>
            {
                return Err(DxbcError::Cb13AlreadyDeclared);
            }
            _ => {}
        }
    }
    let first = temps.ok_or(DxbcError::NoAmbientOcclusion)?;
    let registers = Registers {
        eye: first,
        bounds: first + 1,
        position: first + 2,
        coordinate: first + 3,
        scratch: first + 4,
        texcoord,
    };

    let mut rewriter = Rewriter {
        tokens,
        registers,
        pass,
        out: tokens[..2].to_vec(),
        scaled: HashSet::new(),
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
        rewriter.rewrite(&insn, textures)?;
    }
    let counts = rewriter.counts;
    let recognised = match pass {
        Pass::Generation { .. } => {
            counts.taps > 0
                && counts.basis > 0
                && counts.rotation == ROTATION_ROWS.len()
                && counts.position > 0
        }
        Pass::Filter { .. } => counts.clamps > 0,
    };
    if !recognised {
        return Err(DxbcError::NoAmbientOcclusion);
    }
    let mut out = rewriter.out;
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        dxbc,
        blob,
        isgn.to_vec(),
        chunk(dxbc, blob, b"OSGN")
            .ok_or(DxbcError::MissingOutputSignature)?
            .to_vec(),
        out.iter().flat_map(|v| v.to_le_bytes()).collect(),
        false,
    )
}

/// The names the generation pass binds its inputs under.
const DEPTH_TEXTURE: &[u8] = b"CS_Z_buffer";
const NORMAL_TEXTURE: &[u8] = b"Normal_buffer";
const CONSTANTS: &[u8] = b"Constants";

/// The names the blur and the composite bind their inputs under.
const BLUR_SOURCE: &[u8] = b"Source";
const BLUR_SAMPLER: &[u8] = b"SamplerNearest";
const COMPOSITE_SOURCE: &[u8] = b"Src";
const COMPOSITE_VOLUME: &[u8] = b"AOVolumeSrc";

/// The resource table's binding type for a sampler.
const BINDING_SAMPLER: u32 = 3;

/// The `cbConstants` row holding the reconstruction basis, and the rows holding the normal's view
/// rotation, in the order they take after the basis in each eye's block.
const BASIS_ROW: u32 = 2;
const ROTATION_ROWS: [u32; 3] = [4, 5, 6];

/// The temporaries the rewrite adds after the shader's own.
const ADDED_TEMPS: u32 = 5;

/// The fraction of a target texel the sample clamp keeps inside each eye's half. Any value below one
/// half lands a point sample on the eye's edge texel; the target is half the resolution of the depth
/// texture's top mip, so this is a quarter of that mip's texel.
const CLAMP_INSET: f32 = 0.125;

/// The register numbers the rewrite reads and writes.
#[derive(Clone, Copy)]
struct Registers {
    /// `.x` all ones in the right eye's half, `.y` the eye's first row in the constant buffer, `.z`
    /// the eye's first UV (`0` or `0.5`).
    eye: u32,
    /// `.x` and `.y` the eye's sample clamp; `.z` scratch for the eye's half width.
    bounds: u32,
    /// The eye-local pixel position.
    position: u32,
    /// A sample's clamped coordinate.
    coordinate: u32,
    /// Scratch for a tap's offset.
    scratch: u32,
    /// The screen UV input.
    texcoord: u32,
}

#[derive(Default, Clone, Copy)]
struct Counts {
    clamps: usize,
    taps: usize,
    basis: usize,
    rotation: usize,
    position: usize,
}

struct Rewriter<'a> {
    tokens: &'a [u32],
    registers: Registers,
    pass: Pass,
    out: Vec<u32>,
    /// Tap coordinate components already halved and not written since.
    scaled: HashSet<(u32, u32)>,
    counts: Counts,
}

impl Rewriter<'_> {
    fn emit_prologue(&mut self) {
        let Registers {
            eye,
            bounds,
            position,
            texcoord,
            ..
        } = self.registers;
        let half = 0.5f32.to_bits();
        let pixel = match self.pass {
            Pass::Generation { position, .. } => position,
            Pass::Filter { source } => {
                self.emit(OPCODE_GE, &[&dst(eye, X), &input(texcoord, X), &imm(half)]);
                self.emit(OPCODE_AND, &[&dst(eye, Z), &src(eye, X), &imm(half)]);
                self.emit(
                    OPCODE_RESINFO,
                    &[&dst(bounds, W), &imm(0), &[RESOURCE_XXXX, source]],
                );
                self.emit(OPCODE_DIV, &[&dst(bounds, W), &imm(half), &src(bounds, W)]);
                self.emit_bounds();
                return;
            }
        };
        self.out.extend([
            (4 << 24) | CB_ACCESS_DYNAMIC_INDEXED | OPCODE_DCL_CONSTANT_BUFFER,
            OPERAND_CB_2D_IMM,
            AMBIENT_OCCLUSION_CB_REGISTER,
            2 * AMBIENT_OCCLUSION_ROWS_PER_EYE,
        ]);
        self.emit(OPCODE_GE, &[&dst(eye, X), &input(texcoord, X), &imm(half)]);
        self.emit(
            OPCODE_AND,
            &[
                &dst(eye, Y),
                &src(eye, X),
                &imm(AMBIENT_OCCLUSION_ROWS_PER_EYE),
            ],
        );
        self.emit(OPCODE_AND, &[&dst(eye, Z), &src(eye, X), &imm(half)]);
        self.emit(OPCODE_DERIV_RTX, &[&dst(bounds, Z), &input(texcoord, X)]);
        self.emit(
            OPCODE_MUL,
            &[
                &dst(bounds, W),
                &src_mod(bounds, Z, MODIFIER_ABS),
                &imm(CLAMP_INSET.to_bits()),
            ],
        );
        self.emit_bounds();
        self.emit(
            OPCODE_DIV,
            &[
                &dst(bounds, Z),
                &imm(half),
                &src_mod(bounds, Z, MODIFIER_ABS),
            ],
        );
        self.emit(OPCODE_ROUND_NE, &[&dst(bounds, Z), &src(bounds, Z)]);
        self.emit(
            OPCODE_AND,
            &[&dst(bounds, Z), &src(bounds, Z), &src(eye, X)],
        );
        self.emit(
            OPCODE_ADD,
            &[
                &dst(position, X),
                &input(pixel, X),
                &src_mod(bounds, Z, MODIFIER_NEG),
            ],
        );
        self.emit(OPCODE_MOV, &[&dst(position, Y), &input(pixel, Y)]);
    }

    /// The eye's sample clamp, `[first + inset, first + 0.5 - inset]`, from the eye's first UV in
    /// `eye.z` and the inset in `bounds.w`.
    fn emit_bounds(&mut self) {
        let Registers { eye, bounds, .. } = self.registers;
        self.emit(
            OPCODE_ADD,
            &[&dst(bounds, X), &src(eye, Z), &src(bounds, W)],
        );
        self.emit(
            OPCODE_ADD,
            &[&dst(bounds, Y), &src(eye, Z), &imm(0.5f32.to_bits())],
        );
        self.emit(
            OPCODE_ADD,
            &[
                &dst(bounds, Y),
                &src(bounds, Y),
                &src_mod(bounds, W, MODIFIER_NEG),
            ],
        );
    }

    fn rewrite(&mut self, insn: &Instruction<'_>, textures: &[u32]) -> Result<(), DxbcError> {
        let tokens = self.tokens;
        let offsets: Vec<usize> = insn
            .operands()
            .map(|operand| operand.map(|operand| operand.token_offset))
            .collect::<Result<_, _>>()?;
        let span = |index: usize| {
            let end = offsets.get(index + 1).copied().unwrap_or(insn.end);
            offsets[index]..end
        };
        let reads_texture = (0..offsets.len()).any(|index| {
            resource_register(&tokens[span(index)]).is_some_and(|r| textures.contains(&r))
        });
        let mut coordinate = None;
        if reads_texture {
            if !matches!(insn.opcode, OPCODE_SAMPLE | OPCODE_SAMPLE_L) || offsets.len() < 4 {
                return Err(DxbcError::NoAmbientOcclusion);
            }
            coordinate = Some(self.emit_clamped_coordinate(&tokens[span(1)])?);
        }

        let start = self.out.len();
        self.out
            .extend_from_slice(&tokens[insn.start..insn.operands_start()]);
        for index in 0..offsets.len() {
            let operand = &tokens[span(index)];
            if index == 1
                && let Some(clamped) = coordinate
            {
                self.out.extend(clamped);
                continue;
            }
            self.rewrite_operand(operand);
        }
        let length = self.out.len() - start;
        self.out[start] = (self.out[start] & !LENGTH_MASK) | instruction(0, length)?;

        if !offsets.is_empty()
            && let Some((register, mask)) = temp_destination(&tokens[span(0)])
        {
            self.scaled
                .retain(|&(scaled, component)| scaled != register || mask & (1 << component) == 0);
        }
        Ok(())
    }

    /// Emit the halving of a tap's offset and the clamp to the eye's half, returning the coordinate
    /// operand the sample reads instead.
    fn emit_clamped_coordinate(&mut self, operand: &[u32]) -> Result<[u32; 2], DxbcError> {
        let Registers {
            bounds,
            coordinate,
            scratch,
            texcoord,
            ..
        } = self.registers;
        let source = plain_source(operand).ok_or(DxbcError::NoAmbientOcclusion)?;
        let [u, v] = source.components;
        let read = |component| match source.kind {
            SourceKind::Temp => src(source.register, component),
            SourceKind::Input => input(source.register, component),
        };
        let generation = matches!(self.pass, Pass::Generation { .. });
        match source.kind {
            SourceKind::Temp if !generation => {}
            SourceKind::Temp => {
                if self.scaled.insert((source.register, u)) {
                    self.emit(
                        OPCODE_ADD,
                        &[
                            &dst(scratch, X),
                            &read(u),
                            &input_mod(texcoord, X, MODIFIER_NEG),
                        ],
                    );
                    self.emit(
                        OPCODE_MAD,
                        &[
                            &dst(source.register, u),
                            &src(scratch, X),
                            &imm(0.5f32.to_bits()),
                            &input(texcoord, X),
                        ],
                    );
                    self.counts.taps += 1;
                }
            }
            SourceKind::Input if source.register == texcoord => {}
            SourceKind::Input => return Err(DxbcError::NoAmbientOcclusion),
        }
        self.emit(
            OPCODE_MAX,
            &[&dst(coordinate, X), &read(u), &src(bounds, X)],
        );
        self.emit(
            OPCODE_MIN,
            &[&dst(coordinate, X), &src(coordinate, X), &src(bounds, Y)],
        );
        self.emit(OPCODE_MOV, &[&dst(coordinate, Y), &read(v)]);
        self.counts.clamps += 1;
        Ok([TEMP_XYXX_SOURCE, coordinate])
    }

    /// Copy an operand, redirecting the pixel position to its eye-local temporary and the basis and
    /// rotation rows to the eye's block of the mod-owned constant buffer.
    fn rewrite_operand(&mut self, operand: &[u32]) {
        let Pass::Generation {
            constants_slot,
            position,
        } = self.pass
        else {
            self.out.extend_from_slice(operand);
            return;
        };
        let token = operand[0];
        let extended = (token >> 31) as usize;
        let operand_type = (token >> 12) & 0xff;
        let dimensions = (token >> 20) & 0x3;
        let representations = (token >> 22) & 0x1ff;
        let indices = &operand[1 + extended..];
        if operand_type == OPERAND_TYPE_INPUT
            && dimensions == 1
            && representations == 0
            && indices == [position]
        {
            self.counts.position += 1;
            self.out.push(token & !(0xff << 12));
            self.out.extend_from_slice(&operand[1..1 + extended]);
            self.out.push(self.registers.position);
            return;
        }
        if operand_type == OPERAND_TYPE_CONSTANT_BUFFER
            && dimensions == 2
            && representations == 0
            && let [slot, row] = indices
            && *slot == constants_slot
        {
            let eye_row = if *row == BASIS_ROW {
                self.counts.basis += 1;
                Some(0)
            } else {
                ROTATION_ROWS.iter().position(|r| r == row).map(|index| {
                    self.counts.rotation += 1;
                    index as u32 + 1
                })
            };
            if let Some(eye_row) = eye_row {
                self.out.push(token | IDX_IMM32_PLUS_RELATIVE << 25);
                self.out.extend_from_slice(&operand[1..1 + extended]);
                self.out.extend([
                    AMBIENT_OCCLUSION_CB_REGISTER,
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

/// The register and write mask of a plain temporary destination.
fn temp_destination(operand: &[u32]) -> Option<(u32, u32)> {
    let [token, register] = operand else {
        return None;
    };
    ((token >> 12) & 0xff == OPERAND_TYPE_TEMP && (token >> 2) & 0x3 == SELECTION_MASK)
        .then_some((*register, (token >> 4) & 0xf))
}

const X: u32 = 0;
const Y: u32 = 1;
const Z: u32 = 2;
const W: u32 = 3;

const OPCODE_ADD: u32 = 0x00;
const OPCODE_AND: u32 = 0x01;
const OPCODE_DERIV_RTX: u32 = 0x0b;
const OPCODE_DIV: u32 = 0x0e;
const OPCODE_GE: u32 = 0x1d;
const OPCODE_MAD: u32 = 0x32;
const OPCODE_MIN: u32 = 0x33;
const OPCODE_MAX: u32 = 0x34;
const OPCODE_MOV: u32 = 0x36;
const OPCODE_MUL: u32 = 0x38;
const OPCODE_RESINFO: u32 = 0x3d;
const OPCODE_ROUND_NE: u32 = 0x40;
const OPCODE_SAMPLE: u32 = 0x45;
const OPCODE_SAMPLE_L: u32 = 0x48;

/// The instruction-length field of an opcode token.
const LENGTH_MASK: u32 = 0x7f00_0000;

const OPERAND_TYPE_TEMP: u32 = 0;
const OPERAND_TYPE_INPUT: u32 = 1;
const OPERAND_TYPE_RESOURCE: u32 = 7;
const OPERAND_TYPE_CONSTANT_BUFFER: u32 = 8;

const SELECTION_MASK: u32 = 0;
const SELECTION_SWIZZLE: u32 = 1;
const SELECTION_SELECT_1: u32 = 2;

/// `tN.xxxx` as a resource operand, followed by the register index: a `resinfo` through it writes the
/// texture's width to every component of its destination.
const RESOURCE_XXXX: u32 = 2 | SELECTION_SWIZZLE << 2 | OPERAND_TYPE_RESOURCE << 12 | 1 << 20;

/// `rN.xyxx` as a source, followed by the register index.
const TEMP_XYXX_SOURCE: u32 = 0x0010_0046;

/// The extended operand tokens for a negated and an absolute source.
const MODIFIER_NEG: u32 = 0x41;
const MODIFIER_ABS: u32 = 0x81;

/// The opcode token for `opcode` with an instruction of `length` tokens.
fn instruction(opcode: u32, length: usize) -> Result<u32, DxbcError> {
    let length = u32::try_from(length)
        .ok()
        .filter(|length| *length <= LENGTH_MASK >> 24)
        .ok_or(DxbcError::InstructionTooLong)?;
    Ok(opcode | length << 24)
}

/// A four-component, one-dimensional operand token of `operand_type` selecting `component`.
fn select(operand_type: u32, component: u32) -> u32 {
    2 | SELECTION_SELECT_1 << 2 | component << 4 | operand_type << 12 | 1 << 20
}

/// `rN.c` as a destination.
fn dst(register: u32, component: u32) -> [u32; 2] {
    [
        2 | SELECTION_MASK << 2 | (1 << component) << 4 | OPERAND_TYPE_TEMP << 12 | 1 << 20,
        register,
    ]
}

/// `rN.c` as a source.
fn src(register: u32, component: u32) -> [u32; 2] {
    [select(OPERAND_TYPE_TEMP, component), register]
}

/// `rN.c` as a source with an operand modifier.
fn src_mod(register: u32, component: u32, modifier: u32) -> [u32; 3] {
    [
        select(OPERAND_TYPE_TEMP, component) | 1 << 31,
        modifier,
        register,
    ]
}

/// `vN.c` as a source.
fn input(register: u32, component: u32) -> [u32; 2] {
    [select(OPERAND_TYPE_INPUT, component), register]
}

/// `vN.c` as a source with an operand modifier.
fn input_mod(register: u32, component: u32, modifier: u32) -> [u32; 3] {
    [
        select(OPERAND_TYPE_INPUT, component) | 1 << 31,
        modifier,
        register,
    ]
}

/// A one-component immediate holding `bits`.
fn imm(bits: u32) -> [u32; 2] {
    [0x4001, bits]
}
