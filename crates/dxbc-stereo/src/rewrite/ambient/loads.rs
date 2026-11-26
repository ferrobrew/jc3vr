//! The samples a snapped pass reads through loads instead: a point sample as the texel its
//! coordinate floors to, a bilinear one as four texels blended in the shader.

use crate::{
    container::DxbcError,
    rewrite::ambient::{
        LoadKind, Loads, Pass,
        operands::{
            IMMEDIATE_VECTOR, MODIFIER_NEG, NEGATE_FLAG, OPCODE_ADD, OPCODE_FTOI, OPCODE_LD,
            OPCODE_MAD, OPCODE_MAX, OPCODE_MIN, OPCODE_MOV, OPCODE_MUL, OPCODE_RESINFO,
            OPCODE_ROUND_NI, RESOURCE_XYZW, TEMP_MASK_XY, TEMP_MASK_XYZW, TEMP_MASK_ZW,
            TEMP_XYXX_SOURCE, TEMP_XYXY_SOURCE, TEMP_XYZW_SOURCE, TEMP_ZWZW_SOURCE,
            mask_destination, swizzle_source,
        },
        rewriter::{Registers, Rewriter},
    },
};

impl Rewriter<'_> {
    /// How this pass loads from `resource` instead of sampling it, and the register holding that
    /// texture's size, if it does.
    pub(super) fn load_of(&self, resource: u32) -> Option<(LoadKind, u32)> {
        let loads = match self.pass {
            Pass::Filter { loads, .. } | Pass::Snap { loads, .. } => loads,
            Pass::Generation { .. } => Loads::NONE,
        };
        loads
            .0
            .iter()
            .zip(self.registers.sizes)
            .find_map(|(load, size)| load.filter(|(r, _)| *r == resource).map(|(_, k)| (k, size)))
    }

    /// The sizes of the textures this pass loads from, into [`Registers::sizes`]: `.xy` the width
    /// and height at the top mip, `.zw` the last column and row.
    pub(super) fn emit_load_sizes(&mut self, loads: Loads) {
        for (load, size) in loads.0.iter().zip(self.registers.sizes) {
            let Some((resource, _)) = load else {
                continue;
            };
            self.raw(
                OPCODE_RESINFO,
                &[TEMP_MASK_XYZW, size, 0x4001, 0, RESOURCE_XYZW, *resource],
            );
            self.raw(
                OPCODE_ADD,
                &[
                    TEMP_MASK_ZW,
                    size,
                    TEMP_XYXY_SOURCE,
                    size,
                    IMMEDIATE_VECTOR,
                    0,
                    0,
                    (-1.0f32).to_bits(),
                    (-1.0f32).to_bits(),
                ],
            );
        }
    }

    /// Replace a point sample at `coordinate` with a load of the texel the coordinate floors to,
    /// clamped to the texture as the clamping sampler is: `floor(coordinate * size)`.
    ///
    /// Two renders of one point through targets of two widths place it at a normalized coordinate
    /// and at exactly half of it, and the sampler's conversion of the coordinate to a texel carries
    /// one bit less of the position in the wider texture, so a tap within a hair of a texel boundary
    /// takes one texel in one and its neighbour in the other. The product of the coordinate and the
    /// size, computed in the shader, is the same number in both.
    pub(super) fn emit_point_load(
        &mut self,
        destination: &[u32],
        coordinate: u32,
        resource: &[u32],
        size: u32,
    ) {
        let scratch = self.registers.scratch;
        self.raw(
            OPCODE_MUL,
            &[
                TEMP_MASK_XY,
                scratch,
                TEMP_XYXX_SOURCE,
                coordinate,
                TEMP_XYXX_SOURCE,
                size,
            ],
        );
        self.raw(
            OPCODE_ROUND_NI,
            &[TEMP_MASK_XY, scratch, TEMP_XYXX_SOURCE, scratch],
        );
        self.emit_texel_clamp(TEMP_MASK_XY, scratch, size);
        self.raw(
            OPCODE_FTOI,
            &[TEMP_MASK_XY, scratch, TEMP_XYXX_SOURCE, scratch],
        );
        self.raw(
            OPCODE_MOV,
            &[TEMP_MASK_ZW, scratch, IMMEDIATE_VECTOR, 0, 0, 0, 0],
        );
        let mut operands = destination.to_vec();
        operands.extend([TEMP_XYZW_SOURCE, scratch]);
        operands.extend_from_slice(resource);
        self.raw(OPCODE_LD, &operands);
        self.counts.clamps += 1;
    }

    /// Replace a bilinear sample at `coordinate` with four loads weighted in the shader: the corners
    /// around `coordinate * size - 0.5`, each clamped to the texture, blended by the fraction.
    ///
    /// The sampler derives its weights from the coordinate with the same width-dependent precision as
    /// a point sample's texel, and quantizes them; the fraction computed in the shader is the same
    /// number in both renders, and so is the blend.
    pub(super) fn emit_bilinear_load(
        &mut self,
        destination: &[u32],
        coordinate: u32,
        resource: &[u32],
        size: u32,
    ) -> Result<(), DxbcError> {
        let Registers {
            scratch,
            weights,
            corners,
            samples,
            ..
        } = self.registers;
        // The one component the sample reads into its destination, from every corner.
        let component =
            single_component(destination, resource).ok_or(DxbcError::NoAmbientOcclusion)?;
        let mut replicated = resource.to_vec();
        replicated[0] = (replicated[0] & !(0xff << 4)) | (component * 0x55) << 4;
        self.raw(
            OPCODE_MAD,
            &[
                TEMP_MASK_XY,
                weights,
                TEMP_XYXX_SOURCE,
                coordinate,
                TEMP_XYXX_SOURCE,
                size,
                IMMEDIATE_VECTOR,
                (-0.5f32).to_bits(),
                (-0.5f32).to_bits(),
                0,
                0,
            ],
        );
        self.raw(
            OPCODE_ROUND_NI,
            &[TEMP_MASK_XY, corners, TEMP_XYXX_SOURCE, weights],
        );
        self.raw(
            OPCODE_ADD,
            &[
                TEMP_MASK_XY,
                weights,
                TEMP_XYXX_SOURCE,
                weights,
                TEMP_XYXX_SOURCE | NEGATE_FLAG,
                MODIFIER_NEG,
                corners,
            ],
        );
        self.raw(
            OPCODE_ADD,
            &[
                TEMP_MASK_ZW,
                corners,
                TEMP_XYXY_SOURCE,
                corners,
                IMMEDIATE_VECTOR,
                0,
                0,
                1.0f32.to_bits(),
                1.0f32.to_bits(),
            ],
        );
        self.emit_texel_clamp(TEMP_MASK_XYZW, corners, size);
        self.raw(
            OPCODE_FTOI,
            &[TEMP_MASK_XYZW, corners, TEMP_XYZW_SOURCE, corners],
        );
        self.raw(
            OPCODE_MOV,
            &[TEMP_MASK_ZW, scratch, IMMEDIATE_VECTOR, 0, 0, 0, 0],
        );
        // (x0, y0), (x1, y0), (x0, y1), (x1, y1) into samples.x, .y, .z, .w.
        for (target, columns) in [(0, [0, 1]), (1, [2, 1]), (2, [0, 3]), (3, [2, 3])] {
            self.raw(
                OPCODE_MOV,
                &[
                    TEMP_MASK_XY,
                    scratch,
                    swizzle_source([columns[0], columns[1], 0, 0]),
                    corners,
                ],
            );
            let mut operands = vec![mask_destination(1 << target), samples];
            operands.extend([TEMP_XYZW_SOURCE, scratch]);
            operands.extend_from_slice(&replicated);
            self.raw(OPCODE_LD, &operands);
        }
        // The two rows along x, then the blend of the rows along y.
        self.raw(
            OPCODE_ADD,
            &[
                TEMP_MASK_ZW,
                weights,
                swizzle_source([0, 0, 1, 3]),
                samples,
                swizzle_source([0, 0, 0, 2]) | NEGATE_FLAG,
                MODIFIER_NEG,
                samples,
            ],
        );
        self.raw(
            OPCODE_MAD,
            &[
                TEMP_MASK_ZW,
                weights,
                swizzle_source([2, 2, 2, 3]),
                weights,
                swizzle_source([0, 0, 0, 0]),
                weights,
                swizzle_source([0, 0, 0, 2]),
                samples,
            ],
        );
        self.raw(
            OPCODE_ADD,
            &[
                mask_destination(0b1000),
                weights,
                swizzle_source([3, 3, 3, 3]),
                weights,
                swizzle_source([2, 2, 2, 2]) | NEGATE_FLAG,
                MODIFIER_NEG,
                weights,
            ],
        );
        let mut operands = destination.to_vec();
        operands.extend([
            swizzle_source([3, 3, 3, 3]),
            weights,
            swizzle_source([1, 1, 1, 1]),
            weights,
            swizzle_source([2, 2, 2, 2]),
            weights,
        ]);
        self.raw(OPCODE_MAD, &operands);
        self.counts.clamps += 1;
        Ok(())
    }

    /// Clamp the float texel indices in `register` (under `mask`, `.xy` or `.xyzw`) to the texture's
    /// first and last column and row.
    pub(super) fn emit_texel_clamp(&mut self, mask: u32, register: u32, size: u32) {
        let source = if mask == TEMP_MASK_XY {
            TEMP_XYXX_SOURCE
        } else {
            TEMP_XYZW_SOURCE
        };
        self.raw(
            OPCODE_MAX,
            &[
                mask,
                register,
                source,
                register,
                IMMEDIATE_VECTOR,
                0,
                0,
                0,
                0,
            ],
        );
        self.raw(
            OPCODE_MIN,
            &[mask, register, source, register, TEMP_ZWZW_SOURCE, size],
        );
    }
}

/// The component of the texture a sample writes into its one-component destination, from the
/// destination's mask and the resource's swizzle, or `None` for a destination of more components.
pub(super) fn single_component(destination: &[u32], resource: &[u32]) -> Option<u32> {
    let mask = (destination.first()? >> 4) & 0xf;
    if mask.count_ones() != 1 {
        return None;
    }
    let lane = mask.trailing_zeros();
    Some((resource.first()? >> (4 + 2 * lane)) & 0x3)
}
