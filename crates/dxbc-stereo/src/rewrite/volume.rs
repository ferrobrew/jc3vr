//! Shade the ambient-occlusion volumes over each eye's half of a double-wide target from one draw.
//!
//! An ambient-occlusion volume (`aobox`) is a box rasterized over the scene. Its pixel shader takes
//! the screen UV its vertex shader interpolates, samples the scene depth and the reflective water
//! planes' depth there, and maps each `(u, v, depth)` into the box's own unit space through one
//! matrix, `ScreenToLocal`, which the block builds on the CPU from the render context's camera and the
//! box's transform; the occlusion falls off with the distance of the mapped point from the box centre:
//!
//! ```text
//! div r0.xy, v1.xyxx, v1.zzzz                       // the screen UV, projectively
//! mul r1.xyzw, r0.yyyy, cb1[2].xyzw                 // u and v through ScreenToLocal
//! mad r1.xyzw, r0.xxxx, cb1[1].xyzw, r1.xyzw
//! sample_l r0.z, r0.xyxx, t1.yzxw, s1, l(0)         // the scene depth
//! mad r0.xyzw, r0.zzzz, cb1[3].xyzw, r1.xyzw        // the depth through ScreenToLocal
//! add r0.xyzw, r0.xyzw, cb1[4].xyzw                 // then the matrix's last row, and a divide
//! ```
//!
//! Under the collapse one draw covers both eyes: the vertex shader is reprojected, so each eye's half
//! rasterizes the box from that eye, but the matrix is the centred camera's, the depth texture spans
//! both eyes, and the interpolated UV is the centred camera's too. [`localize_ambient_occlusion_volume`]
//! computes each pixel's mapping from its eye instead, entirely from its pixel position:
//!
//! - The eye is the half `SV_Position.x` falls in, against a boundary in `.x` of
//!   [`AMBIENT_OCCLUSION_VOLUME_BOUNDARY_ROW`] of a mod-owned constant buffer at
//!   [`AMBIENT_OCCLUSION_VOLUME_CB_REGISTER`].
//! - Both depths are sampled at `SV_Position.xy * scale + offset`, from `.xy` and `.zw` of
//!   [`AMBIENT_OCCLUSION_VOLUME_DEPTH_UV_ROW`]: the double-wide UV of the pixel.
//! - Every product of a sampled depth with the matrix's depth row becomes the product of the whole
//!   matrix with `P_eye · (x, y, depth, 1)`, divided through by its `w`, where `P_eye` is the four
//!   rows of the eye's block ([`AMBIENT_OCCLUSION_VOLUME_ROWS_PER_EYE`]): the caller's map from the
//!   eye's pixel and depth to the centred camera's UV and depth, which is what the centred matrix
//!   reads. The matrix's last row, added after, then stands for the `w` of one the divide leaves.
//!
//! The UV products the shader computes once and adds to both depth products are then unused; they
//! stay in place, dead. The pass is found by the names its resource table binds the two depth
//! textures and the matrix under; a shader without them, or one that reads either depth texture other
//! than through a sample into one component, or reads the matrix's depth row other than in such a
//! product, is declined.

use std::collections::HashSet;

use crate::{
    container::{Dxbc, DxbcError},
    isa::{
        IDX_IMM32_PLUS_RELATIVE, INTERPOLATION_LINEAR_NOPERSPECTIVE, LENGTH_MASK, OPCODE_AND,
        OPCODE_DCL_CONSTANT_BUFFER, OPCODE_DCL_INPUT_PS_SIV, OPCODE_DCL_TEMPS, OPCODE_DIV,
        OPCODE_DP4, OPCODE_GE, OPCODE_MAD, OPCODE_MOV, OPCODE_MUL, OPCODE_SAMPLE, OPCODE_SAMPLE_L,
        OPERAND_CB_2D_SELECT_X, OPERAND_IMM32_SCALAR, OPERAND_INPUT_MASK_XY,
        OPERAND_INPUT_SELECT_X, OPERAND_INPUT_SWIZZLE_XYXX, OPERAND_TEMP_MASK_X,
        OPERAND_TEMP_MASK_XY, OPERAND_TEMP_SELECT_X, OPERAND_TEMP_SWIZZLE_XYXX,
        OPERAND_TEMP_SWIZZLE_XYZW, OPERAND_TYPE_CONSTANT_BUFFER, OPERAND_TYPE_RESOURCE,
        OPERAND_TYPE_TEMP, SB_NAME_POSITION, SELECTION_MASK, SELECTION_SELECT_1, SELECTION_SWIZZLE,
        W, X, Y, Z, instruction,
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

/// The pixel-stage constant-buffer slot the rewritten shader reads each eye's mapping, the depth UV,
/// and the eye boundary from. No game pixel shader declares it, and the passes that bind their own
/// buffer there run after the volumes.
pub const AMBIENT_OCCLUSION_VOLUME_CB_REGISTER: u32 = 13;
/// The rows each eye's block occupies: the four rows of the map from the eye's `(x, y, depth, 1)` to
/// the centred camera's homogeneous `(u, v, depth, w)`, each dotted with that vector.
pub const AMBIENT_OCCLUSION_VOLUME_ROWS_PER_EYE: u32 = 4;
/// The row after both eyes' blocks whose `.xy` scales and `.zw` offsets the pixel position into the
/// UV both depth textures are sampled at.
pub const AMBIENT_OCCLUSION_VOLUME_DEPTH_UV_ROW: u32 = 2 * AMBIENT_OCCLUSION_VOLUME_ROWS_PER_EYE;
/// The row whose `.x` holds the eye boundary: the first pixel coordinate of the right eye's half.
pub const AMBIENT_OCCLUSION_VOLUME_BOUNDARY_ROW: u32 = AMBIENT_OCCLUSION_VOLUME_DEPTH_UV_ROW + 1;
/// The size of [`AMBIENT_OCCLUSION_VOLUME_CB_REGISTER`] in float4 rows.
pub const AMBIENT_OCCLUSION_VOLUME_CB_ROWS: u32 = AMBIENT_OCCLUSION_VOLUME_BOUNDARY_ROW + 1;

/// Rewrite the ambient-occlusion volume pixel shader so each half of a double-wide target maps its
/// pixels through its own eye (see the module docs). Returns
/// [`DxbcError::NoAmbientOcclusionVolume`] for any other shader, or for one whose shape this rewrite
/// does not recognise.
pub fn localize_ambient_occlusion_volume(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let rdef = dxbc
        .chunk(b"RDEF")
        .ok_or(DxbcError::NoAmbientOcclusionVolume)?
        .body(blob);
    let textures = [DEPTH_TEXTURE, WATER_DEPTH_TEXTURE].map(|name| {
        binding(rdef, name, BINDING_TEXTURE).ok_or(DxbcError::NoAmbientOcclusionVolume)
    });
    let textures = [textures[0]?, textures[1]?];
    let matrix =
        constant_location(rdef, SCREEN_TO_LOCAL).ok_or(DxbcError::NoAmbientOcclusionVolume)?;
    if matrix.component != 0 {
        return Err(DxbcError::NoAmbientOcclusionVolume);
    }
    let isgn = dxbc
        .chunk(b"ISGN")
        .ok_or(DxbcError::MissingInputSignature)?
        .body(blob);
    let (position, _) =
        signature_register(isgn, "SV_Position", 0).ok_or(DxbcError::NoAmbientOcclusionVolume)?;
    let stream = TokenStream::new(
        dxbc.chunk(b"SHEX")
            .ok_or(DxbcError::UnsupportedShaderModel)?
            .body(blob),
    )?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoAmbientOcclusionVolume);
    }
    let tokens = stream.tokens();
    let mut temps = None;
    let mut position_declared = false;
    for insn in stream.instructions() {
        let insn = insn?;
        match insn.opcode {
            OPCODE_DCL_TEMPS => temps = tokens.get(insn.start + 1).copied(),
            OPCODE_DCL_INPUT_PS_SIV if tokens.get(insn.end - 1) == Some(&SB_NAME_POSITION) => {
                let operand = *tokens
                    .get(insn.start + 1)
                    .ok_or(DxbcError::UnexpectedEndOfTokens)?;
                if (operand >> 4) & 0b0011 != 0b0011 {
                    return Err(DxbcError::NoAmbientOcclusionVolume);
                }
                position_declared = true;
            }
            OPCODE_DCL_CONSTANT_BUFFER
                if declared_register(tokens, &insn)? == AMBIENT_OCCLUSION_VOLUME_CB_REGISTER =>
            {
                return Err(DxbcError::NoAmbientOcclusionVolume);
            }
            _ => {}
        }
    }
    let first = temps.ok_or(DxbcError::NoAmbientOcclusionVolume)?;
    let mut rewriter = Rewriter {
        tokens,
        textures,
        matrix: Matrix {
            slot: matrix.slot,
            first_row: matrix.register,
        },
        registers: Registers {
            eye: first,
            uv: first + 1,
            point: first + 2,
            mapped: first + 3,
            position,
        },
        out: tokens[..2].to_vec(),
        position_declared,
        depths: HashSet::new(),
        samples: 0,
        products: 0,
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
    if rewriter.samples == 0 || rewriter.products != rewriter.samples {
        return Err(DxbcError::NoAmbientOcclusionVolume);
    }
    let mut out = rewriter.out;
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        blob,
        mark_position_read(isgn)?,
        dxbc.chunk(b"OSGN")
            .ok_or(DxbcError::MissingOutputSignature)?
            .body(blob)
            .to_vec(),
        out.iter().flat_map(|v| v.to_le_bytes()).collect(),
        false,
    )
}

/// The names the volume pass binds its inputs under.
const DEPTH_TEXTURE: &[u8] = b"DepthMap";
const WATER_DEPTH_TEXTURE: &[u8] = b"WaterPlaneDepthMap";
const SCREEN_TO_LOCAL: &[u8] = b"ScreenToLocal";

/// The temporaries the rewrite adds after the shader's own.
const ADDED_TEMPS: u32 = 4;

/// Where the matrix lives: its buffer's slot and its first register, the row `u` multiplies.
#[derive(Clone, Copy)]
struct Matrix {
    slot: u32,
    first_row: u32,
}

impl Matrix {
    /// The row a sampled depth multiplies.
    fn depth_row(self) -> u32 {
        self.first_row + 2
    }
}

/// The register numbers the rewrite reads and writes.
#[derive(Clone, Copy)]
struct Registers {
    /// `.x` the eye's first row in the mod-owned buffer.
    eye: u32,
    /// `.xy` the double-wide UV both depths are sampled at.
    uv: u32,
    /// `(x, y, depth, 1)`: the pixel position and the depth being mapped.
    point: u32,
    /// The point mapped into the centred camera's `(u, v, depth)`.
    mapped: u32,
    /// The `SV_Position` input.
    position: u32,
}

struct Rewriter<'a> {
    tokens: &'a [u32],
    /// The registers of the scene depth and the water planes' depth textures.
    textures: [u32; 2],
    matrix: Matrix,
    registers: Registers,
    out: Vec<u32>,
    /// Whether the shader already declares the `SV_Position` input it reads its eye from.
    position_declared: bool,
    /// Temporary components holding a sampled depth and not written since.
    depths: HashSet<(u32, u32)>,
    samples: usize,
    products: usize,
}

impl Rewriter<'_> {
    fn emit_prologue(&mut self) {
        let Registers {
            eye,
            uv,
            point,
            position,
            ..
        } = self.registers;
        self.out.extend([
            (4 << 24) | CB_ACCESS_DYNAMIC_INDEXED | OPCODE_DCL_CONSTANT_BUFFER,
            OPERAND_CB_2D_IMM,
            AMBIENT_OCCLUSION_VOLUME_CB_REGISTER,
            AMBIENT_OCCLUSION_VOLUME_CB_ROWS,
        ]);
        if !self.position_declared {
            self.out.extend([
                (4 << 24) | (INTERPOLATION_LINEAR_NOPERSPECTIVE << 11) | OPCODE_DCL_INPUT_PS_SIV,
                OPERAND_INPUT_MASK_XY,
                position,
                SB_NAME_POSITION,
            ]);
        }
        let boundary = [
            OPERAND_CB_2D_SELECT_X,
            AMBIENT_OCCLUSION_VOLUME_CB_REGISTER,
            AMBIENT_OCCLUSION_VOLUME_BOUNDARY_ROW,
        ];
        emit(
            &mut self.out,
            OPCODE_GE,
            &[
                &[OPERAND_TEMP_MASK_X, eye],
                &[OPERAND_INPUT_SELECT_X, position],
                &boundary,
            ],
        );
        emit(
            &mut self.out,
            OPCODE_AND,
            &[
                &[OPERAND_TEMP_MASK_X, eye],
                &[OPERAND_TEMP_SELECT_X, eye],
                &[OPERAND_IMM32_SCALAR, AMBIENT_OCCLUSION_VOLUME_ROWS_PER_EYE],
            ],
        );
        emit(
            &mut self.out,
            OPCODE_MAD,
            &[
                &[OPERAND_TEMP_MASK_XY, uv],
                &[OPERAND_INPUT_SWIZZLE_XYXX, position],
                &[
                    OPERAND_CB_2D_XYXX,
                    AMBIENT_OCCLUSION_VOLUME_CB_REGISTER,
                    AMBIENT_OCCLUSION_VOLUME_DEPTH_UV_ROW,
                ],
                &[
                    OPERAND_CB_2D_ZWZZ,
                    AMBIENT_OCCLUSION_VOLUME_CB_REGISTER,
                    AMBIENT_OCCLUSION_VOLUME_DEPTH_UV_ROW,
                ],
            ],
        );
        emit(
            &mut self.out,
            OPCODE_MOV,
            &[
                &[OPERAND_TEMP_MASK_XY, point],
                &[OPERAND_INPUT_SWIZZLE_XYXX, position],
            ],
        );
        emit(
            &mut self.out,
            OPCODE_MOV,
            &[
                &[temp_mask(1 << W), point],
                &[OPERAND_IMM32_SCALAR, 1.0f32.to_bits()],
            ],
        );
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
        let reads_depth = (0..offsets.len()).any(|index| {
            resource_register(&tokens[span(index)]).is_some_and(|r| self.textures.contains(&r))
        });
        let destination = offsets
            .first()
            .and_then(|_| temp_destination(&tokens[span(0)]));
        let saturate = tokens[insn.start] & SATURATE != 0;
        let result = self.rewrite_body(insn, &offsets, destination, reads_depth, saturate);
        if let Some((register, mask)) = destination {
            self.depths
                .retain(|&(depth, component)| depth != register || mask & (1 << component) == 0);
            if reads_depth {
                self.depths.insert((register, mask.trailing_zeros()));
            }
        }
        result
    }

    fn rewrite_body(
        &mut self,
        insn: &Instruction<'_>,
        offsets: &[usize],
        destination: Option<(u32, u32)>,
        reads_depth: bool,
        saturate: bool,
    ) -> Result<(), DxbcError> {
        let tokens = self.tokens;
        let span = |index: usize| {
            let end = offsets.get(index + 1).copied().unwrap_or(insn.end);
            offsets[index]..end
        };
        if reads_depth {
            if !matches!(insn.opcode, OPCODE_SAMPLE | OPCODE_SAMPLE_L) || offsets.len() < 4 {
                return Err(DxbcError::NoAmbientOcclusionVolume);
            }
            let (_, mask) = destination.ok_or(DxbcError::NoAmbientOcclusionVolume)?;
            if mask.count_ones() != 1 || saturate {
                return Err(DxbcError::NoAmbientOcclusionVolume);
            }
            let start = self.out.len();
            self.out
                .extend_from_slice(&tokens[insn.start..insn.operands_start()]);
            for index in 0..offsets.len() {
                if index == 1 {
                    self.out
                        .extend([OPERAND_TEMP_SWIZZLE_XYXX, self.registers.uv]);
                } else {
                    self.out.extend_from_slice(&tokens[span(index)]);
                }
            }
            let length = self.out.len() - start;
            self.out[start] = (self.out[start] & !LENGTH_MASK) | instruction(0, length)?;
            self.samples += 1;
            return Ok(());
        }
        let reads_depth_row =
            (0..offsets.len()).any(|index| self.is_depth_row(&tokens[span(index)]));
        if !reads_depth_row {
            self.out.extend_from_slice(&tokens[insn.start..insn.end]);
            return Ok(());
        }
        // The only read of the depth row this rewrite knows is `mad dst, depth.cccc, row, base`.
        let [destination, scale, row, _base] = offsets[..] else {
            return Err(DxbcError::NoAmbientOcclusionVolume);
        };
        let component = replicated_temp(&tokens[scale..row])
            .filter(|key| self.depths.contains(key))
            .map(|(_, component)| component);
        let (Some(component), OPCODE_MAD, false) = (component, insn.opcode, saturate) else {
            return Err(DxbcError::NoAmbientOcclusionVolume);
        };
        let (depth_register, _) =
            replicated_temp(&tokens[scale..row]).ok_or(DxbcError::NoAmbientOcclusionVolume)?;
        let (dst_register, dst_mask) = temp_destination(&tokens[destination..scale])
            .ok_or(DxbcError::NoAmbientOcclusionVolume)?;
        let row_token = tokens[row];
        if row_token >> 31 != 0 {
            return Err(DxbcError::NoAmbientOcclusionVolume);
        }
        self.emit_product(
            depth_register,
            component,
            (dst_register, dst_mask),
            row_token,
        );
        self.products += 1;
        Ok(())
    }

    /// Whether an operand reads the matrix's depth row.
    fn is_depth_row(&self, operand: &[u32]) -> bool {
        let [token, slot, row, ..] = operand else {
            return false;
        };
        (token >> 12) & 0xff == OPERAND_TYPE_CONSTANT_BUFFER
            && (token >> 20) & 0x3 == 2
            && *slot == self.matrix.slot
            && (*row == self.matrix.depth_row() || (token >> 25) & 0x7 != 0)
    }

    /// Emit the product of the whole matrix with the eye's mapping of `(x, y, depth, 1)`, into the
    /// original product's destination. `row_token` is the original read of the depth row, whose
    /// swizzle the three row reads keep.
    fn emit_product(
        &mut self,
        depth_register: u32,
        component: u32,
        (dst_register, dst_mask): (u32, u32),
        row_token: u32,
    ) {
        let Registers {
            eye, point, mapped, ..
        } = self.registers;
        emit(
            &mut self.out,
            OPCODE_MOV,
            &[
                &[temp_mask(1 << Z), point],
                &[temp_select(component), depth_register],
            ],
        );
        for (row, lane) in [X, Y, Z, W].into_iter().enumerate() {
            emit(
                &mut self.out,
                OPCODE_DP4,
                &[
                    &[temp_mask(1 << lane), mapped],
                    &[
                        OPERAND_CB_2D_IMM | IDX_IMM32_PLUS_RELATIVE << 25,
                        AMBIENT_OCCLUSION_VOLUME_CB_REGISTER,
                        row as u32,
                        OPERAND_TEMP_SELECT_X,
                        eye,
                    ],
                    &[OPERAND_TEMP_SWIZZLE_XYZW, point],
                ],
            );
        }
        emit(
            &mut self.out,
            OPCODE_DIV,
            &[
                &[temp_mask(0b0111), mapped],
                &[OPERAND_TEMP_SWIZZLE_XYZW, mapped],
                &[temp_swizzle(W * 0x55), mapped],
            ],
        );
        let destination = [temp_mask(dst_mask), dst_register];
        let accumulated = [OPERAND_TEMP_SWIZZLE_XYZW, dst_register];
        let matrix_row = |row: u32| [row_token, self.matrix.slot, row];
        let first = self.matrix.first_row;
        emit(
            &mut self.out,
            OPCODE_MUL,
            &[
                &destination,
                &[temp_swizzle(Y * 0x55), mapped],
                &matrix_row(first + 1),
            ],
        );
        emit(
            &mut self.out,
            OPCODE_MAD,
            &[
                &destination,
                &[temp_swizzle(X * 0x55), mapped],
                &matrix_row(first),
                &accumulated,
            ],
        );
        emit(
            &mut self.out,
            OPCODE_MAD,
            &[
                &destination,
                &[temp_swizzle(Z * 0x55), mapped],
                &matrix_row(first + 2),
                &accumulated,
            ],
        );
    }
}

/// Append one instruction of `opcode` with `operands`.
fn emit(out: &mut Vec<u32>, opcode: u32, operands: &[&[u32]]) {
    let length = 1 + operands.iter().map(|operand| operand.len()).sum::<usize>();
    out.push(instruction(opcode, length).expect("injected instructions are short"));
    for operand in operands {
        out.extend_from_slice(operand);
    }
}

/// The input signature with the `SV_Position` element marked as read through `.xy`.
fn mark_position_read(isgn: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let word = |offset: usize| {
        isgn.get(offset..offset + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")) as usize)
    };
    let (count, table) = (
        word(0).ok_or(DxbcError::MalformedSignature)?,
        word(4).ok_or(DxbcError::MalformedSignature)?,
    );
    let record = (0..count)
        .map(|index| table + index * SIGNATURE_ELEMENT_LEN)
        .find(|&record| word(record + 8) == Some(SIGNATURE_SYSVALUE_POSITION))
        .ok_or(DxbcError::NoAmbientOcclusionVolume)?;
    let mut out = isgn.to_vec();
    *out.get_mut(record + 21)
        .ok_or(DxbcError::MalformedSignature)? |= 0b0011;
    Ok(out)
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
    ((token >> 12) & 0xff == OPERAND_TYPE_TEMP
        && (token >> 2) & 0x3 == SELECTION_MASK
        && (token >> 20) & 0x3 == 1
        && (token >> 22) & 0x7 == 0)
        .then_some((*register, (token >> 4) & 0xf))
}

/// The register and component of a plain temporary source that replicates one component to all four
/// (`rN.cccc`), with no modifier.
fn replicated_temp(operand: &[u32]) -> Option<(u32, u32)> {
    let [token, register] = operand else {
        return None;
    };
    let swizzle = (token >> 4) & 0xff;
    let component = swizzle & 0x3;
    ((token >> 12) & 0xff == OPERAND_TYPE_TEMP
        && token & 0x3 == 2
        && (token >> 2) & 0x3 == SELECTION_SWIZZLE
        && (token >> 20) & 0x3 == 1
        && (token >> 22) & 0x7 == 0
        && swizzle == component * 0x55)
        .then_some((*register, component))
}

/// The signature's system-value code for `SV_Position`, as the element offset arithmetic reads it.
const SIGNATURE_SYSVALUE_POSITION: usize = 1;
/// The bytes of one signature element.
const SIGNATURE_ELEMENT_LEN: usize = 24;

/// The saturate bit of an opcode token.
const SATURATE: u32 = 1 << 13;

/// A constant-buffer operand with two immediate indices, swizzled `.xyxx` and `.zwzz`.
const OPERAND_CB_2D_XYXX: u32 = 0x0020_8046;
const OPERAND_CB_2D_ZWZZ: u32 = 0x0020_8AE6;

/// A temporary register destination writing the components of `mask`.
fn temp_mask(mask: u32) -> u32 {
    2 | SELECTION_MASK << 2 | mask << 4 | OPERAND_TYPE_TEMP << 12 | 1 << 20
}

/// A temporary register source with the four-component `swizzle`.
fn temp_swizzle(swizzle: u32) -> u32 {
    2 | SELECTION_SWIZZLE << 2 | swizzle << 4 | OPERAND_TYPE_TEMP << 12 | 1 << 20
}

/// A temporary register source selecting `component`.
fn temp_select(component: u32) -> u32 {
    2 | SELECTION_SELECT_1 << 2 | component << 4 | OPERAND_TYPE_TEMP << 12 | 1 << 20
}
