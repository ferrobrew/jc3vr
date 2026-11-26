//! The SM4/5 token vocabulary the parser and the rewrites share: the opcodes, the operand-token
//! fields, the operand tokens more than one rewrite emits, and the operand builders.
//!
//! An operand token's fields are its component count (bits 0-1), selection mode (2-3), mask,
//! swizzle, or selected component (4-11), operand type (12-19), index dimension (20-21), the index
//! representation of each dimension (22-24, 25-27, ...), and the extended-operand flag (31). The
//! named tokens below are byte-identical to what fxc emits for the same operand. A token a single
//! rewrite needs stays beside that rewrite.

use crate::container::DxbcError;

pub(crate) const OPCODE_ADD: u32 = 0x00;
pub(crate) const OPCODE_AND: u32 = 0x01;
pub(crate) const OPCODE_DERIV_RTX: u32 = 0x0b;
pub(crate) const OPCODE_DIV: u32 = 0x0e;
pub(crate) const OPCODE_DP2: u32 = 0x0f;
pub(crate) const OPCODE_DP4: u32 = 0x11;
pub(crate) const OPCODE_ENDLOOP: u32 = 0x16;
pub(crate) const OPCODE_FRC: u32 = 0x1a;
pub(crate) const OPCODE_FTOI: u32 = 0x1b;
pub(crate) const OPCODE_GE: u32 = 0x1d;
pub(crate) const OPCODE_IADD: u32 = 0x1e;
pub(crate) const OPCODE_IGE: u32 = 0x21;
pub(crate) const OPCODE_ILT: u32 = 0x22;
pub(crate) const OPCODE_RETC: u32 = 0x25;
pub(crate) const OPCODE_IMUL: u32 = 0x26;
pub(crate) const OPCODE_LD: u32 = 0x2d;
pub(crate) const OPCODE_LOOP: u32 = 0x30;
pub(crate) const OPCODE_LT: u32 = 0x31;
pub(crate) const OPCODE_MAD: u32 = 0x32;
pub(crate) const OPCODE_MIN: u32 = 0x33;
pub(crate) const OPCODE_MAX: u32 = 0x34;
pub(crate) const OPCODE_CUSTOMDATA: u32 = 0x35;
pub(crate) const OPCODE_MOV: u32 = 0x36;
pub(crate) const OPCODE_MOVC: u32 = 0x37;
pub(crate) const OPCODE_MUL: u32 = 0x38;
pub(crate) const OPCODE_RESINFO: u32 = 0x3d;
pub(crate) const OPCODE_RET: u32 = 0x3e;
pub(crate) const OPCODE_ROUND_NE: u32 = 0x40;
pub(crate) const OPCODE_ROUND_NI: u32 = 0x41;
pub(crate) const OPCODE_SAMPLE: u32 = 0x45;
pub(crate) const OPCODE_SAMPLE_C: u32 = 0x46;
pub(crate) const OPCODE_SAMPLE_C_LZ: u32 = 0x47;
pub(crate) const OPCODE_SAMPLE_L: u32 = 0x48;
pub(crate) const OPCODE_SAMPLE_D: u32 = 0x49;
pub(crate) const OPCODE_SAMPLE_B: u32 = 0x4a;
pub(crate) const OPCODE_USHR: u32 = 0x55;
pub(crate) const OPCODE_DCL_RESOURCE: u32 = 0x58;
pub(crate) const OPCODE_DCL_CONSTANT_BUFFER: u32 = 0x59;
pub(crate) const OPCODE_DCL_INPUT: u32 = 0x5f;
pub(crate) const OPCODE_DCL_INPUT_SGV: u32 = 0x60;
pub(crate) const OPCODE_DCL_INPUT_PS: u32 = 0x62;
pub(crate) const OPCODE_DCL_INPUT_PS_SIV: u32 = 0x64;
pub(crate) const OPCODE_DCL_OUTPUT: u32 = 0x65;
pub(crate) const OPCODE_DCL_OUTPUT_SGV: u32 = 0x66;
pub(crate) const OPCODE_DCL_OUTPUT_SIV: u32 = 0x67;
pub(crate) const OPCODE_DCL_TEMPS: u32 = 0x68;
pub(crate) const OPCODE_GATHER4: u32 = 0x6d;
pub(crate) const OPCODE_HS_CONTROL_POINT_PHASE: u32 = 0x72;
pub(crate) const OPCODE_HS_FORK_PHASE: u32 = 0x73;
pub(crate) const OPCODE_HS_JOIN_PHASE: u32 = 0x74;
pub(crate) const OPCODE_GATHER4_C: u32 = 0x7e;
pub(crate) const OPCODE_RCP: u32 = 0x81;

/// The opcode field of an opcode token.
pub(crate) const OPCODE_MASK: u32 = 0x7ff;
/// The instruction-length field of an opcode token.
pub(crate) const LENGTH_MASK: u32 = 0x7f00_0000;

/// The components, as a selected component or a swizzle lane.
pub(crate) const X: u32 = 0;
pub(crate) const Y: u32 = 1;
pub(crate) const Z: u32 = 2;
pub(crate) const W: u32 = 3;

pub(crate) const OPERAND_TYPE_TEMP: u32 = 0;
pub(crate) const OPERAND_TYPE_INPUT: u32 = 1;
pub(crate) const OPERAND_TYPE_OUTPUT: u32 = 2;
pub(crate) const OPERAND_TYPE_IMMEDIATE32: u32 = 4;
pub(crate) const OPERAND_TYPE_IMMEDIATE64: u32 = 5;
pub(crate) const OPERAND_TYPE_RESOURCE: u32 = 7;
pub(crate) const OPERAND_TYPE_CONSTANT_BUFFER: u32 = 8;

/// The selection modes of a four-component operand.
pub(crate) const SELECTION_MASK: u32 = 0;
pub(crate) const SELECTION_SWIZZLE: u32 = 1;
pub(crate) const SELECTION_SELECT_1: u32 = 2;

/// The representations of one index dimension.
pub(crate) const IDX_IMM32: u32 = 0;
pub(crate) const IDX_IMM64: u32 = 1;
pub(crate) const IDX_RELATIVE: u32 = 2;
pub(crate) const IDX_IMM32_PLUS_RELATIVE: u32 = 3;
pub(crate) const IDX_IMM64_PLUS_RELATIVE: u32 = 4;

/// The bit of an operand token that says an extended operand token follows it.
pub(crate) const OPERAND_EXTENDED: u32 = 1 << 31;
/// The extended operand tokens for a negated and an absolute source.
pub(crate) const MODIFIER_NEG: u32 = 0x41;
pub(crate) const MODIFIER_ABS: u32 = 0x81;

/// `D3D_NAME_POSITION`, the system-value name a `dcl_*_siv` gives `SV_Position`.
pub(crate) const SB_NAME_POSITION: u32 = 1;
/// The system-value field of a signature element for `SV_Position`.
pub(crate) const SIGNATURE_SYSVALUE_POSITION: u32 = 1;
/// The pixel position's interpolation mode in an input declaration.
pub(crate) const INTERPOLATION_LINEAR_NOPERSPECTIVE: u32 = 4;

/// A one-component 32-bit immediate (`l(k)`), followed by its value.
pub(crate) const OPERAND_IMM32_SCALAR: u32 = 0x0000_4001;
/// A four-component 32-bit immediate, followed by its four values.
pub(crate) const OPERAND_IMM32_VECTOR: u32 = 0x0000_4002;

/// `vN.x`, `vN.y` as sources, followed by the register index.
pub(crate) const OPERAND_INPUT_SELECT_X: u32 = 0x0010_100a;
pub(crate) const OPERAND_INPUT_SELECT_Y: u32 = 0x0010_101a;
/// `vN.xyxx` as a source, followed by the register index.
pub(crate) const OPERAND_INPUT_SWIZZLE_XYXX: u32 = 0x0010_1046;
/// `vN.xy` as the destination of an input declaration, followed by the register index.
pub(crate) const OPERAND_INPUT_MASK_XY: u32 = 0x0010_1032;

/// `rN.x`, `rN.y`, `rN.z`, `rN.w`, and the multi-component masks as destinations, followed by the
/// register index.
pub(crate) const OPERAND_TEMP_MASK_X: u32 = 0x0010_0012;
pub(crate) const OPERAND_TEMP_MASK_Y: u32 = 0x0010_0022;
pub(crate) const OPERAND_TEMP_MASK_Z: u32 = 0x0010_0042;
pub(crate) const OPERAND_TEMP_MASK_W: u32 = 0x0010_0082;
pub(crate) const OPERAND_TEMP_MASK_XY: u32 = 0x0010_0032;
pub(crate) const OPERAND_TEMP_MASK_ZW: u32 = 0x0010_00c2;
pub(crate) const OPERAND_TEMP_MASK_XYZW: u32 = 0x0010_00f2;
/// `rN.x` as a source (and as a relative index), followed by the register index.
pub(crate) const OPERAND_TEMP_SELECT_X: u32 = 0x0010_000a;
/// `rN` under a swizzle as a source, followed by the register index.
pub(crate) const OPERAND_TEMP_SWIZZLE_XYXX: u32 = 0x0010_0046;
pub(crate) const OPERAND_TEMP_SWIZZLE_XYZW: u32 = 0x0010_0e46;
pub(crate) const OPERAND_TEMP_SWIZZLE_ZWZZ: u32 = 0x0010_0ae6;

/// `cbN[a][b].x` and `cbN[a][b].z` as sources, followed by the two indices.
pub(crate) const OPERAND_CB_2D_SELECT_X: u32 = 0x0020_800a;
pub(crate) const OPERAND_CB_2D_SELECT_Z: u32 = 0x0020_802a;

/// `tN.xxxx` as a resource operand, followed by the register index: a `resinfo` through it writes the
/// texture's width to every component of its destination.
pub(crate) const RESOURCE_XXXX: u32 =
    2 | SELECTION_SWIZZLE << 2 | OPERAND_TYPE_RESOURCE << 12 | 1 << 20;

/// The opcode token for `opcode` with an instruction of `length` tokens.
pub(crate) fn instruction(opcode: u32, length: usize) -> Result<u32, DxbcError> {
    let length = u32::try_from(length)
        .ok()
        .filter(|length| *length <= LENGTH_MASK >> 24)
        .ok_or(DxbcError::InstructionTooLong)?;
    Ok(opcode | length << 24)
}

/// A four-component, one-dimensional operand token of `operand_type` selecting `component`.
pub(crate) fn select(operand_type: u32, component: u32) -> u32 {
    2 | SELECTION_SELECT_1 << 2 | component << 4 | operand_type << 12 | 1 << 20
}

/// `rN.c` as a destination.
pub(crate) fn dst(register: u32, component: u32) -> [u32; 2] {
    dst_mask(register, 1 << component)
}

/// `rN` with write mask `mask` as a destination.
pub(crate) fn dst_mask(register: u32, mask: u32) -> [u32; 2] {
    [
        2 | SELECTION_MASK << 2 | mask << 4 | OPERAND_TYPE_TEMP << 12 | 1 << 20,
        register,
    ]
}

/// `rN.c` as a source.
pub(crate) fn src(register: u32, component: u32) -> [u32; 2] {
    [select(OPERAND_TYPE_TEMP, component), register]
}

/// `rN.xyzw` as a source.
pub(crate) fn src_xyzw(register: u32) -> [u32; 2] {
    [OPERAND_TEMP_SWIZZLE_XYZW, register]
}

/// `rN.c` as a source with an extended operand token (a modifier).
pub(crate) fn src_mod(register: u32, component: u32, modifier: u32) -> [u32; 3] {
    [
        select(OPERAND_TYPE_TEMP, component) | OPERAND_EXTENDED,
        modifier,
        register,
    ]
}

/// `vN.c` as a source.
pub(crate) fn input(register: u32, component: u32) -> [u32; 2] {
    [select(OPERAND_TYPE_INPUT, component), register]
}

/// `vN.c` as a source with an extended operand token (a modifier).
pub(crate) fn input_mod(register: u32, component: u32, modifier: u32) -> [u32; 3] {
    [
        select(OPERAND_TYPE_INPUT, component) | OPERAND_EXTENDED,
        modifier,
        register,
    ]
}

/// A one-component immediate holding `bits`.
pub(crate) fn imm(bits: u32) -> [u32; 2] {
    [OPERAND_IMM32_SCALAR, bits]
}

/// A four-component immediate.
pub(crate) fn imm4(values: [f32; 4]) -> [u32; 5] {
    let [x, y, z, w] = values.map(f32::to_bits);
    [OPERAND_IMM32_VECTOR, x, y, z, w]
}
