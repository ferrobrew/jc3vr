//! The opcodes, operand tokens, and operand builders the rewrite emits.

use crate::container::DxbcError;

/// `rN.<components>` as a source, followed by the register index.
pub(super) fn swizzle_source(components: [u32; 4]) -> u32 {
    let swizzle = components[0] | components[1] << 2 | components[2] << 4 | components[3] << 6;
    2 | SELECTION_SWIZZLE << 2 | swizzle << 4 | OPERAND_TYPE_TEMP << 12 | 1 << 20
}

/// `rN.<mask>` as a destination, followed by the register index.
pub(super) fn mask_destination(mask: u32) -> u32 {
    2 | SELECTION_MASK << 2 | mask << 4 | OPERAND_TYPE_TEMP << 12 | 1 << 20
}

/// A plain register source: a temporary or an input, one immediate index, a swizzle, no modifier.
pub(super) struct PlainSource {
    pub(super) kind: SourceKind,
    pub(super) register: u32,
    /// The components the swizzle reads into `.x` and `.y`.
    pub(super) components: [u32; 2],
}

pub(super) enum SourceKind {
    Temp,
    Input,
}

pub(super) fn plain_source(operand: &[u32]) -> Option<PlainSource> {
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
pub(super) fn resource_register(operand: &[u32]) -> Option<u32> {
    let [token, index, ..] = operand else {
        return None;
    };
    ((token >> 12) & 0xff == OPERAND_TYPE_RESOURCE && token >> 31 == 0).then_some(*index)
}

/// The register and write mask of a plain temporary destination.
pub(super) fn temp_destination(operand: &[u32]) -> Option<(u32, u32)> {
    let [token, register] = operand else {
        return None;
    };
    ((token >> 12) & 0xff == OPERAND_TYPE_TEMP && (token >> 2) & 0x3 == SELECTION_MASK)
        .then_some((*register, (token >> 4) & 0xf))
}

pub(super) const X: u32 = 0;
pub(super) const Y: u32 = 1;
pub(super) const Z: u32 = 2;
pub(super) const W: u32 = 3;

pub(super) const OPCODE_ADD: u32 = 0x00;
pub(super) const OPCODE_AND: u32 = 0x01;
pub(super) const OPCODE_DERIV_RTX: u32 = 0x0b;
pub(super) const OPCODE_DCL_INPUT_PS_SIV: u32 = 0x64;
pub(super) const OPCODE_DIV: u32 = 0x0e;
pub(super) const OPCODE_GE: u32 = 0x1d;
pub(super) const OPCODE_MAD: u32 = 0x32;
pub(super) const OPCODE_MIN: u32 = 0x33;
pub(super) const OPCODE_MAX: u32 = 0x34;
pub(super) const OPCODE_MOV: u32 = 0x36;
pub(super) const OPCODE_MUL: u32 = 0x38;
pub(super) const OPCODE_RESINFO: u32 = 0x3d;
pub(super) const OPCODE_ROUND_NE: u32 = 0x40;
pub(super) const OPCODE_SAMPLE: u32 = 0x45;
pub(super) const OPCODE_SAMPLE_L: u32 = 0x48;
pub(super) const OPCODE_LD: u32 = 0x2d;
pub(super) const OPCODE_ROUND_NI: u32 = 0x41;
pub(super) const OPCODE_FTOI: u32 = 0x1b;

/// The pixel position's interpolation mode in an input declaration.
pub(super) const INTERPOLATION_LINEAR_NOPERSPECTIVE: u32 = 4;
/// The system-value name of the pixel position in an input declaration.
pub(super) const SB_NAME_POSITION: u32 = 1;
/// `vN.xy` as the destination of an input declaration, followed by the register index.
pub(super) const OPERAND_INPUT_MASK_XY: u32 = 0x0010_1032;

/// The instruction-length field of an opcode token.
pub(super) const LENGTH_MASK: u32 = 0x7f00_0000;

pub(super) const OPERAND_TYPE_TEMP: u32 = 0;
pub(super) const OPERAND_TYPE_INPUT: u32 = 1;
pub(super) const OPERAND_TYPE_RESOURCE: u32 = 7;
pub(super) const OPERAND_TYPE_CONSTANT_BUFFER: u32 = 8;

pub(super) const SELECTION_MASK: u32 = 0;
pub(super) const SELECTION_SWIZZLE: u32 = 1;
pub(super) const SELECTION_SELECT_1: u32 = 2;

/// `tN.xxxx` as a resource operand, followed by the register index: a `resinfo` through it writes the
/// texture's width to every component of its destination.
pub(super) const RESOURCE_XXXX: u32 =
    2 | SELECTION_SWIZZLE << 2 | OPERAND_TYPE_RESOURCE << 12 | 1 << 20;

/// `rN.xyxx` as a source, followed by the register index.
pub(super) const TEMP_XYXX_SOURCE: u32 = 0x0010_0046;
/// `rN.xyxy`, `rN.zwzw`, and `rN.xyzw` as sources, followed by the register index.
pub(super) const TEMP_XYXY_SOURCE: u32 = 0x0010_0446;
pub(super) const TEMP_ZWZW_SOURCE: u32 = 0x0010_0EE6;
pub(super) const TEMP_XYZW_SOURCE: u32 = 0x0010_0E46;
/// `rN.xy`, `rN.zw`, and `rN.xyzw` as destinations, followed by the register index.
pub(super) const TEMP_MASK_XY: u32 = 0x0010_0032;
pub(super) const TEMP_MASK_ZW: u32 = 0x0010_00C2;
pub(super) const TEMP_MASK_XYZW: u32 = 0x0010_00F2;
/// A four-component immediate, followed by its four values.
pub(super) const IMMEDIATE_VECTOR: u32 = 0x0000_4002;
/// `tN.xxxy` as a resource operand, followed by the register index: a `resinfo` through it writes
/// the width to `.z` and the height to `.w` of its destination.
pub(super) const RESOURCE_XXXY: u32 =
    2 | SELECTION_SWIZZLE << 2 | 0x40 << 4 | OPERAND_TYPE_RESOURCE << 12 | 1 << 20;
/// `tN.xyzw` as a resource operand, followed by the register index.
pub(super) const RESOURCE_XYZW: u32 =
    2 | SELECTION_SWIZZLE << 2 | 0xE4 << 4 | OPERAND_TYPE_RESOURCE << 12 | 1 << 20;

/// The bit of an operand token that says an extended operand token (a modifier) follows it.
pub(super) const NEGATE_FLAG: u32 = 1 << 31;

/// The extended operand tokens for a negated and an absolute source.
pub(super) const MODIFIER_NEG: u32 = 0x41;
pub(super) const MODIFIER_ABS: u32 = 0x81;

/// The opcode token for `opcode` with an instruction of `length` tokens.
pub(super) fn instruction(opcode: u32, length: usize) -> Result<u32, DxbcError> {
    let length = u32::try_from(length)
        .ok()
        .filter(|length| *length <= LENGTH_MASK >> 24)
        .ok_or(DxbcError::InstructionTooLong)?;
    Ok(opcode | length << 24)
}

/// A four-component, one-dimensional operand token of `operand_type` selecting `component`.
pub(super) fn select(operand_type: u32, component: u32) -> u32 {
    2 | SELECTION_SELECT_1 << 2 | component << 4 | operand_type << 12 | 1 << 20
}

/// `rN.c` as a destination.
pub(super) fn dst(register: u32, component: u32) -> [u32; 2] {
    [
        2 | SELECTION_MASK << 2 | (1 << component) << 4 | OPERAND_TYPE_TEMP << 12 | 1 << 20,
        register,
    ]
}

/// `rN.c` as a source.
pub(super) fn src(register: u32, component: u32) -> [u32; 2] {
    [select(OPERAND_TYPE_TEMP, component), register]
}

/// `rN.c` as a source with an operand modifier.
pub(super) fn src_mod(register: u32, component: u32, modifier: u32) -> [u32; 3] {
    [
        select(OPERAND_TYPE_TEMP, component) | 1 << 31,
        modifier,
        register,
    ]
}

/// `vN.c` as a source.
pub(super) fn input(register: u32, component: u32) -> [u32; 2] {
    [select(OPERAND_TYPE_INPUT, component), register]
}

/// `vN.c` as a source with an operand modifier.
pub(super) fn input_mod(register: u32, component: u32, modifier: u32) -> [u32; 3] {
    [
        select(OPERAND_TYPE_INPUT, component) | 1 << 31,
        modifier,
        register,
    ]
}

/// A one-component immediate holding `bits`.
pub(super) fn imm(bits: u32) -> [u32; 2] {
    [0x4001, bits]
}
