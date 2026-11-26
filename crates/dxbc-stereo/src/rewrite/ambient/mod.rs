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
//! - The taps are placed in one of two ways ([`AmbientOcclusionSampling`]). Under
//!   [`Exact`](AmbientOcclusionSampling::Exact) every read of the screen UV outside a sample reads the
//!   eye's own UV computed from the pixel position, `(x - eye offset) / eye width`, with the sizes
//!   read from the depth texture, so the taps and the reconstruction run on the numbers an eye-sized
//!   target rewritten by [`snap_ambient_occlusion_uv`] runs on, and only a sample's coordinate is
//!   taken onto the eye's half, as `u / 2 + eye / 2`, which in the left eye is an exact halving. The
//!   centre samples move half a full-resolution texel off the boundary they would otherwise sit on
//!   ([`CENTRE_NUDGE`]). Under [`DoubleWide`](AmbientOcclusionSampling::DoubleWide) every tap's
//!   horizontal offset from `v1.x` is halved, in place, after the shader has added it to the
//!   double-wide UV, which agrees in real arithmetic but rounds the tap twice at the double-wide UV's
//!   scale.
//! - Every depth and normal sample is clamped to the texel centres of the eye's half, which is what
//!   the clamping sampler does at the edges of an eye-sized target.
//! - The pixel position is made eye-local by subtracting the eye's share of the target width.
//!
//! The pass is found by the names its resource table binds the two textures and the constant array
//! under. A shader without them, or one that touches either texture other than through a sample, is
//! declined.

mod loads;
mod operands;
mod rewriter;

use std::collections::HashSet;

use crate::{
    container::{Dxbc, DxbcError},
    isa::{
        OPCODE_DCL_CONSTANT_BUFFER, OPCODE_DCL_INPUT_PS_SIV, OPCODE_DCL_TEMPS, SB_NAME_POSITION,
    },
    rewrite::{
        ambient::rewriter::{Counts, Registers, Rewriter},
        common::{declared_register, reassemble, signature_register},
        planar::{BINDING_TEXTURE, binding, constant_location},
    },
    tokens::{ShaderStage, TokenStream},
};

/// The constant-buffer slot the rewritten shader reads each eye's basis and normal rotation from.
/// Free across the game's pixel shaders.
pub const AMBIENT_OCCLUSION_CB_REGISTER: u32 = 13;
/// The rows each eye occupies in [`AMBIENT_OCCLUSION_CB_REGISTER`]: the reconstruction basis, then
/// the three rows of the normal's view rotation, in the layout the engine stages them in
/// `cbConstants`.
pub const AMBIENT_OCCLUSION_ROWS_PER_EYE: u32 = 4;

/// How the rewritten passes place their samples, which decides the basis row the caller stages for
/// each eye.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbientOcclusionSampling {
    /// The eye's own screen UV computed from the pixel position, `(x - eye offset) / eye width` and
    /// `y / height`, in place of the interpolated one, and the samples whose texel or weight the
    /// sampler would decide by the coordinate's last bits computed in the shader. An eye-sized target
    /// rewritten by [`snap_ambient_occlusion_uv`] does the same arithmetic on the same operands, so
    /// the two agree to the bit in the left eye, where the rasterizer's and the sampler's handling of
    /// targets of two widths does not. The caller stages each eye's basis exactly as the engine
    /// builds it for an eye rendered on its own.
    Exact,
    /// The double-wide screen UV, with each tap's offset halved after the fact: the caller stages each
    /// eye's basis with its horizontal terms remapped onto the eye's half,
    /// `(2 * basis.x, basis.z - eye * basis.x)`.
    DoubleWide,
}

/// Rewrite the ambient-occlusion generation pixel shader so each half of a double-wide target is
/// computed for its own eye, placing its samples by `sampling`. Returns
/// [`DxbcError::NoAmbientOcclusion`] for any other shader, or for one whose shape this rewrite does
/// not recognise.
pub fn localize_ambient_occlusion(
    blob: &[u8],
    sampling: AmbientOcclusionSampling,
) -> Result<Vec<u8>, DxbcError> {
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
            depth,
            sampling,
        },
        &[depth, normal],
    )
}

/// Rewrite an ambient-occlusion pass of a target that holds one eye (or none) to read the screen UV
/// computed from the pixel position, `x / width` and `y / height`, in place of the interpolated one,
/// and to read the textures whose texel the sampler would decide by the coordinate's last bits
/// through loads: the arithmetic [`AmbientOcclusionSampling::Exact`] and the snapped filters
/// ([`localize_ambient_occlusion_filter`]) do in a double-wide target. The passes are the generation
/// (with its discontinuity-mask variants), the bilateral blur, and the composite. Returns
/// [`DxbcError::NoAmbientOcclusion`] for any other shader.
///
/// The rasterizer's interpolated UV is not the same function of the pixel in targets of two widths,
/// and the sampler turns a normalized coordinate into a texel and a weight with a precision that
/// depends on the texture's width. Both decide something wherever a coordinate lies on or near a
/// texel boundary: the generation's centre samples sit exactly on one, the blur's taps step by half
/// a texel, and the composite's bilinear weights are quantized. A UV both renders compute by one
/// division of the same operands, a texel index both compute by one product, and a weight both
/// compute in the shader agree. The target's size is read from a texture of known relation to it,
/// never from the UV's derivatives, whose rounding at these sizes is a large fraction of a pixel.
pub fn snap_ambient_occlusion_uv(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let rdef = chunk(&dxbc, blob, b"RDEF").ok_or(DxbcError::NoAmbientOcclusion)?;
    let texture = |name| binding(rdef, name, BINDING_TEXTURE);
    let constants = constant_location(rdef, CONSTANTS).is_some();
    let isgn = chunk(&dxbc, blob, b"ISGN").ok_or(DxbcError::MissingInputSignature)?;
    let (position, _) =
        signature_register(isgn, "SV_Position", 0).ok_or(DxbcError::NoAmbientOcclusion)?;
    let (pass, textures) = if let (Some(depth), Some(normal), true) =
        (texture(DEPTH_TEXTURE), texture(NORMAL_TEXTURE), constants)
    {
        (
            Pass::Snap {
                position,
                size: SizeSource::generation(depth, 1.0),
                centre: true,
                loads: Loads::NONE,
            },
            vec![depth, normal],
        )
    } else if let (Some(source), true) = (
        texture(BLUR_SOURCE),
        constants && binding(rdef, BLUR_SAMPLER, BINDING_SAMPLER).is_some(),
    ) {
        (
            Pass::Snap {
                position,
                size: SizeSource::same(source),
                centre: false,
                loads: Loads::point(source),
            },
            vec![],
        )
    } else if let (Some(occlusion), Some(volumes)) =
        (texture(COMPOSITE_SOURCE), texture(COMPOSITE_VOLUME))
    {
        (
            Pass::Snap {
                position,
                size: SizeSource::same(volumes),
                centre: false,
                loads: Loads::composite(occlusion, volumes),
            },
            vec![],
        )
    } else {
        return Err(DxbcError::NoAmbientOcclusion);
    };
    rewrite_pass(&dxbc, blob, pass, &textures)
}

/// Rewrite the ambient-occlusion pass's bilateral blur (`ssao_blur`) or its composite (`ssao_apply`)
/// so each half of a double-wide target reads only its own eye's occlusion, and with `snap` reads the
/// screen UV computed from the pixel position over the target's size (see
/// [`snap_ambient_occlusion_uv`]). Returns [`DxbcError::NoAmbientOcclusion`] for any other shader.
///
/// Both passes address their source by the screen UV, which is right under a double-wide target --
/// the blur steps one target texel through the screen-size constant, and the composite upsamples by
/// the UV -- except at the seam, where the blur's outer taps and the composite's bilinear footprint
/// reach into the other eye. Each sample of the source is clamped to the texel centres of the eye's
/// half, which is what the clamping sampler does at the edges of an eye-sized target. The blur is the
/// shader binding a texture named `Source` through `SamplerNearest` with a `Constants` array; the
/// composite is the shader binding `Src` and `AOVolumeSrc`, of which only `Src`, the occlusion, is
/// clamped.
pub fn localize_ambient_occlusion_filter(blob: &[u8], snap: bool) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let rdef = chunk(&dxbc, blob, b"RDEF").ok_or(DxbcError::NoAmbientOcclusion)?;
    let blur = binding(rdef, BLUR_SOURCE, BINDING_TEXTURE).filter(|_| {
        binding(rdef, BLUR_SAMPLER, BINDING_SAMPLER).is_some()
            && constant_location(rdef, CONSTANTS).is_some()
    });
    let composite = binding(rdef, COMPOSITE_SOURCE, BINDING_TEXTURE)
        .filter(|_| binding(rdef, COMPOSITE_VOLUME, BINDING_TEXTURE).is_some());
    let source = blur.or(composite).ok_or(DxbcError::NoAmbientOcclusion)?;
    let snap = if snap {
        let isgn = chunk(&dxbc, blob, b"ISGN").ok_or(DxbcError::MissingInputSignature)?;
        let (position, _) =
            signature_register(isgn, "SV_Position", 0).ok_or(DxbcError::NoAmbientOcclusion)?;
        // The blur's target is its source's size; the composite's is the full-resolution volumes'.
        let size = match blur {
            Some(blur) => SizeSource::same(blur),
            None => SizeSource::same(
                binding(rdef, COMPOSITE_VOLUME, BINDING_TEXTURE).expect("checked above"),
            ),
        };
        Some((position, size))
    } else {
        None
    };
    let loads = match (snap, blur) {
        (None, _) => Loads::NONE,
        (Some(_), Some(blur)) => Loads::point(blur),
        (Some(_), None) => Loads::composite(
            source,
            binding(rdef, COMPOSITE_VOLUME, BINDING_TEXTURE).expect("checked above"),
        ),
    };
    rewrite_pass(
        &dxbc,
        blob,
        Pass::Filter {
            source,
            snap,
            loads,
        },
        &[source],
    )
}

/// The body of a chunk.
fn chunk<'a>(dxbc: &Dxbc, blob: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    dxbc.chunk(tag).map(|c| c.body(blob))
}

/// Which pass a rewrite is localizing, and what that pass alone needs.
#[derive(Clone, Copy)]
enum Pass {
    /// The generation pass: its basis and rotation rows move to the mod-owned buffer, its pixel
    /// position becomes eye-local, and its samples are placed by `sampling`.
    Generation {
        constants_slot: u32,
        position: u32,
        depth: u32,
        sampling: AmbientOcclusionSampling,
    },
    /// The blur or the composite: only the samples of `source` are clamped, and with `snap` (the
    /// pixel position's input register) every read of the screen UV reads the UV computed from the
    /// pixel position instead.
    ///
    /// The textures in `loads` are read through loads instead of samples (see [`Loads`]). `snap`
    /// holds the pixel position's input register and where the target's size comes from.
    Filter {
        source: u32,
        snap: Option<(u32, SizeSource)>,
        loads: Loads,
    },
    /// A pass of a target holding one eye: every read of the screen UV reads the UV computed from
    /// the pixel position instead, and in the generation pass (`centre`) the centre samples read it
    /// moved off the texel boundary (see [`CENTRE_NUDGE`]). The textures in `loads` are read through
    /// loads instead of samples (see [`Loads`]).
    Snap {
        position: u32,
        size: SizeSource,
        centre: bool,
        loads: Loads,
    },
}

/// Where a snapped pass reads its target's size: a texture's top-mip size, scaled. The scaled height
/// is floored, as the engine sizes a reduced-resolution target by shifting the full one.
#[derive(Clone, Copy)]
struct SizeSource {
    texture: u32,
    width_scale: f32,
    height_scale: f32,
}

impl SizeSource {
    /// A target the size of `texture`.
    fn same(texture: u32) -> Self {
        Self {
            texture,
            width_scale: 1.0,
            height_scale: 1.0,
        }
    }

    /// The generation pass's target, half the size of the full-resolution `depth`; `eyes` is how
    /// many eyes the width holds, whose share of it is each eye's own width.
    fn generation(depth: u32, eyes: f32) -> Self {
        Self {
            texture: depth,
            width_scale: 0.5 / eyes,
            height_scale: 0.5,
        }
    }
}

/// How a pass reads a texture through loads instead of samples.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LoadKind {
    /// A point sample: the texel the coordinate floors to ([`Rewriter::emit_point_load`]).
    Point,
    /// A bilinear sample: four texels blended in the shader ([`Rewriter::emit_bilinear_load`]).
    Bilinear,
}

/// The textures (at most two) a snapped pass reads through loads, and how: the blur's source by
/// point, the composite's occlusion by bilinear and its occlusion volumes by point. The sampler
/// turns a normalized coordinate into a texel and a weight with a precision that depends on the
/// texture's width, so a double-wide texture and an eye-sized one disagree where a coordinate lies
/// within a hair of a texel boundary; the same arithmetic done in the shader agrees.
#[derive(Clone, Copy)]
struct Loads([Option<(u32, LoadKind)>; 2]);

impl Loads {
    const NONE: Self = Self([None, None]);

    fn point(texture: u32) -> Self {
        Self([Some((texture, LoadKind::Point)), None])
    }

    fn composite(occlusion: u32, volumes: u32) -> Self {
        Self([
            Some((occlusion, LoadKind::Bilinear)),
            Some((volumes, LoadKind::Point)),
        ])
    }
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
    let mut position_declared = false;
    for insn in stream.instructions() {
        let insn = insn?;
        match insn.opcode {
            OPCODE_DCL_TEMPS => temps = tokens.get(insn.start + 1).copied(),
            OPCODE_DCL_INPUT_PS_SIV if tokens.get(insn.end - 1) == Some(&SB_NAME_POSITION) => {
                position_declared = true;
            }
            OPCODE_DCL_CONSTANT_BUFFER
                if declared_register(tokens, &insn)? == AMBIENT_OCCLUSION_CB_REGISTER =>
            {
                return Err(DxbcError::Cb13AlreadyDeclared);
            }
            _ => {}
        }
    }
    // A shader with no temporaries declares none; the rewrite then declares its own.
    let first = match (temps, pass) {
        (Some(first), _) => first,
        (None, Pass::Snap { centre: false, .. }) => 0,
        (None, _) => return Err(DxbcError::NoAmbientOcclusion),
    };
    let registers = Registers {
        eye: first,
        bounds: first + 1,
        position: first + 2,
        coordinate: first + 3,
        scratch: first + 4,
        local: first + 5,
        sizes: [first + 6, first + 7],
        weights: first + 8,
        corners: first + 9,
        samples: first + 10,
        texcoord,
    };

    let mut rewriter = Rewriter {
        tokens,
        registers,
        pass,
        out: tokens[..2].to_vec(),
        scaled: HashSet::new(),
        counts: Counts::default(),
        position_declared,
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
                    .extend([(2 << 24) | OPCODE_DCL_TEMPS, first + ADDED_TEMPS]);
            }
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
        Pass::Snap { .. } => counts.redirected > 0,
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

/// How far the generation pass's centre samples move right, in units of the eye's width: a quarter
/// of a target pixel, half a texel of the full-resolution depth and normals.
///
/// A target pixel's centre falls exactly on the boundary between two full-resolution texels, so the
/// texel a point sample takes there rests on the last bit of the coordinate and on how the sampler
/// quantizes it, which differs between an eye-sized texture and a double-wide one of twice the width
/// (the double-wide one's normalized coordinate carries one bit less of the position). Half a texel
/// to the right is the centre of the texel the exact coordinate floors to, far from either boundary,
/// and every mode computes it by the same division of the same operands.
const CENTRE_NUDGE: f32 = 0.25;

/// The temporaries the rewrite adds after the shader's own.
const ADDED_TEMPS: u32 = 11;

/// The fraction of a target texel the sample clamp keeps inside each eye's half. Any value below one
/// half lands a point sample on the eye's edge texel; the target is half the resolution of the depth
/// texture's top mip, so this is a quarter of that mip's texel.
const CLAMP_INSET: f32 = 0.125;
