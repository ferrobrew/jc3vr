//! Look the WaveWorks water's planar reflection up over one eye of a double-wide target.
//!
//! The `nvwater*` pixel shaders read the planar reflection (`g_ReflectionTexture`) by the screen UV,
//! `SV_Position × cb0[8].zw`, divided by the G-buffer UV scale (`g_UVScale`) and offset by a
//! normal-driven disturbance in the reflection texture's own space:
//!
//! ```text
//! mul r5.yz, v0.xxyx, cb0[8].zzwz      // the screen UV
//! div r8.x, r5.y, cb1[14].x            // the reflection's horizontal coordinate: u / g_UVScale.x
//! ...
//! sample r6.xyz, r5.xwxx, t2.xyzw, s2  // g_ReflectionTexture
//! ```
//!
//! The reflection pass renders one eye-sized frame from the mirrored camera, while under a
//! double-wide target the screen UV spans both eyes, so each eye reads half of the reflection
//! stretched to twice its width. [`localize_planar_reflection`] gives that division an eye-local
//! numerator, `2u - eye`, which maps each half of the target onto the whole reflection. The other
//! lookups that share the screen UV (refraction colour and depth, ambient occlusion, the dynamic
//! local reflection) address double-wide buffers and keep it.
//!
//! The division is found structurally: the shader must bind a texture named `g_ReflectionTexture`,
//! and the one `div` whose divisor is the `.x` of the constant named `g_UVScale` is the site. A
//! shader with neither, or with more than one such division, is declined.

use std::ops::Range;

use crate::{
    container::{Dxbc, DxbcError},
    rewrite::common::reassemble,
    tokens::{OperandKind, ShaderStage, TokenStream},
};

/// Rewrite a water pixel shader so its planar reflection is looked up over the left (`right ==
/// false`) or right eye's half of a double-wide target. Returns [`DxbcError::NoPlanarReflection`]
/// for a shader that binds no planar reflection or forms its coordinate in a shape this rewrite does
/// not recognise.
pub fn localize_planar_reflection(blob: &[u8], right: bool) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let chunk = |tag| dxbc.chunk(tag).map(|c| c.body(blob));
    let rdef = chunk(b"RDEF").ok_or(DxbcError::NoPlanarReflection)?;
    if binding(rdef, REFLECTION_TEXTURE, BINDING_TEXTURE).is_none() {
        return Err(DxbcError::NoPlanarReflection);
    }
    let scale = constant_location(rdef, UV_SCALE).ok_or(DxbcError::NoPlanarReflection)?;
    let stream = TokenStream::new(chunk(b"SHEX").ok_or(DxbcError::UnsupportedShaderModel)?)?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoPlanarReflection);
    }
    let tokens = stream.tokens();

    let mut site = None;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.opcode != OPCODE_DIV {
            continue;
        }
        let operands: Vec<_> = insn.operands().collect::<Result<_, _>>()?;
        let [destination, numerator, divisor] = operands[..] else {
            continue;
        };
        let divides_by_scale = matches!(
            divisor.kind,
            OperandKind::ConstantBuffer { register, element }
                if (register, element) == (scale.slot, scale.register)
        ) && selected_component(tokens[divisor.token_offset])
            == Some(scale.component);
        if !divides_by_scale {
            continue;
        }
        let destination_token = tokens[destination.token_offset];
        let component = single_temp_component(destination_token, tokens[insn.start])
            .ok_or(DxbcError::NoPlanarReflection)?;
        let found = Site {
            start: insn.start,
            end: insn.end,
            numerator: numerator.token_offset..divisor.token_offset,
            destination: destination.token_offset..numerator.token_offset,
            component,
        };
        if site.replace(found).is_some() {
            return Err(DxbcError::NoPlanarReflection);
        }
    }
    let site = site.ok_or(DxbcError::NoPlanarReflection)?;

    // mad rD.c, <numerator>, l(2), l(-eye)
    // div rD.c, rD.c, <divisor>
    let destination = &tokens[site.destination.clone()];
    let numerator = &tokens[site.numerator.clone()];
    let mut splice = vec![instruction(
        OPCODE_MAD,
        1 + destination.len() + numerator.len() + 4,
    )?];
    splice.extend_from_slice(destination);
    splice.extend_from_slice(numerator);
    let bias = if right { -1f32 } else { 0f32 };
    splice.extend([
        IMMEDIATE_SCALAR,
        2f32.to_bits(),
        IMMEDIATE_SCALAR,
        bias.to_bits(),
    ]);
    let divide_start = splice.len();
    splice.push(tokens[site.start]);
    splice.extend_from_slice(destination);
    splice.push(select_component(destination[0], site.component));
    splice.extend_from_slice(&destination[1..]);
    splice.extend_from_slice(&tokens[site.numerator.end..site.end]);
    let length = splice.len() - divide_start;
    splice[divide_start] = (splice[divide_start] & !LENGTH_MASK) | instruction(0, length)?;

    let mut out = tokens[..site.start].to_vec();
    out.extend(splice);
    out.extend_from_slice(&tokens[site.end..]);
    out[1] = u32::try_from(out.len()).map_err(|_| DxbcError::InstructionTooLong)?;
    reassemble(
        &dxbc,
        blob,
        chunk(b"ISGN")
            .ok_or(DxbcError::MissingInputSignature)?
            .to_vec(),
        chunk(b"OSGN")
            .ok_or(DxbcError::MissingOutputSignature)?
            .to_vec(),
        out.iter().flat_map(|v| v.to_le_bytes()).collect(),
        false,
    )
}

/// The names the water permutations bind the planar reflection and the G-buffer UV scale under.
const REFLECTION_TEXTURE: &[u8] = b"g_ReflectionTexture";
const UV_SCALE: &[u8] = b"g_UVScale";

/// The division that forms the reflection's horizontal coordinate.
struct Site {
    start: usize,
    end: usize,
    destination: Range<usize>,
    numerator: Range<usize>,
    component: u32,
}

/// Where a constant lives: the constant buffer's slot, its register within the buffer, and the
/// component the constant starts at.
pub(super) struct ConstantLocation {
    pub(super) slot: u32,
    pub(super) register: u32,
    pub(super) component: u32,
}

const OPCODE_DIV: u32 = 0x0e;
const OPCODE_MAD: u32 = 0x32;

/// The instruction-length field of an opcode token, and the opcode's extended-token flag.
const LENGTH_MASK: u32 = 0x7f00_0000;
const EXTENDED: u32 = 0x8000_0000;

/// A one-component immediate, followed by its value.
const IMMEDIATE_SCALAR: u32 = 0x4001;

/// The resource table's binding types for a constant buffer and a texture.
const BINDING_CBUFFER: u32 = 0;
pub(super) const BINDING_TEXTURE: u32 = 2;

/// The operand token's fields this rewrite reads or writes.
const OPERAND_TYPE_TEMP: u32 = 0;
const SELECTION_MODE_MASK: u32 = 0;
const SELECTION_MODE_SELECT_1: u32 = 2;

/// The opcode token for `opcode` with an instruction of `length` tokens.
fn instruction(opcode: u32, length: usize) -> Result<u32, DxbcError> {
    let length = u32::try_from(length)
        .ok()
        .filter(|length| *length <= LENGTH_MASK >> 24)
        .ok_or(DxbcError::InstructionTooLong)?;
    Ok(opcode | length << 24)
}

/// The component a select-1 source operand reads.
fn selected_component(token: u32) -> Option<u32> {
    ((token >> 2) & 0x3 == SELECTION_MODE_SELECT_1).then_some((token >> 4) & 0x3)
}

/// The component of a one-dimensional, immediately indexed temporary destination that writes exactly
/// one component. `None` for any other destination, or when the instruction saturates (the
/// division keeps the opcode token, so a saturate would apply to the quotient as it should, but no
/// shipped permutation has one and declining keeps the rewrite to the shape it was checked on).
fn single_temp_component(token: u32, opcode_token: u32) -> Option<u32> {
    let operand_type = (token >> 12) & 0xff;
    let dimensions = (token >> 20) & 0x3;
    let representation = (token >> 22) & 0x7;
    let mask = (token >> 4) & 0xf;
    let plain = operand_type == OPERAND_TYPE_TEMP
        && dimensions == 1
        && representation == 0
        && token & EXTENDED == 0
        && (token >> 2) & 0x3 == SELECTION_MODE_MASK
        && opcode_token & (EXTENDED | 1 << 13) == 0;
    (plain && mask.count_ones() == 1).then(|| mask.trailing_zeros())
}

/// A destination operand token rewritten to read its own `component` as a select-1 source.
fn select_component(destination: u32, component: u32) -> u32 {
    (destination & !0xffc) | SELECTION_MODE_SELECT_1 << 2 | component << 4
}

fn word(rdef: &[u8], offset: usize) -> Option<usize> {
    rdef.get(offset..offset + 4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")) as usize)
}

/// Whether the NUL-terminated string at `offset` in the resource table is `name`.
fn name_at(rdef: &[u8], offset: usize, name: &[u8]) -> bool {
    rdef.get(offset..offset + name.len() + 1)
        .is_some_and(|bound| &bound[..name.len()] == name && bound[name.len()] == 0)
}

/// The register the resource table binds `name` of binding type `kind` to.
pub(super) fn binding(rdef: &[u8], name: &[u8], kind: u32) -> Option<u32> {
    const BINDING_SIZE: usize = 32;
    let (count, table) = (word(rdef, 8)?, word(rdef, 12)?);
    (0..count).find_map(|index| {
        let entry = table + index * BINDING_SIZE;
        (name_at(rdef, word(rdef, entry)?, name) && word(rdef, entry + 4)? == kind as usize)
            .then(|| word(rdef, entry + 20))
            .flatten()
            .and_then(|register| u32::try_from(register).ok())
    })
}

/// Where the resource table places the constant named `name`: the slot its buffer is bound to, and
/// the register and component its first element starts at.
pub(super) fn constant_location(rdef: &[u8], name: &[u8]) -> Option<ConstantLocation> {
    const BUFFER_SIZE: usize = 24;
    // The shader-model-5 header names its variable-description stride; older ones use the
    // shader-model-4 layout.
    let variable_size = if rdef.get(28..32) == Some(b"RD11") {
        word(rdef, 44)?
    } else {
        24
    };
    let (count, table) = (word(rdef, 0)?, word(rdef, 4)?);
    (0..count).find_map(|index| {
        let buffer = table + index * BUFFER_SIZE;
        let (buffer_name, variables, first) = (
            word(rdef, buffer)?,
            word(rdef, buffer + 4)?,
            word(rdef, buffer + 8)?,
        );
        let offset = (0..variables).find_map(|variable| {
            let entry = first + variable * variable_size;
            name_at(rdef, word(rdef, entry)?, name)
                .then(|| word(rdef, entry + 4))
                .flatten()
        })?;
        let buffer_name = rdef.get(buffer_name..)?;
        let buffer_name = &buffer_name[..buffer_name.iter().position(|b| *b == 0)?];
        let slot = binding(rdef, buffer_name, BINDING_CBUFFER)?;
        Some(ConstantLocation {
            slot,
            register: u32::try_from(offset / 16).ok()?,
            component: u32::try_from(offset % 16 / 4).ok()?,
        })
    })
}
