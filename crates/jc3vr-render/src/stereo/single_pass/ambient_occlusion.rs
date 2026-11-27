//! The ambient-occlusion generation pass, computed for each eye in one draw under the collapse.
//!
//! `CRenderBlockSSAO::Draw` generates occlusion with one fullscreen draw into its half-resolution
//! occlusion target. Its pixel shader reconstructs view-space positions from the packed linear depth
//! with a basis the block derives from the render context's projection, and turns the normal buffer
//! into view space with the render context's view rotation, so under the collapse one centred basis
//! and one rotation serve both halves of the double-wide target. Its tap offsets also reach twice as
//! far across the view horizontally, taps near the seam read the other eye's depth, and the right
//! eye's spiral rotation and quad parity are offset by the eye's width.
//!
//! [`dxbc_stereo::localize_ambient_occlusion`] rewrites the pass's pixel shaders so each half computes
//! what an eye-sized target computes, reading the basis and rotation rows of the eye its pixel falls in
//! from a mod-owned pixel constant buffer. This module builds those rows from each eye's projection
//! and pose, exactly as the block derives them from an eye's render context, and binds the buffer for
//! the length of the block. Under `single_pass.ssao_exact_sampling` the shader works in each eye's own
//! screen UV, so the basis is the eye's as the engine builds it; otherwise it works in the
//! double-wide UV, and the basis's horizontal terms are remapped onto the eye's half. The rest of the block needs nothing per eye: the linear-depth pack and its
//! mips are per pixel, the blur steps one target texel whatever the width, the temporal resolve is
//! seeded fresh on every stereo frame (`hooks/graphics_engine/screen/ssao.rs`), and the composite
//! upsamples by the screen UV. Only at the seam do the blur's outer taps and the composite's bilinear
//! footprint reach the other eye, so their shaders are rewritten to clamp those samples to the eye's
//! half.
//!
//! The rewrite is baked into the bytecode when the engine creates the shader, so it is only right
//! while the collapse is live; the debug UI requests a shader reload when the switch changes, and the
//! block binds the rows only while the shader it will run is a rewritten one.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use dxbc_stereo::{
    AMBIENT_OCCLUSION_CB_REGISTER, AMBIENT_OCCLUSION_ROWS_PER_EYE, AmbientOcclusionSampling,
    Applicable, localize_ambient_occlusion, localize_ambient_occlusion_filter,
    snap_ambient_occlusion_uv,
};
use jc3gi::types::math::{Matrix4, Vector4};
use jc3vr_view::stereo::{collapse_eyes, engine_context::EngineContext};
use parking_lot::Mutex;

use crate::stereo::single_pass::{
    applied,
    per_eye_cb::{PerEyeCb, PixelRowsBound, per_eye_rows},
    report_malformed,
};

/// An ambient-occlusion pass's bytecode rewritten for the collapse, or `None` for any other shader.
///
/// Called for every fragment program the engine creates. `enabled` is whether the rewrite applies
/// now, and `sampling` the space its taps are placed in; a generation shader records which
/// sampling it was built with, or that it was built without the rewrite, so the block binds rows
/// that match the shader it will run.
/// The bilateral blur and the composite are rewritten alongside it, so each eye's half reads only its
/// own occlusion at the seam ([`dxbc_stereo::localize_ambient_occlusion_filter`]); they read no eye
/// rows.
pub(crate) fn rewrite_fragment_program(
    code: &[u8],
    enabled: bool,
    sampling: AmbientOcclusionSampling,
) -> Option<Vec<u8>> {
    let rewritten = match localize_ambient_occlusion(code, sampling).applicable() {
        Ok(Some(rewritten)) => {
            REWRITTEN.store(
                if enabled {
                    BuiltWith::from(sampling)
                } else {
                    BuiltWith::Unrewritten
                } as u8,
                Ordering::Relaxed,
            );
            rewritten
        }
        Ok(None) if !enabled => return None,
        Ok(None) => applied(
            "ssao filter",
            localize_ambient_occlusion_filter(code, sampling == AmbientOcclusionSampling::Exact),
        )?,
        Err(error) => {
            report_malformed("ssao generation", error);
            return None;
        }
    };
    if enabled && !LOGGED.swap(true, Ordering::Relaxed) {
        tracing::info!(
            target: "single_pass",
            "ssao: passes rewritten to compute each eye's half of the double-wide target",
        );
    }
    enabled.then_some(rewritten)
}

/// An ambient-occlusion pass's bytecode with its screen UV computed from the pixel position and its
/// boundary-sensitive samples read through loads (`single_pass.ssao_exact_sampling`), for a target
/// that is not double-wide, or `None` for any other shader. The block binds no rows for a snapped
/// generation pass: it reads the engine's own constants.
pub(crate) fn snap_fragment_program(code: &[u8]) -> Option<Vec<u8>> {
    let snapped = applied("ssao uv snap", snap_ambient_occlusion_uv(code))?;
    REWRITTEN.store(BuiltWith::Unrewritten as u8, Ordering::Relaxed);
    Some(snapped)
}

/// Bind each eye's reconstruction rows for the generation pass, for as long as the returned guard
/// lives. `None`, with nothing bound, when the generation shader is not a rewritten one or an eye's
/// projection or pose is not available; the block then runs as it does with the switch off.
pub(crate) fn bind_eye_rows() -> Option<EyeRowsBound> {
    let sampling = match BuiltWith::from_u8(REWRITTEN.load(Ordering::Relaxed)) {
        BuiltWith::Unrewritten => return None,
        BuiltWith::Exact => AmbientOcclusionSampling::Exact,
        BuiltWith::DoubleWide => AmbientOcclusionSampling::DoubleWide,
    };
    let eyes = collapse_eyes()?;
    let rows = per_eye_rows(|eye| {
        Some(eye_rows(
            &eyes[eye].params.projection_reverse_z,
            eyes[eye].world(),
            eye,
            sampling,
        ))
    })?;
    let d3d = EngineContext::get()?;
    match Rows::bind_pixel_scoped(&BUFFER, d3d, &rows) {
        Ok(bound) => Some(bound),
        Err(error) => {
            if !BIND_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    target: "single_pass",
                    "ssao: could not bind the per-eye rows ({error}); ambient occlusion keeps one \
                     basis over both eyes",
                );
            }
            None
        }
    }
}

/// The scope opened by [`bind_eye_rows`]; unbinds the rows when dropped.
pub(crate) type EyeRowsBound = PixelRowsBound<AMBIENT_OCCLUSION_CB_REGISTER>;

/// Release the mod-owned buffer, so it does not outlive an eject.
pub(super) fn release() {
    BUFFER.lock().release();
    REWRITTEN.store(BuiltWith::Unrewritten as u8, Ordering::Relaxed);
}

/// The rows of one eye's block.
const EYE_ROWS: usize = AMBIENT_OCCLUSION_ROWS_PER_EYE as usize;
/// The whole buffer: both eyes' blocks.
const ROWS: usize = 2 * EYE_ROWS;

/// How the generation shader the engine last created was built, as a [`BuiltWith`].
static REWRITTEN: AtomicU8 = AtomicU8::new(BuiltWith::Unrewritten as u8);

/// How a generation shader was built: without the rewrite, or with it under one of the two
/// samplings.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum BuiltWith {
    Unrewritten,
    Exact,
    DoubleWide,
}

impl BuiltWith {
    fn from_u8(value: u8) -> Self {
        match value {
            v if v == Self::Exact as u8 => Self::Exact,
            v if v == Self::DoubleWide as u8 => Self::DoubleWide,
            _ => Self::Unrewritten,
        }
    }
}

impl From<AmbientOcclusionSampling> for BuiltWith {
    fn from(sampling: AmbientOcclusionSampling) -> Self {
        match sampling {
            AmbientOcclusionSampling::Exact => Self::Exact,
            AmbientOcclusionSampling::DoubleWide => Self::DoubleWide,
        }
    }
}
static LOGGED: AtomicBool = AtomicBool::new(false);
static BIND_WARNED: AtomicBool = AtomicBool::new(false);

/// The mod-owned buffer holding both eyes' blocks.
type Rows = PerEyeCb<ROWS, AMBIENT_OCCLUSION_CB_REGISTER>;
static BUFFER: Mutex<Rows> = Mutex::new(Rows::new());

/// One eye's block: the reconstruction basis the block derives from the eye's projection, then the
/// three rows of the eye's view rotation.
///
/// The block forms the basis from four terms of the render context's projection,
/// `(-2 / p00, -2 / p11, (1 - p02) / p00, (p12 + 1) / p11)`, against which the shader reconstructs
/// `z * (uv * basis.xy + basis.zw)`. Under the exact sampling the shader's `uv` is the eye's
/// own, and the basis is staged as it is. Under the double-wide sampling it is the double-wide
/// `u`, of which the eye's own UV is `u' = 2u - eye`, so the horizontal terms become
/// `(2 * basis.x, basis.z - eye * basis.x)`. The rotation rows are the transpose of the view's
/// inverse with its translation dropped, which is the eye's world rotation read by rows.
fn eye_rows(
    projection: &Matrix4,
    eye_world: glam::Mat4,
    eye: usize,
    sampling: AmbientOcclusionSampling,
) -> [Vector4; EYE_ROWS] {
    let p = &projection.data;
    let scale_x = -2.0 / p[0];
    let scale_y = -2.0 / p[5];
    let bias_x = (1.0 - p[2]) / p[0];
    let bias_y = (p[6] + 1.0) / p[5];
    let inverse_view = Matrix4::from(eye_world).data;
    let rotation = |row: usize| Vector4 {
        data: [
            inverse_view[row],
            inverse_view[4 + row],
            inverse_view[8 + row],
            0.0,
        ],
    };
    let basis = match sampling {
        AmbientOcclusionSampling::Exact => [scale_x, scale_y, bias_x, bias_y],
        AmbientOcclusionSampling::DoubleWide => [
            2.0 * scale_x,
            scale_y,
            bias_x - eye as f32 * scale_x,
            bias_y,
        ],
    };
    [
        Vector4 { data: basis },
        rotation(0),
        rotation(1),
        rotation(2),
    ]
}

#[cfg(test)]
mod tests {
    use glam::{Mat4, Quat, Vec3, Vec4};

    use super::*;

    fn projection() -> Matrix4 {
        // An off-axis frustum, as a VR eye has; the block reads only its scale and the two terms the
        // engine's own projections leave at zero.
        Matrix4::from(Mat4::from_cols(
            Vec4::new(0.9, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 0.8, 0.0, 0.0),
            Vec4::new(0.1, -0.05, 0.0, -1.0),
            Vec4::new(0.0, 0.0, 0.2, 0.0),
        ))
    }

    /// Each eye's half reconstructs over the double-wide UV exactly what the eye-sized target
    /// reconstructs over the eye's own UV.
    #[test]
    fn basis_maps_each_half_onto_the_eye() {
        let p = projection();
        let local = |u: f32| u * (-2.0 / p.data[0]) + (1.0 - p.data[2]) / p.data[0];
        for eye in 0..2 {
            let [basis, ..] = eye_rows(
                &p,
                Mat4::IDENTITY,
                eye,
                AmbientOcclusionSampling::DoubleWide,
            );
            for u in [0.0f32, 0.25, 0.5, 1.0] {
                let wide = (u + eye as f32) / 2.0;
                let x = wide * basis.data[0] + basis.data[2];
                assert!(
                    (x - local(u)).abs() < 1e-5,
                    "eye {eye} u {u}: {x} vs {}",
                    local(u)
                );
            }
            assert_eq!(basis.data[1], -2.0 / p.data[5]);
            assert_eq!(basis.data[3], (p.data[6] + 1.0) / p.data[5]);
        }
    }

    /// Under the exact sampling each eye's basis is the one the block builds for an eye rendered on
    /// its own, whichever eye it is.
    #[test]
    fn exact_basis_is_the_eyes_own() {
        let p = projection();
        for eye in 0..2 {
            let [basis, ..] = eye_rows(&p, Mat4::IDENTITY, eye, AmbientOcclusionSampling::Exact);
            assert_eq!(
                basis.data,
                [
                    -2.0 / p.data[0],
                    -2.0 / p.data[5],
                    (1.0 - p.data[2]) / p.data[0],
                    (p.data[6] + 1.0) / p.data[5],
                ]
            );
        }
    }

    /// The rotation rows take a world-space normal into the eye's view space, whatever the eye's
    /// position and cant.
    #[test]
    fn rotation_rows_take_normals_into_view_space() {
        let eye_world = Mat4::from_rotation_translation(
            Quat::from_euler(glam::EulerRot::YXZ, 0.4, -0.2, 0.087),
            Vec3::new(10.0, 2.0, -3.0),
        );
        let [_, rows @ ..] = eye_rows(&projection(), eye_world, 1, AmbientOcclusionSampling::Exact);
        let normal = Vec3::new(0.3, 0.8, -0.52).normalize();
        let view = rows
            .iter()
            .zip([normal.x, normal.y, normal.z])
            .fold(Vec3::ZERO, |sum, (row, n)| {
                sum + Vec3::new(row.data[0], row.data[1], row.data[2]) * n
            });
        let expected = eye_world.inverse().transform_vector3(normal);
        assert!(view.abs_diff_eq(expected, 1e-5), "{view} vs {expected}");
        assert!(rows.iter().all(|row| row.data[3] == 0.0));
    }
}
