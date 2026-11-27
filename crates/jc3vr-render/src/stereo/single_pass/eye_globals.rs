//! Per-eye fragment global constants for a pass run once per eye.
//!
//! Under the collapse the engine stages the fragment `GlobalConstants` once per pass, from the
//! centred render camera. A fullscreen pass split per eye
//! ([`split_fullscreen_pass`](crate::hooks::graphics_engine::reconstruction)) reconstructs each
//! half's world positions from that eye's basis, but its pixel shader still reads the centred camera
//! position for everything measured from the camera: the view vector of the specular and reflection
//! terms, fog distances, and the origin the sun-shadow lookup subtracts. This module moves the staged
//! camera rows to the eye for the length of one run and puts them back afterwards, so each half is
//! shaded from the eye it is drawn for. The WaveWorks water surface's per-eye re-issue stages them
//! the same way, since its pixel shaders measure the view vector of their reflection and specular
//! terms from the same rows.
//!
//! The sun-shadow cascade transform is baked relative to the camera position the lookup subtracts, so
//! moving that position moves the lookup with it. The transform's translation row takes the same
//! correction `apply_shadow_cascade_anchor_fix` gives a separately rendered eye, which leaves the
//! lookup on the map the cascade was rendered to.
//!
//! The same three rows, moved to each eye, are what the pixel shaders read per pixel from the
//! eye-camera buffer ([`eye_camera`](crate::stereo::single_pass::eye_camera)), so a run staged here
//! and a rewritten shader drawing into that eye's half read the same values.

use std::cell::Cell;

use glam::Vec3;
use jc3gi::{graphics_engine::render_engine::RenderEngine, types::math::Vector4};
use jc3vr_view::stereo::collapse_eye;

use crate::hooks::graphics_engine::upload_global_constants;

/// Move the staged fragment camera rows to `eye` and upload them, for one per-eye run on this
/// thread. A no-op when a run is already staged, or when the eye's offset or the engine is
/// unavailable; [`restore`] undoes exactly what this staged. Each caller gates it on its own switch.
pub(crate) fn stage(eye: usize) {
    if SAVED.get().is_some() {
        return;
    }
    let Some(delta) = eye_world_offset(eye) else {
        return;
    };
    // SAFETY: the render engine singleton is live on the render thread during a pass.
    let Some(engine) = (unsafe { RenderEngine::get() }) else {
        return;
    };
    let rows = &mut engine.m_FPGlobalConstData;
    let saved = SavedRows {
        camera: [rows[CAMERA_POSITION_ROWS[0]], rows[CAMERA_POSITION_ROWS[1]]],
        cascade_translation: rows[CASCADE_TRANSLATION_ROW],
    };
    offset_camera_rows(rows, delta);
    if upload_global_constants(engine) {
        SAVED.set(Some(saved));
    } else {
        saved.write(&mut engine.m_FPGlobalConstData);
    }
}

/// `eye`'s copies of the rows [`stage`] moves, derived from the `staged` fragment block: the two
/// camera position rows and the cascade translation row, in the order of
/// [`dxbc_stereo::EYE_CAMERA_SOURCE_ROWS`]. `None` when no stereo frame is rendering.
pub(super) fn eye_rows(staged: &[Vector4], eye: usize) -> Option<[Vector4; EYE_ROW_COUNT]> {
    let delta = eye_world_offset(eye)?;
    let mut rows = staged.to_vec();
    offset_camera_rows(&mut rows, delta);
    Some(EYE_ROWS.map(|row| rows[row]))
}

/// `eye`'s world offset from the centred render camera in `.xyz`, with `.w` zero: what the eye-camera
/// rewrite subtracts from the clustered-lighting frame constants' light positions, which the engine
/// builds relative to that camera. `None` when no stereo frame is rendering.
pub(super) fn eye_light_offset(eye: usize) -> Option<Vector4> {
    let offset = eye_world_offset(eye)?;
    Some(Vector4 {
        data: [offset.x, offset.y, offset.z, 0.0],
    })
}

/// How many rows [`eye_rows`] gives each eye.
pub(super) const EYE_ROW_COUNT: usize = EYE_ROWS.len();

/// Put back the rows [`stage`] moved and upload them. A no-op when nothing is staged.
pub(crate) fn restore() {
    let Some(saved) = SAVED.take() else {
        return;
    };
    // SAFETY: as in `stage`.
    let Some(engine) = (unsafe { RenderEngine::get() }) else {
        return;
    };
    saved.write(&mut engine.m_FPGlobalConstData);
    upload_global_constants(engine);
}

/// The fragment `GlobalConstants` rows holding the camera world position.
const CAMERA_POSITION_ROWS: [usize; 2] = [4, 5];

/// The first of the three fragment `GlobalConstants` rows holding the sun-shadow cascade transform's
/// linear part, followed by its translation row.
const CASCADE_LINEAR_ROW: usize = 45;
const CASCADE_TRANSLATION_ROW: usize = CASCADE_LINEAR_ROW + 3;

/// Every row [`stage`] moves, in the order the pixel-shader eye-camera buffer lays them out.
const EYE_ROWS: [usize; 3] = [
    CAMERA_POSITION_ROWS[0],
    CAMERA_POSITION_ROWS[1],
    CASCADE_TRANSLATION_ROW,
];

// The pixel-shader rewrite reads each eye's copies of exactly these rows, in this order.
const _: () = {
    let mut i = 0;
    while i < EYE_ROWS.len() {
        assert!(EYE_ROWS[i] == dxbc_stereo::EYE_CAMERA_SOURCE_ROWS[i] as usize);
        i += 1;
    }
    assert!(EYE_ROWS.len() == dxbc_stereo::EYE_CAMERA_LIGHT_OFFSET_ROW as usize);
};

/// The rows [`stage`] overwrote, as the engine staged them.
#[derive(Clone, Copy)]
struct SavedRows {
    camera: [Vector4; 2],
    cascade_translation: Vector4,
}

impl SavedRows {
    fn write(&self, rows: &mut [Vector4]) {
        rows[CAMERA_POSITION_ROWS[0]] = self.camera[0];
        rows[CAMERA_POSITION_ROWS[1]] = self.camera[1];
        rows[CASCADE_TRANSLATION_ROW] = self.cascade_translation;
    }
}

thread_local! {
    /// The rows the run in flight on this thread staged over, or `None` outside a staged run.
    static SAVED: Cell<Option<SavedRows>> = const { Cell::new(None) };
}

/// `eye`'s world-space offset from the centred render camera, or `None` when no stereo frame is
/// rendering.
fn eye_world_offset(eye: usize) -> Option<Vec3> {
    let offset = collapse_eye(eye)?.offset();
    offset.is_finite().then_some(offset)
}

/// Move the camera position rows by `delta`, and add `M · delta` to the cascade transform's
/// translation row, where `M`'s columns are the transform's three linear rows.
fn offset_camera_rows(rows: &mut [Vector4], delta: Vec3) {
    for row in CAMERA_POSITION_ROWS {
        for (value, offset) in rows[row].data.iter_mut().zip(delta.to_array()) {
            *value += offset;
        }
    }
    let linear: [[f32; 4]; 3] = std::array::from_fn(|i| rows[CASCADE_LINEAR_ROW + i].data);
    for (i, value) in rows[CASCADE_TRANSLATION_ROW].data.iter_mut().enumerate() {
        *value += delta.x * linear[0][i] + delta.y * linear[1][i] + delta.z * linear[2][i];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged() -> Vec<Vector4> {
        let mut rows = vec![Vector4 { data: [0.0; 4] }; 95];
        rows[4].data = [100.0, 20.0, -50.0, 1.0];
        rows[5].data = [100.0, 20.0, -50.0, 1.0];
        rows[45].data = [0.5, 0.0, 0.0, 0.0];
        rows[46].data = [0.0, 0.25, 0.0, 0.0];
        rows[47].data = [0.0, 0.0, 2.0, 0.0];
        rows[48].data = [0.1, 0.2, 0.3, 1.0];
        rows
    }

    /// The lookup `M · (world - camera) + translation` gives the same result before and after the
    /// camera rows move, which is what keeps the shadow on the map the cascade was rendered to.
    #[test]
    fn shadow_lookup_is_unchanged_by_the_move() {
        let lookup = |rows: &[Vector4], world: Vec3| -> [f32; 3] {
            let relative = world - Vec3::from_slice(&rows[4].data[..3]);
            std::array::from_fn(|i| {
                relative.x * rows[45].data[i]
                    + relative.y * rows[46].data[i]
                    + relative.z * rows[47].data[i]
                    + rows[48].data[i]
            })
        };
        let world = Vec3::new(130.0, 18.0, -90.0);
        let mut rows = staged();
        let before = lookup(&rows, world);
        offset_camera_rows(&mut rows, Vec3::new(0.03, -0.001, 0.02));
        let after = lookup(&rows, world);
        for (a, b) in before.iter().zip(after) {
            assert!((a - b).abs() < 1e-4, "{before:?} vs {after:?}");
        }
        assert_eq!(rows[4].data, [100.03, 19.999, -49.98, 1.0]);
        assert_eq!(rows[4].data, rows[5].data);
        assert_eq!(rows[48].data[3], 1.0);
    }

    #[test]
    fn saved_rows_put_the_staged_values_back() {
        let mut rows = staged();
        let saved = SavedRows {
            camera: [rows[4], rows[5]],
            cascade_translation: rows[48],
        };
        offset_camera_rows(&mut rows, Vec3::new(1.0, 2.0, 3.0));
        saved.write(&mut rows);
        let original = staged();
        for row in [4, 5, 48] {
            assert_eq!(rows[row].data, original[row].data);
        }
    }
}
