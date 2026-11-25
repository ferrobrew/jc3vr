//! Map the final composite's screen-space overlays over one eye of a double-wide target.
//!
//! The post composite permutations (`comp_*` and `dof_*`) lay two textures over the finished frame
//! by the screen UV: the edge-fade vignette and the lens dirt. Under a double-wide target that UV
//! spans both eyes, so each eye receives half of the overlay, stretched to twice its width.
//!
//! [`localize_screen_overlays`] gives those lookups a UV that spans one eye instead. The eye-local
//! horizontal coordinate is `frac(2u)`: the left half of the target maps to `[0, 1)` and so does the
//! right half. It is applied at three kinds of site:
//!
//! ```text
//! sample r1.xyz, v1.xyxx, t9, s9       // the vignette, read directly by the screen UV
//! sample r1.xyz, v1.xyxx, t10, s10     // the lens dirt, likewise (or by a distorted screen UV)
//! mad r2.xy, v1.xyxx, l(2, 2, 0, 0), l(-1, -1, 0, 0)   // the mirrored vignette's centred coordinate
//! ```
//!
//! The third feeds a vignette lookup that folds the screen about its centre, so the centre has to be
//! the eye's; the vignette sample that follows reads the folded temporary and is left alone. The
//! scene-buffer lookups (scene, blur, bloom, depth) keep the full-target UV, because those buffers
//! are double-wide too.
//!
//! The overlays are found by the names the shader's own resource table binds them under, so the
//! rewrite applies to every permutation that carries one and declines every other pixel shader.

use crate::{
    container::{Dxbc, DxbcError},
    rewrite::common::reassemble,
    tokens::{ShaderStage, TokenStream},
};

/// Rewrite a composite pixel shader so its screen-space overlays span one eye of a double-wide
/// target. Returns [`DxbcError::NoScreenOverlay`] for a shader that binds neither overlay texture or
/// samples them in a shape this rewrite does not recognise.
pub fn localize_screen_overlays(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let chunk = |tag| dxbc.chunk(tag).map(|c| c.body(blob));
    let overlays = overlay_registers(chunk(b"RDEF").ok_or(DxbcError::NoScreenOverlay)?);
    if overlays.is_empty() {
        return Err(DxbcError::NoScreenOverlay);
    }
    let stream = TokenStream::new(chunk(b"SHEX").ok_or(DxbcError::UnsupportedShaderModel)?)?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoScreenOverlay);
    }
    let tokens = stream.tokens();
    let mut out = tokens[..2].to_vec();
    let mut scratch = None;
    let mut sites = 0usize;
    for insn in stream.instructions() {
        let insn = insn?;
        let body = &tokens[insn.start..insn.end];
        if insn.opcode == OPCODE_DCL_TEMPS {
            let [opcode, count] = body else {
                return Err(DxbcError::NoScreenOverlay);
            };
            scratch = Some(*count);
            out.extend([*opcode, count + 1]);
            continue;
        }
        let operands: Vec<usize> = insn
            .operands()
            .map(|operand| operand.map(|operand| operand.token_offset))
            .collect::<Result<_, _>>()?;
        let span = |index: usize| {
            let end = operands.get(index + 1).copied().unwrap_or(insn.end);
            &tokens[operands[index]..end]
        };
        let localized = match (insn.opcode, operands.len()) {
            (OPCODE_SAMPLE, 4)
                if register_of(span(2), OPERAND_RESOURCE).is_some_and(|r| overlays.contains(r)) =>
            {
                // The mirrored vignette's lookup reads the temporary the centred coordinate was
                // folded into, which is already eye-local.
                let folded = register_of(span(2), OPERAND_RESOURCE) == overlays.vignette
                    && register_of(span(1), OPERAND_TEMP).is_some();
                (!folded).then_some(1)
            }
            (OPCODE_MAD, 4)
                if register_of(span(1), OPERAND_INPUT).is_some()
                    && span(2) == centre_scale()
                    && span(3) == centre_bias() =>
            {
                Some(1)
            }
            _ if insn.opcode != OPCODE_SAMPLE
                && !insn.is_declaration()
                && (0..operands.len()).any(|index| {
                    register_of(span(index), OPERAND_RESOURCE).is_some_and(|r| overlays.contains(r))
                }) =>
            {
                return Err(DxbcError::NoScreenOverlay);
            }
            _ => None,
        };
        let Some(index) = localized else {
            out.extend_from_slice(body);
            continue;
        };
        let scratch = scratch.ok_or(DxbcError::NoScreenOverlay)?;
        let source = span(index);
        out.push(instruction(OPCODE_MOV, 3 + source.len())?);
        out.extend([TEMP_XY_DEST, scratch]);
        out.extend_from_slice(source);
        out.extend([
            instruction(OPCODE_MUL, 7)?,
            TEMP_X_DEST,
            scratch,
            TEMP_X_SOURCE,
            scratch,
            IMMEDIATE_SCALAR,
            2f32.to_bits(),
            instruction(OPCODE_FRC, 5)?,
            TEMP_X_DEST,
            scratch,
            TEMP_X_SOURCE,
            scratch,
        ]);
        let start = out.len();
        out.extend_from_slice(&tokens[insn.start..operands[index]]);
        out.extend([TEMP_XYXX_SOURCE, scratch]);
        out.extend_from_slice(&tokens[operands[index] + source.len()..insn.end]);
        let length = out.len() - start;
        out[start] = (out[start] & !LENGTH_MASK) | instruction(0, length)?;
        sites += 1;
    }
    if sites == 0 {
        return Err(DxbcError::NoScreenOverlay);
    }
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

/// The names the composite permutations bind their screen-space overlays under.
const VIGNETTE_TEXTURE: &[u8] = b"EdgeFadeTexture";
const LENS_DIRT_TEXTURE: &[u8] = b"LensDirtTexture";

/// The texture registers a shader binds the overlays to.
struct Overlays {
    vignette: Option<u32>,
    lens_dirt: Option<u32>,
}

impl Overlays {
    fn is_empty(&self) -> bool {
        self.vignette.is_none() && self.lens_dirt.is_none()
    }

    fn contains(&self, register: u32) -> bool {
        self.vignette == Some(register) || self.lens_dirt == Some(register)
    }
}

const OPCODE_FRC: u32 = 0x1a;
const OPCODE_MAD: u32 = 0x32;
const OPCODE_MOV: u32 = 0x36;
const OPCODE_MUL: u32 = 0x38;
const OPCODE_SAMPLE: u32 = 0x45;
const OPCODE_DCL_TEMPS: u32 = 0x68;

/// The instruction-length field of an opcode token.
const LENGTH_MASK: u32 = 0x7f00_0000;

const OPERAND_TEMP: u32 = 0;
const OPERAND_INPUT: u32 = 1;
const OPERAND_RESOURCE: u32 = 7;

/// `rN.xy` as a destination, `rN.x` as a destination and as a source, and `rN.xyxx` as a source; each
/// is followed by the register index.
const TEMP_XY_DEST: u32 = 0x0010_0032;
const TEMP_X_DEST: u32 = 0x0010_0012;
const TEMP_X_SOURCE: u32 = 0x0010_000a;
const TEMP_XYXX_SOURCE: u32 = 0x0010_0046;

/// A one-component immediate, followed by its value.
const IMMEDIATE_SCALAR: u32 = 0x4001;
/// A four-component immediate, followed by its values.
const IMMEDIATE_VECTOR: u32 = 0x4002;

/// The resource table's binding type for a texture, and the size of one binding entry in bytes.
const BINDING_TEXTURE: usize = 2;
const BINDING_SIZE: usize = 32;

/// The opcode token for `opcode` with an instruction of `length` tokens.
fn instruction(opcode: u32, length: usize) -> Result<u32, DxbcError> {
    let length = u32::try_from(length)
        .ok()
        .filter(|length| *length <= LENGTH_MASK >> 24)
        .ok_or(DxbcError::InstructionTooLong)?;
    Ok(opcode | length << 24)
}

/// `l(2, 2, 0, 0)` and `l(-1, -1, 0, 0)`: the scale and bias that turn a `[0, 1]` UV into a
/// coordinate centred on the screen.
fn centre_scale() -> [u32; 5] {
    [IMMEDIATE_VECTOR, 2f32.to_bits(), 2f32.to_bits(), 0, 0]
}

fn centre_bias() -> [u32; 5] {
    let minus_one = (-1f32).to_bits();
    [IMMEDIATE_VECTOR, minus_one, minus_one, 0, 0]
}

/// The register index of a one-dimensional, immediately indexed operand of type `kind`, or `None`
/// for any other operand.
fn register_of(operand: &[u32], kind: u32) -> Option<u32> {
    let [token, index] = operand else {
        return None;
    };
    let operand_type = (token >> 12) & 0xff;
    let dimensions = (token >> 20) & 0x3;
    let representation = (token >> 22) & 0x7;
    (operand_type == kind && dimensions == 1 && representation == 0).then_some(*index)
}

/// The texture registers the shader's resource table binds the overlays to.
fn overlay_registers(rdef: &[u8]) -> Overlays {
    let word = |offset: usize| {
        rdef.get(offset..offset + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")) as usize)
    };
    let binding = |name: &[u8]| {
        let (count, table) = (word(8)?, word(12)?);
        (0..count).find_map(|index| {
            let entry = table + index * BINDING_SIZE;
            let start = word(entry)?;
            let bound = rdef.get(start..start + name.len() + 1)?;
            (word(entry + 4)? == BINDING_TEXTURE
                && &bound[..name.len()] == name
                && bound[name.len()] == 0)
                .then(|| word(entry + 20))
                .flatten()
                .and_then(|register| u32::try_from(register).ok())
        })
    };
    Overlays {
        vignette: binding(VIGNETTE_TEXTURE),
        lens_dirt: binding(LENS_DIRT_TEXTURE),
    }
}
