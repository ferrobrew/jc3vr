//! The casings a revolver's cylinder dumps, falling from where its chambers were: instances of the
//! stand-in casing model the speedloader is drawn with ([`crate::controllers::weapons::pouch_models`]),
//! each thrown with a small push out of its chamber and falling under gravity, with no body in the
//! physics world, hidden once they have fallen for the tuning's `casings.fall_s`
//! ([`CasingsTuning::fall_s`]).

use std::time::Instant;

use glam::{Mat4, Quat, Vec3};
use parking_lot::Mutex;

use crate::controllers::weapons::{
    pouch_models::owned::OwnedModel,
    reload::{
        plan::MAX_HELD_ROUNDS,
        tuning::{CasingsTuning, RELOAD},
    },
};

/// The world's gravity, metres per second squared, down the world's Y.
const GRAVITY_MPS2: f32 = 9.81;

/// One casing to drop: its world matrix as it leaves its chamber, and the way out of the chamber
/// (out of the cylinder's rear), world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Dumped {
    pub matrix: Mat4,
    pub out: Vec3,
}

/// Drop `casings` of the model the game's model cache holds under `name_hash`. Returns how many are
/// drawn; a model not in the cache draws none.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn drop(name_hash: u32, casings: &[Dumped]) -> usize {
    let mut state = STATE.lock();
    unsafe { state.hide() };
    let now = Instant::now();
    let mut shown = 0;
    for (falling, casing) in state.casings.iter_mut().zip(casings) {
        let made = unsafe { falling.model.show_cached(name_hash, casing.matrix) };
        if made.is_none() {
            continue;
        }
        falling.from = Some((now, *casing));
        shown += 1;
    }
    shown
}

/// Hide the casings that have fallen for long enough. Called on each input tick.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn tick() {
    let fall_s = RELOAD.get().casings.fall_s;
    let mut state = STATE.lock();
    for falling in &mut state.casings {
        if falling
            .from
            .is_some_and(|(at, _)| at.elapsed().as_secs_f32() >= fall_s)
        {
            unsafe { falling.model.hide() };
            falling.from = None;
        }
    }
}

/// Place each falling casing for the frame being drawn.
///
/// # Safety
///
/// The game thread, outside the world update.
pub(crate) unsafe fn render_frame() {
    let tuning = &RELOAD.get().casings;
    let state = STATE.lock();
    for falling in &state.casings {
        let Some((at, casing)) = falling.from else {
            continue;
        };
        let matrix = fallen(casing, at.elapsed().as_secs_f32(), tuning);
        // SAFETY: the game thread, outside the world update.
        unsafe { falling.model.push(matrix) };
    }
}

/// Release every casing.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn release() {
    let mut state = STATE.lock();
    for falling in &mut state.casings {
        unsafe { falling.model.release() };
        falling.from = None;
    }
}

/// `casing`'s world matrix `t` seconds after it left its chamber.
fn fallen(casing: Dumped, t: f32, tuning: &CasingsTuning) -> Mat4 {
    let (scale, rotation, start) = casing.matrix.to_scale_rotation_translation();
    let out = casing.out.try_normalize().unwrap_or(Vec3::NEG_Y);
    let position = start + out * (tuning.eject_mps * t) - Vec3::Y * (0.5 * GRAVITY_MPS2 * t * t);
    let across = out.any_orthonormal_vector();
    let turned = Quat::from_axis_angle(across, tuning.tumble_radps * t) * rotation;
    Mat4::from_scale_rotation_translation(scale, turned, position)
}

#[derive(Debug, Copy, Clone)]
struct Falling {
    model: OwnedModel,
    /// When the casing left its chamber, and how.
    from: Option<(Instant, Dumped)>,
}

struct State {
    casings: [Falling; MAX_HELD_ROUNDS],
}

impl State {
    /// # Safety
    ///
    /// The game thread, before the world update.
    unsafe fn hide(&mut self) {
        for falling in &mut self.casings {
            unsafe { falling.model.hide() };
            falling.from = None;
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    casings: [Falling {
        model: OwnedModel::EMPTY,
        from: None,
    }; MAX_HELD_ROUNDS],
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_casing_leaves_its_chamber_and_falls() {
        let casing = Dumped {
            matrix: Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0)),
            out: Vec3::Z,
        };
        assert!(
            fallen(casing, 0.0, &RELOAD.get().casings)
                .w_axis
                .truncate()
                .distance(Vec3::new(1.0, 2.0, 3.0))
                < 1e-6
        );
        let later = fallen(casing, 0.5, &RELOAD.get().casings).w_axis.truncate();
        assert!(later.z > 3.0 + 0.3, "{later}");
        assert!(later.y < 2.0 - 1.0, "{later}");
    }
}
