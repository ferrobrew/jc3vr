//! The operand tokens and operand matchers only the ambient-occlusion rewrite uses.

use crate::isa::{
    OPERAND_TYPE_INPUT, OPERAND_TYPE_RESOURCE, OPERAND_TYPE_TEMP, SELECTION_MASK, SELECTION_SWIZZLE,
};

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

/// `rN.xyxy` and `rN.zwzw` as sources, followed by the register index.
pub(super) const OPERAND_TEMP_SWIZZLE_XYXY: u32 = 0x0010_0446;
pub(super) const OPERAND_TEMP_SWIZZLE_ZWZW: u32 = 0x0010_0EE6;
/// `tN.xxxy` as a resource operand, followed by the register index: a `resinfo` through it writes
/// the width to `.z` and the height to `.w` of its destination.
pub(super) const RESOURCE_XXXY: u32 =
    2 | SELECTION_SWIZZLE << 2 | 0x40 << 4 | OPERAND_TYPE_RESOURCE << 12 | 1 << 20;
/// `tN.xyzw` as a resource operand, followed by the register index.
pub(super) const RESOURCE_XYZW: u32 =
    2 | SELECTION_SWIZZLE << 2 | 0xE4 << 4 | OPERAND_TYPE_RESOURCE << 12 | 1 << 20;
