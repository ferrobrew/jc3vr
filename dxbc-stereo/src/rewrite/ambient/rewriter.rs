//! The rewriter that walks a pass's instructions: its registers, its prologue, the sample
//! coordinates it clamps and places, and the operands it redirects.

use std::collections::HashSet;

use crate::{
    container::DxbcError,
    rewrite::{
        ambient::{
            AMBIENT_OCCLUSION_CB_REGISTER, AMBIENT_OCCLUSION_ROWS_PER_EYE,
            AmbientOcclusionSampling, BASIS_ROW, CENTRE_NUDGE, CLAMP_INSET, LoadKind, Pass,
            ROTATION_ROWS, SizeSource,
            operands::{
                IMMEDIATE_VECTOR, INTERPOLATION_LINEAR_NOPERSPECTIVE, LENGTH_MASK, MODIFIER_ABS,
                MODIFIER_NEG, OPCODE_ADD, OPCODE_AND, OPCODE_DCL_INPUT_PS_SIV, OPCODE_DERIV_RTX,
                OPCODE_DIV, OPCODE_GE, OPCODE_MAD, OPCODE_MAX, OPCODE_MIN, OPCODE_MOV, OPCODE_MUL,
                OPCODE_RESINFO, OPCODE_ROUND_NE, OPCODE_ROUND_NI, OPCODE_SAMPLE, OPCODE_SAMPLE_L,
                OPERAND_INPUT_MASK_XY, OPERAND_TYPE_CONSTANT_BUFFER, OPERAND_TYPE_INPUT,
                OPERAND_TYPE_TEMP, RESOURCE_XXXX, RESOURCE_XXXY, SB_NAME_POSITION, SourceKind,
                TEMP_XYXX_SOURCE, W, X, Y, Z, dst, imm, input, input_mod, instruction,
                mask_destination, plain_source, resource_register, select, src, src_mod,
                swizzle_source, temp_destination,
            },
        },
        common::{CB_ACCESS_DYNAMIC_INDEXED, OPCODE_DCL_CONSTANT_BUFFER, OPERAND_CB_2D_IMM},
    },
    tokens::{IDX_IMM32_PLUS_RELATIVE, Instruction},
};

/// The register numbers the rewrite reads and writes.
#[derive(Clone, Copy)]
pub(super) struct Registers {
    /// `.x` all ones in the right eye's half, `.y` the eye's first row in the constant buffer, `.z`
    /// the eye's first UV (`0` or `0.5`), `.w` the centre samples' move under the exact sampling.
    pub(super) eye: u32,
    /// `.x` and `.y` the eye's sample clamp (in a snapped generation pass, `.x` the centre samples'
    /// move); `.z` scratch for the eye's half width.
    pub(super) bounds: u32,
    /// The eye-local pixel position.
    pub(super) position: u32,
    /// A sample's clamped coordinate.
    pub(super) coordinate: u32,
    /// Scratch for a tap's offset.
    pub(super) scratch: u32,
    /// The screen UV computed from the pixel position, which an exact or snapped pass reads in place
    /// of the input, in `.xy`; the target's (or the eye's) width in `.z` and height in `.w`.
    pub(super) local: u32,
    /// The sizes of the textures a pass loads from, one register for each entry of its [`Loads`]:
    /// `.xy` the width and height, `.zw` the last column and row, as floats.
    pub(super) sizes: [u32; 2],
    /// A bilinear load's fraction in `.xy` and its rows' blend in `.zw`.
    pub(super) weights: u32,
    /// A bilinear load's corner texels: `.xy` the first, `.zw` the second.
    pub(super) corners: u32,
    /// A bilinear load's four texel values.
    pub(super) samples: u32,
    /// The screen UV input.
    pub(super) texcoord: u32,
}

#[derive(Default, Clone, Copy)]
pub(super) struct Counts {
    pub(super) clamps: usize,
    pub(super) redirected: usize,
    pub(super) taps: usize,
    pub(super) basis: usize,
    pub(super) rotation: usize,
    pub(super) position: usize,
}

pub(super) struct Rewriter<'a> {
    pub(super) tokens: &'a [u32],
    pub(super) registers: Registers,
    pub(super) pass: Pass,
    pub(super) out: Vec<u32>,
    /// Tap coordinate components already halved and not written since.
    pub(super) scaled: HashSet<(u32, u32)>,
    pub(super) counts: Counts,
    /// Whether the shader declares the pixel position; the snap declares it where it does not.
    pub(super) position_declared: bool,
}

impl Rewriter<'_> {
    pub(super) fn emit_prologue(&mut self) {
        let Registers {
            eye,
            bounds,
            position,
            local,
            texcoord,
            ..
        } = self.registers;
        let half = 0.5f32.to_bits();
        let (pixel, depth, sampling) = match self.pass {
            Pass::Generation {
                position,
                depth,
                sampling,
                ..
            } => (position, depth, sampling),
            Pass::Snap {
                position,
                size,
                centre,
                loads,
            } => {
                self.emit_target_size(size);
                self.emit_snapped_uv(position);
                self.emit_load_sizes(loads);
                if centre {
                    self.emit(
                        OPCODE_DIV,
                        &[
                            &dst(bounds, X),
                            &imm(CENTRE_NUDGE.to_bits()),
                            &src(local, Z),
                        ],
                    );
                }
                return;
            }
            Pass::Filter {
                source,
                snap,
                loads,
            } => {
                if let Some((position, size)) = snap {
                    self.emit_target_size(size);
                    self.emit_snapped_uv(position);
                }
                self.emit_load_sizes(loads);
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
        if sampling == AmbientOcclusionSampling::Exact {
            // The eye's width and the target's height from the depth texture, which the derivative
            // above only approximates; the eye's offset below is then exact.
            self.emit_target_size(SizeSource::generation(depth, 2.0));
            self.emit(OPCODE_MOV, &[&dst(bounds, Z), &src(local, Z)]);
            self.emit(
                OPCODE_DIV,
                &[&dst(eye, W), &imm(CENTRE_NUDGE.to_bits()), &src(local, Z)],
            );
        }
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
        if sampling == AmbientOcclusionSampling::Exact {
            // The eye-local pixel position over the eye's width: the left eye's operands are the
            // eye-sized target's, and the right eye's position is exact after the subtraction.
            self.emit(
                OPCODE_DIV,
                &[&dst(local, X), &src(position, X), &src(local, Z)],
            );
            self.emit(
                OPCODE_DIV,
                &[&dst(local, Y), &input(pixel, Y), &src(local, W)],
            );
        }
    }

    /// Emit an instruction from its operand tokens, computing its length.
    pub(super) fn raw(&mut self, opcode: u32, operands: &[u32]) {
        self.out
            .push(instruction(opcode, 1 + operands.len()).expect("rewrite instructions are short"));
        self.out.extend_from_slice(operands);
    }

    /// `local.zw`, the target's width and height from `size`.
    fn emit_target_size(&mut self, size: SizeSource) {
        let local = self.registers.local;
        self.raw(
            OPCODE_RESINFO,
            &[
                mask_destination(0b1100),
                local,
                0x4001,
                0,
                RESOURCE_XXXY,
                size.texture,
            ],
        );
        self.raw(
            OPCODE_MUL,
            &[
                mask_destination(0b1100),
                local,
                swizzle_source([2, 2, 2, 3]),
                local,
                IMMEDIATE_VECTOR,
                0,
                0,
                size.width_scale.to_bits(),
                size.height_scale.to_bits(),
            ],
        );
        self.raw(
            OPCODE_ROUND_NI,
            &[
                mask_destination(0b1100),
                local,
                swizzle_source([2, 2, 2, 3]),
                local,
            ],
        );
    }

    /// `local.xy`, the pixel position over the target's size in `local.zw`, declaring the position
    /// where the shader does not.
    fn emit_snapped_uv(&mut self, position: u32) {
        let local = self.registers.local;
        self.declare_position(position);
        self.emit(
            OPCODE_DIV,
            &[&dst(local, X), &input(position, X), &src(local, Z)],
        );
        self.emit(
            OPCODE_DIV,
            &[&dst(local, Y), &input(position, Y), &src(local, W)],
        );
    }

    fn declare_position(&mut self, position: u32) {
        if !self.position_declared {
            self.out.extend([
                (4 << 24) | (INTERPOLATION_LINEAR_NOPERSPECTIVE << 11) | OPCODE_DCL_INPUT_PS_SIV,
                OPERAND_INPUT_MASK_XY,
                position,
                SB_NAME_POSITION,
            ]);
            self.position_declared = true;
        }
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

    pub(super) fn rewrite(
        &mut self,
        insn: &Instruction<'_>,
        textures: &[u32],
    ) -> Result<(), DxbcError> {
        let tokens = self.tokens;
        let offsets: Vec<usize> = insn
            .operands()
            .map(|operand| operand.map(|operand| operand.token_offset))
            .collect::<Result<_, _>>()?;
        let span = |index: usize| {
            let end = offsets.get(index + 1).copied().unwrap_or(insn.end);
            offsets[index]..end
        };
        let resource = (0..offsets.len()).find_map(|index| resource_register(&tokens[span(index)]));
        let reads_texture = resource.is_some_and(|r| textures.contains(&r));
        let load = resource.and_then(|r| self.load_of(r));
        let mut coordinate = None;
        if reads_texture || load.is_some() {
            if !matches!(insn.opcode, OPCODE_SAMPLE | OPCODE_SAMPLE_L) || offsets.len() < 4 {
                return Err(DxbcError::NoAmbientOcclusion);
            }
            coordinate = if reads_texture {
                self.emit_clamped_coordinate(&tokens[span(1)])?
            } else {
                Some(self.emit_plain_coordinate(&tokens[span(1)])?)
            };
            if let (Some([_, register]), Some((kind, size))) = (coordinate, load) {
                let (destination, resource) = (&tokens[span(0)], &tokens[span(2)]);
                match kind {
                    LoadKind::Point => self.emit_point_load(destination, register, resource, size),
                    LoadKind::Bilinear => {
                        self.emit_bilinear_load(destination, register, resource, size)?
                    }
                }
                return Ok(());
            }
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

    /// Copy a sample's coordinate into the coordinate temporary, the screen UV read as the snapped one,
    /// returning the operand a load reads it from.
    fn emit_plain_coordinate(&mut self, operand: &[u32]) -> Result<[u32; 2], DxbcError> {
        let Registers {
            coordinate,
            local,
            texcoord,
            ..
        } = self.registers;
        let source = plain_source(operand).ok_or(DxbcError::NoAmbientOcclusion)?;
        let [u, v] = source.components;
        if matches!(source.kind, SourceKind::Input) && source.register == texcoord {
            self.counts.redirected += 1;
        }
        let read = |component| match source.kind {
            SourceKind::Input if source.register == texcoord => src(local, component),
            SourceKind::Input => input(source.register, component),
            SourceKind::Temp => src(source.register, component),
        };
        self.emit(OPCODE_MOV, &[&dst(coordinate, X), &read(u)]);
        self.emit(OPCODE_MOV, &[&dst(coordinate, Y), &read(v)]);
        Ok([TEMP_XYXX_SOURCE, coordinate])
    }

    /// Emit the halving of a tap's offset and the clamp to the eye's half, returning the coordinate
    /// operand the sample reads instead; in a snapped generation pass, only the centre samples' moved
    /// coordinate, and `None` for its taps, which read their own.
    fn emit_clamped_coordinate(&mut self, operand: &[u32]) -> Result<Option<[u32; 2]>, DxbcError> {
        let Registers {
            bounds,
            coordinate,
            scratch,
            texcoord,
            ..
        } = self.registers;
        let source = plain_source(operand).ok_or(DxbcError::NoAmbientOcclusion)?;
        let [u, v] = source.components;
        if let Pass::Snap { .. } = self.pass {
            if !matches!(source.kind, SourceKind::Input) || source.register != texcoord {
                return Ok(None);
            }
            let local = self.registers.local;
            self.emit(
                OPCODE_ADD,
                &[&dst(coordinate, X), &src(local, u), &src(bounds, X)],
            );
            self.emit(OPCODE_MOV, &[&dst(coordinate, Y), &src(local, v)]);
            self.counts.redirected += 1;
            return Ok(Some([TEMP_XYXX_SOURCE, coordinate]));
        }
        let read = |component| match source.kind {
            SourceKind::Temp => src(source.register, component),
            SourceKind::Input => input(source.register, component),
        };
        let sampling = match self.pass {
            Pass::Generation { sampling, .. } => Some(sampling),
            Pass::Filter { .. } | Pass::Snap { .. } => None,
        };
        let filter_snap = matches!(self.pass, Pass::Filter { snap: Some(_), .. });
        let exact = sampling == Some(AmbientOcclusionSampling::Exact);
        let mut clamp_from = read(u);
        let mut row = read(v);
        match source.kind {
            SourceKind::Temp if sampling.is_none() => {}
            SourceKind::Temp if exact => {
                // The tap is in the eye's own UV; only the sample reads it on the eye's half.
                self.emit(
                    OPCODE_MAD,
                    &[
                        &dst(coordinate, X),
                        &read(u),
                        &imm(0.5f32.to_bits()),
                        &src(self.registers.eye, Z),
                    ],
                );
                clamp_from = src(coordinate, X);
                self.counts.taps += 1;
            }
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
            SourceKind::Input
                if source.register == texcoord
                    && sampling == Some(AmbientOcclusionSampling::Exact) =>
            {
                // The centre samples read the computed UV moved off the texel boundary, taken onto
                // the eye's half.
                self.emit(
                    OPCODE_ADD,
                    &[
                        &dst(coordinate, X),
                        &src(self.registers.local, X),
                        &src(self.registers.eye, W),
                    ],
                );
                self.emit(
                    OPCODE_MAD,
                    &[
                        &dst(coordinate, X),
                        &src(coordinate, X),
                        &imm(0.5f32.to_bits()),
                        &src(self.registers.eye, Z),
                    ],
                );
                clamp_from = src(coordinate, X);
                row = src(self.registers.local, Y);
            }
            SourceKind::Input if source.register == texcoord && filter_snap => {
                clamp_from = src(self.registers.local, u);
                row = src(self.registers.local, v);
            }
            SourceKind::Input if source.register == texcoord => {}
            SourceKind::Input => return Err(DxbcError::NoAmbientOcclusion),
        }
        self.emit(
            OPCODE_MAX,
            &[&dst(coordinate, X), &clamp_from, &src(bounds, X)],
        );
        self.emit(
            OPCODE_MIN,
            &[&dst(coordinate, X), &src(coordinate, X), &src(bounds, Y)],
        );
        self.emit(OPCODE_MOV, &[&dst(coordinate, Y), &row]);
        self.counts.clamps += 1;
        Ok(Some([TEMP_XYXX_SOURCE, coordinate]))
    }

    /// Copy an operand, redirecting the pixel position to its eye-local temporary and the basis and
    /// rotation rows to the eye's block of the mod-owned constant buffer.
    fn rewrite_operand(&mut self, operand: &[u32]) {
        let token = operand[0];
        let extended = (token >> 31) as usize;
        let operand_type = (token >> 12) & 0xff;
        let dimensions = (token >> 20) & 0x3;
        let representations = (token >> 22) & 0x1ff;
        let indices = &operand[1 + extended..];
        let reads_texcoord = operand_type == OPERAND_TYPE_INPUT
            && dimensions == 1
            && representations == 0
            && indices == [self.registers.texcoord];
        let (constants_slot, position, sampling) = match self.pass {
            Pass::Generation {
                constants_slot,
                position,
                sampling,
                ..
            } => (constants_slot, position, sampling),
            Pass::Snap { .. } | Pass::Filter { snap: Some(_), .. } if reads_texcoord => {
                self.redirect_to_local(operand);
                return;
            }
            Pass::Snap { .. } | Pass::Filter { .. } => {
                self.out.extend_from_slice(operand);
                return;
            }
        };
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
        if sampling != AmbientOcclusionSampling::DoubleWide && reads_texcoord {
            self.redirect_to_local(operand);
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

    /// Copy a read of the screen UV input as a read of the computed or eye-local UV, keeping its
    /// swizzle and modifiers.
    fn redirect_to_local(&mut self, operand: &[u32]) {
        let token = operand[0];
        let extended = (token >> 31) as usize;
        self.counts.redirected += 1;
        // The temporary's operand type is zero, so clearing the type field retypes the input.
        self.out.push(token & !(0xff << 12));
        self.out.extend_from_slice(&operand[1..1 + extended]);
        self.out.push(self.registers.local);
    }

    pub(super) fn emit(&mut self, opcode: u32, operands: &[&[u32]]) {
        let length = 1 + operands.iter().map(|operand| operand.len()).sum::<usize>();
        self.out
            .push(instruction(opcode, length).expect("prologue instructions are short"));
        for operand in operands {
            self.out.extend_from_slice(operand);
        }
    }
}
