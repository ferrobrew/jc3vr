//! Keep a screen-mapped pass's texture reads inside the eye's half of a double-wide target.
//!
//! The post chain's filters draw one fullscreen triangle over a target and address their sources by
//! the screen UV `TEXCOORD0`, plus fixed offsets for a filter's taps. Under a double-wide target both
//! the target and its sources hold the two eyes side by side, so every pixel already reads its own
//! eye's data, except near the seam: a blur tap, a box downsample's far corner, a bilinear footprint,
//! or a morphological neighbourhood that reaches across the middle of the source reads the other eye,
//! where an eye-sized target would have read its own edge through the clamping sampler (or, for a
//! texel load, read zero past the edge).
//!
//! The rewrite makes every read of the selected textures behave as it would at the edges of an
//! eye-sized texture:
//!
//! - The eye is the half `TEXCOORD0.x` falls in, so the rewrite needs no size constant.
//! - A filtered sample's horizontal coordinate is clamped to the texel centres of the eye's half of
//!   the sampled texture, measured with `resinfo`, which is what the clamping sampler returns for a
//!   coordinate past the edge of an eye-sized texture. A `gather4` is clamped a whole texel in, so its
//!   two-by-two footprint stays inside the half.
//! - A texel load whose horizontal index falls outside the eye's half returns zero, as an
//!   out-of-range load does.
//!
//! [`localize_post_filter`] rewrites every texture read of the post chain's generic filters, found by
//! the names their resource tables bind their sources under; it is meant to stand in for the original
//! only while a pass that draws over the double-wide target binds it, since several of those programs
//! are shared with passes that draw elsewhere. [`localize_composite_bloom`] rewrites only the final
//! composite's bloom lookups, a pass that always draws over the whole target.
//! [`localize_reflection_blur`] rewrites every texture read of the screen-space reflection blur, whose
//! program only that block binds.

use crate::{
    container::{Dxbc, DxbcError},
    isa::{
        LENGTH_MASK, MODIFIER_NEG, OPCODE_ADD, OPCODE_AND, OPCODE_DCL_RESOURCE, OPCODE_DCL_TEMPS,
        OPCODE_DIV, OPCODE_GATHER4, OPCODE_GATHER4_C, OPCODE_GE, OPCODE_IADD, OPCODE_IGE,
        OPCODE_ILT, OPCODE_LD, OPCODE_MAX, OPCODE_MIN, OPCODE_MOV, OPCODE_RESINFO, OPCODE_SAMPLE,
        OPCODE_SAMPLE_B, OPCODE_SAMPLE_C, OPCODE_SAMPLE_C_LZ, OPCODE_SAMPLE_D, OPCODE_SAMPLE_L,
        OPCODE_USHR, OPERAND_TEMP_SWIZZLE_XYXX, OPERAND_TEMP_SWIZZLE_XYZW, OPERAND_TYPE_INPUT,
        OPERAND_TYPE_RESOURCE, OPERAND_TYPE_TEMP, RESOURCE_XXXX, SELECTION_MASK, SELECTION_SWIZZLE,
        W, X, Y, Z, dst, dst_mask, imm, input, instruction, select, src, src_mod,
    },
    rewrite::common::{reassemble, signature_register},
    tokens::{Instruction, ShaderStage, TokenStream},
};

/// Rewrite one of the post chain's generic filter pixel shaders so each half of a double-wide target
/// reads only its own eye's half of every source. Returns [`DxbcError::NoPostFilter`] for a shader
/// that binds a texture under any other name, reads a texture other than through a two-dimensional
/// sample, gather, or load, or reads none.
pub fn localize_post_filter(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let rdef = chunk(&dxbc, blob, b"RDEF").ok_or(DxbcError::NoPostFilter)?;
    let bindings = texture_bindings(rdef).ok_or(DxbcError::NoPostFilter)?;
    if bindings.is_empty()
        || !bindings
            .iter()
            .all(|(name, _)| POST_FILTER_TEXTURES.contains(name))
    {
        return Err(DxbcError::NoPostFilter);
    }
    let textures: Vec<u32> = bindings.iter().map(|(_, register)| *register).collect();
    rewrite(&dxbc, blob, &textures, DxbcError::NoPostFilter)
}

/// Rewrite a final composite pixel shader (`comp_*`, `dof_*`, and their kin) so each half of a
/// double-wide target reads only its own eye's half of the bloom textures. Returns
/// [`DxbcError::NoCompositeBloom`] for a shader that binds neither, or reads them other than
/// through a two-dimensional sample.
pub fn localize_composite_bloom(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let rdef = chunk(&dxbc, blob, b"RDEF").ok_or(DxbcError::NoCompositeBloom)?;
    let textures: Vec<u32> = texture_bindings(rdef)
        .ok_or(DxbcError::NoCompositeBloom)?
        .into_iter()
        .filter(|(name, _)| COMPOSITE_BLOOM_TEXTURES.contains(name))
        .map(|(_, register)| register)
        .collect();
    if textures.is_empty() {
        return Err(DxbcError::NoCompositeBloom);
    }
    rewrite(&dxbc, blob, &textures, DxbcError::NoCompositeBloom)
}

/// Rewrite the screen-space reflection blur (`screenspacereflectionblur` and
/// `screenspacereflectionblurreducedfireflies`) so each half of a double-wide target gathers only its
/// own eye's reflections, depth, and gloss. Its taps step up to six texels along the blur axis, so the
/// horizontal pass would otherwise read across the seam. Returns [`DxbcError::NoReflectionBlur`] for a
/// shader that binds neither reflection texture, or reads a texture in a shape this rewrite does not
/// recognise.
pub fn localize_reflection_blur(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let rdef = chunk(&dxbc, blob, b"RDEF").ok_or(DxbcError::NoReflectionBlur)?;
    let bindings = texture_bindings(rdef).ok_or(DxbcError::NoReflectionBlur)?;
    let names: Vec<&[u8]> = bindings.iter().map(|(name, _)| *name).collect();
    if !REFLECTION_BLUR_SOURCES
        .iter()
        .all(|name| names.contains(name))
        || !names
            .iter()
            .all(|name| REFLECTION_BLUR_TEXTURES.contains(name))
    {
        return Err(DxbcError::NoReflectionBlur);
    }
    let textures: Vec<u32> = bindings.iter().map(|(_, register)| *register).collect();
    rewrite(&dxbc, blob, &textures, DxbcError::NoReflectionBlur)
}

/// The names the screen-space reflection blur binds the reflection pair under, and every name it binds
/// a texture under.
const REFLECTION_BLUR_SOURCES: [&[u8]; 2] = [b"SrcReflection", b"SrcReflectionAlpha"];
const REFLECTION_BLUR_TEXTURES: [&[u8]; 4] = [
    b"SrcReflection",
    b"SrcReflectionAlpha",
    b"NormalGlossTexture",
    b"LinearDepthTexture",
];

/// The names the post chain's generic filters bind their sources under: the bright pass, the box
/// downsample, the separable blurs, the morphological erosion and dilation, the level composite,
/// and the copy and blend programs the chain upsamples and accumulates with.
const POST_FILTER_TEXTURES: [&[u8]; 8] = [
    b"RenderTarget",
    b"BinaryImage",
    b"Texture0",
    b"Texture1",
    b"Bloom2x",
    b"Bloom4x",
    b"Bloom8x",
    b"Bloom16x",
];

/// The names the composite permutations bind the bloom chain's two results under.
const COMPOSITE_BLOOM_TEXTURES: [&[u8]; 2] = [b"BloomTexture", b"SecondaryBloomTexture"];

fn chunk<'a>(dxbc: &Dxbc, blob: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    dxbc.chunk(tag).map(|c| c.body(blob))
}

/// Every texture binding in the resource table, as its name and register.
fn texture_bindings(rdef: &[u8]) -> Option<Vec<(&[u8], u32)>> {
    let word = |offset: usize| {
        rdef.get(offset..offset + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")) as usize)
    };
    let (count, table) = (word(8)?, word(12)?);
    let mut bindings = Vec::new();
    for index in 0..count {
        let entry = table + index * BINDING_SIZE;
        if word(entry + 4)? != BINDING_TEXTURE {
            continue;
        }
        let name = rdef.get(word(entry)?..)?;
        let name = &name[..name.iter().position(|byte| *byte == 0)?];
        bindings.push((name, u32::try_from(word(entry + 20)?).ok()?));
    }
    Some(bindings)
}

/// Rewrite every read of `textures`, failing with `decline` for a shader whose shape this rewrite
/// does not recognise.
fn rewrite(
    dxbc: &Dxbc,
    blob: &[u8],
    textures: &[u32],
    decline: DxbcError,
) -> Result<Vec<u8>, DxbcError> {
    let isgn = chunk(dxbc, blob, b"ISGN").ok_or(DxbcError::MissingInputSignature)?;
    let (texcoord, _) = signature_register(isgn, "TEXCOORD", 0).ok_or(decline)?;
    let stream =
        TokenStream::new(chunk(dxbc, blob, b"SHEX").ok_or(DxbcError::UnsupportedShaderModel)?)?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(decline);
    }
    let tokens = stream.tokens();

    let mut temps = None;
    for insn in stream.instructions() {
        let insn = insn?;
        match insn.opcode {
            OPCODE_DCL_TEMPS => temps = Some(*tokens.get(insn.start + 1).ok_or(decline)?),
            OPCODE_DCL_RESOURCE => {
                let register = *tokens.get(insn.start + 2).ok_or(decline)?;
                let dimension = (tokens[insn.start] >> 11) & 0x1f;
                if textures.contains(&register) && dimension != RESOURCE_DIMENSION_TEXTURE2D {
                    return Err(decline);
                }
            }
            _ => {}
        }
    }
    let first = temps.unwrap_or(0);
    let mut rewriter = Rewriter {
        tokens,
        registers: Registers {
            eye: first,
            bounds: first + 1,
            coordinate: first + 2,
            mask: first + 3,
            texcoord,
        },
        out: tokens[..2].to_vec(),
        reads: 0,
        decline,
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
            if temps.is_none() {
                rewriter
                    .out
                    .extend([instruction(OPCODE_DCL_TEMPS, 2)?, ADDED_TEMPS]);
            }
            rewriter.emit_prologue();
            prologue_emitted = true;
        }
        rewriter.rewrite(&insn, textures)?;
    }
    if rewriter.reads == 0 {
        return Err(decline);
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

/// The resource table's binding type for a texture, and the size of one binding entry in bytes.
const BINDING_TEXTURE: usize = 2;
const BINDING_SIZE: usize = 32;

const RESOURCE_DIMENSION_TEXTURE2D: u32 = 3;

/// The temporaries the rewrite adds after the shader's own.
const ADDED_TEMPS: u32 = 4;

/// The register numbers the rewrite reads and writes.
#[derive(Clone, Copy)]
struct Registers {
    /// `.x` all ones in the right eye's half, `.z` the eye's first UV (`0` or `0.5`).
    eye: u32,
    /// `.x` and `.y` the read's clamp or index range, `.w` the sampled texture's measure.
    bounds: u32,
    /// A sample's clamped coordinate.
    coordinate: u32,
    /// `.x` all ones when a load's index is inside the eye's half; `.y` scratch.
    mask: u32,
    /// The screen UV input.
    texcoord: u32,
}

/// How a read of a selected texture is kept inside the eye's half.
#[derive(Clone, Copy)]
enum Read {
    /// A filtered sample, clamped to the half's texel centres.
    Sample,
    /// A two-by-two gather, clamped a whole texel inside the half.
    Gather,
    /// A texel load, zeroed outside the half.
    Load,
}

impl Read {
    fn of(opcode: u32) -> Option<Read> {
        match opcode {
            OPCODE_SAMPLE | OPCODE_SAMPLE_C | OPCODE_SAMPLE_C_LZ | OPCODE_SAMPLE_L
            | OPCODE_SAMPLE_D | OPCODE_SAMPLE_B => Some(Read::Sample),
            OPCODE_GATHER4 | OPCODE_GATHER4_C => Some(Read::Gather),
            OPCODE_LD => Some(Read::Load),
            _ => None,
        }
    }
}

struct Rewriter<'a> {
    tokens: &'a [u32],
    registers: Registers,
    out: Vec<u32>,
    reads: usize,
    decline: DxbcError,
}

impl Rewriter<'_> {
    fn emit_prologue(&mut self) {
        let Registers { eye, texcoord, .. } = self.registers;
        let half = 0.5f32.to_bits();
        self.emit(OPCODE_GE, &[&dst(eye, X), &input(texcoord, X), &imm(half)]);
        self.emit(OPCODE_AND, &[&dst(eye, Z), &src(eye, X), &imm(half)]);
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
        let resource = (0..offsets.len()).find_map(|index| {
            resource_register(&tokens[span(index)]).filter(|r| textures.contains(r))
        });
        let Some(resource) = resource else {
            self.out.extend_from_slice(&tokens[insn.start..insn.end]);
            return Ok(());
        };
        if insn.opcode == OPCODE_RESINFO {
            self.out.extend_from_slice(&tokens[insn.start..insn.end]);
            return Ok(());
        }
        let read = Read::of(insn.opcode).ok_or(self.decline)?;
        if offsets.len() < 3 || resource_register(&tokens[span(2)]) != Some(resource) {
            return Err(self.decline);
        }
        let source = plain_source(&tokens[span(1)]).ok_or(self.decline)?;
        self.reads += 1;
        match read {
            Read::Sample | Read::Gather => {
                let inset = match read {
                    Read::Gather => 1.0f32,
                    _ => 0.5,
                };
                self.emit_clamp(resource, &source, inset);
                let start = self.out.len();
                self.out.extend_from_slice(&tokens[insn.start..offsets[1]]);
                self.out
                    .extend([OPERAND_TEMP_SWIZZLE_XYXX, self.registers.coordinate]);
                self.out.extend_from_slice(&tokens[offsets[2]..insn.end]);
                let length = self.out.len() - start;
                self.out[start] = (self.out[start] & !LENGTH_MASK) | instruction(0, length)?;
            }
            Read::Load => {
                let destination = temp_destination(&tokens[span(0)]).ok_or(self.decline)?;
                self.emit_load_mask(resource, &source);
                self.out.extend_from_slice(&tokens[insn.start..insn.end]);
                let Registers { mask, .. } = self.registers;
                let (register, write_mask) = destination;
                self.emit(
                    OPCODE_AND,
                    &[
                        &dst_mask(register, write_mask),
                        &[OPERAND_TEMP_SWIZZLE_XYZW, register],
                        &src(mask, X),
                    ],
                );
            }
        }
        Ok(())
    }

    /// Clamp a sample's horizontal coordinate to `[first + inset, first + 0.5 - inset]` of the
    /// sampled texture, with the inset in its texels.
    fn emit_clamp(&mut self, resource: u32, source: &PlainSource, inset: f32) {
        let Registers {
            eye,
            bounds,
            coordinate,
            ..
        } = self.registers;
        let [u, v] = source.components;
        let read = |component| source.operand(component);
        self.emit(
            OPCODE_RESINFO,
            &[&dst(bounds, W), &imm(0), &[RESOURCE_XXXX, resource]],
        );
        self.emit(
            OPCODE_DIV,
            &[&dst(bounds, W), &imm(inset.to_bits()), &src(bounds, W)],
        );
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
        self.emit(
            OPCODE_MAX,
            &[&dst(coordinate, X), &read(u), &src(bounds, X)],
        );
        self.emit(
            OPCODE_MIN,
            &[&dst(coordinate, X), &src(coordinate, X), &src(bounds, Y)],
        );
        self.emit(OPCODE_MOV, &[&dst(coordinate, Y), &read(v)]);
    }

    /// Set `mask.x` to all ones when a load's horizontal index lies in the eye's half of the loaded
    /// texture, `[eye * width / 2, (eye + 1) * width / 2)`.
    fn emit_load_mask(&mut self, resource: u32, source: &PlainSource) {
        let Registers {
            eye, bounds, mask, ..
        } = self.registers;
        let u = source.components[0];
        self.emit(
            OPCODE_RESINFO | RESINFO_RETURN_UINT,
            &[&dst(bounds, W), &imm(0), &[RESOURCE_XXXX, resource]],
        );
        self.emit(OPCODE_USHR, &[&dst(bounds, W), &src(bounds, W), &imm(1)]);
        self.emit(
            OPCODE_AND,
            &[&dst(bounds, X), &src(eye, X), &src(bounds, W)],
        );
        self.emit(
            OPCODE_IADD,
            &[&dst(bounds, Y), &src(bounds, X), &src(bounds, W)],
        );
        self.emit(
            OPCODE_IGE,
            &[&dst(mask, X), &source.operand(u), &src(bounds, X)],
        );
        self.emit(
            OPCODE_ILT,
            &[&dst(mask, Y), &source.operand(u), &src(bounds, Y)],
        );
        self.emit(OPCODE_AND, &[&dst(mask, X), &src(mask, X), &src(mask, Y)]);
    }

    fn emit(&mut self, opcode: u32, operands: &[&[u32]]) {
        let length = 1 + operands.iter().map(|operand| operand.len()).sum::<usize>();
        self.out
            .push(instruction(opcode, length).expect("added instructions are short"));
        for operand in operands {
            self.out.extend_from_slice(operand);
        }
    }
}

/// A plain register source: a temporary or an input, one immediate index, a swizzle, no modifier.
struct PlainSource {
    operand_type: u32,
    register: u32,
    /// The components the swizzle reads into `.x` and `.y`.
    components: [u32; 2],
}

impl PlainSource {
    /// The source's `component`, selected on its own.
    fn operand(&self, component: u32) -> [u32; 2] {
        [select(self.operand_type, component), self.register]
    }
}

fn plain_source(operand: &[u32]) -> Option<PlainSource> {
    let [token, register] = operand else {
        return None;
    };
    let operand_type = (token >> 12) & 0xff;
    let swizzled = token & 0x3 == 2 && (token >> 2) & 0x3 == SELECTION_SWIZZLE;
    let plain = (token >> 20) & 0x3 == 1 && (token >> 22) & 0x7 == 0;
    (matches!(operand_type, OPERAND_TYPE_TEMP | OPERAND_TYPE_INPUT) && swizzled && plain).then(
        || PlainSource {
            operand_type,
            register: *register,
            components: [(token >> 4) & 0x3, (token >> 6) & 0x3],
        },
    )
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

/// The `resinfo` return-type field selecting an unsigned-integer result.
const RESINFO_RETURN_UINT: u32 = 2 << 11;
