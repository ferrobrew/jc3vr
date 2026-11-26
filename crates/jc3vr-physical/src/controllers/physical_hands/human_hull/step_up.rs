//! Stepping the narrowed player up the kerbs and steps the authored hull steps up.
//!
//! The narrowed hull's bottom cone keeps the authored cone's slope at half its height
//! ([`geometry::narrow_points`](crate::controllers::physical_hands::human_hull::geometry::narrow_points)),
//! so an edge higher than the narrowed cone meets the prism's side as a wall, where the authored
//! cone would carry the character up it. No convex shape of half the width does better: the
//! character's rigid body takes a contact leaning past its maximum slope for a wall, and a convex
//! bottom of half the width reaches its full width no higher than that slope allows over half the
//! run. The game has no separate step-up of its own to raise: its only other probe around the feet,
//! `CPfxCharacterInstance::CheckLedges`, casts down at the edge of the hull to stop the character
//! walking off a drop.
//!
//! So while the player walks into something that stops them, each pre-simulation pass looks ahead
//! for a step: a ray down, just past the narrowed hull's edge in the direction the controller
//! drives the proxy, from above the authored cone's height, must find a walkable top higher than
//! the narrowed cone and no higher than the authored one, with nothing in the way at that height
//! above the feet. The proxy is then lifted, as the hands' push-up lifts it, until its feet clear
//! the step's top, and its own horizontal velocity carries it on.
//!
//! Every function here runs on the thread that steps the physics world, outside the step.

use std::sync::atomic::{AtomicBool, Ordering};

use glam::{Vec2, Vec3};
use jc3gi::character::character::Character;
use jc3vr_body::controllers::{
    aim::{ray::Ray, raycast, raycast::CastOptions},
    mode,
    mode::{Mode, OnFootVariant},
};
use parking_lot::Mutex;

use crate::controllers::physical_hands::{
    climb,
    climb::engine as proxy,
    human_hull::{
        geometry::StepProfile,
        tuning::{HUMAN_HULL, StepUpTuning},
    },
    vault::lifted,
};

/// The local player's narrowed hull's step profile and the authored one it replaces.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PlayerSteps {
    pub(crate) narrowed: StepProfile,
    pub(crate) authored: StepProfile,
}

/// The step-up's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StepUpStatus {
    pub enabled: bool,
    /// Whether the proxy was lifted on the last pass.
    pub lifting: bool,
    /// Steps lifted onto since the payload loaded.
    pub steps: u64,
    /// The height of the last step lifted onto, metres above the feet.
    pub last_height_m: Option<f32>,
}

pub fn status() -> StepUpStatus {
    let state = STATE.lock();
    StepUpStatus {
        enabled: ENABLED.load(Ordering::Relaxed),
        ..state.status
    }
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Lift the narrowed player up a step ahead the authored hull would step up, for the coming
/// physics step. `steps` is the player's step profiles while the player holds a narrowed hull.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the characters' proxy updates
/// (the pre-simulation callback).
pub(crate) unsafe fn step(steps: Option<PlayerSteps>, step_s: f32) {
    let tuning = &HUMAN_HULL.get().step_up;
    let mut state = STATE.lock();
    let Some(steps) = steps.filter(|_| ENABLED.load(Ordering::Relaxed) && unsafe { on_foot() })
    else {
        state.finish();
        return;
    };
    let (Ok(place), Ok(current)) = (unsafe { proxy::player_place() }, unsafe {
        proxy::proxy_velocity()
    }) else {
        state.finish();
        return;
    };
    let feet = place.feet;
    if let Some(lift) = state.lift {
        let elapsed = lift.elapsed_s + step_s;
        match lift_speed(feet.y, lift.top_y, step_s, tuning) {
            Some(speed) if elapsed <= tuning.lift_hold_s => {
                state.lift = Some(Lift {
                    elapsed_s: elapsed,
                    ..lift
                });
                unsafe { state.apply(current, speed) };
            }
            _ => state.finish(),
        }
        return;
    }
    let wanted = Vec2::new(current.x, current.z);
    // SAFETY: forwarded from the caller; the local player is live while the proxy is.
    let measured = unsafe { measured_velocity() };
    let (Some(direction), true) = (
        blocked(wanted, measured, tuning),
        unsafe { proxy::support() }.is_some(),
    ) else {
        return;
    };
    let probes = Probes::ahead(feet, direction, &steps, tuning);
    // SAFETY: forwarded from the caller.
    if !unsafe { clear(probes.clear_from, probes.clear_to) } {
        return;
    }
    // SAFETY: forwarded from the caller.
    let Some(hit) = (unsafe { cast(probes.down_from, probes.down_to) }) else {
        return;
    };
    let Some(height) = step_height(feet.y, hit.position.y, hit.normal.y, &steps, tuning) else {
        return;
    };
    let top_y = feet.y + height;
    let Some(speed) = lift_speed(feet.y, top_y, step_s, tuning) else {
        return;
    };
    tracing::info!(
        height_m = height,
        narrowed_m = steps.narrowed.cone_height_m,
        authored_m = steps.authored.cone_height_m,
        "human hull: the narrowed player steps up a step the authored hull would"
    );
    state.status.steps += 1;
    state.status.last_height_m = Some(height);
    state.lift = Some(Lift {
        top_y,
        elapsed_s: 0.0,
    });
    unsafe { state.apply(current, speed) };
}

static ENABLED: AtomicBool = AtomicBool::new(true);

/// The direction across the ground the player walks into something along, when the controller
/// drives the proxy at `wanted` and the player last moved at `measured`, world `(x, z)`.
fn blocked(wanted: Vec2, measured: Vec2, tuning: &StepUpTuning) -> Option<Vec2> {
    let speed = wanted.length();
    if !(speed.is_finite() && speed >= tuning.min_wanted_mps && measured.is_finite()) {
        return None;
    }
    let direction = wanted / speed;
    (measured.dot(direction) < speed * tuning.blocked_share).then_some(direction)
}

/// The rays that find a step ahead, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Probes {
    /// The path ahead above the authored cone's height that must be clear.
    clear_from: Vec3,
    clear_to: Vec3,
    /// The ray down onto the step's top.
    down_from: Vec3,
    down_to: Vec3,
}

impl Probes {
    fn ahead(feet: Vec3, direction: Vec2, steps: &PlayerSteps, tuning: &StepUpTuning) -> Self {
        let across = Vec3::new(direction.x, 0.0, direction.y);
        let above = Vec3::Y * (steps.authored.cone_height_m + tuning.probe_above_m);
        let ahead = across * (steps.narrowed.half_width_m + tuning.probe_ahead_m);
        Self {
            clear_from: feet + above,
            clear_to: feet + above + ahead,
            down_from: feet + above + ahead,
            down_to: feet + ahead + Vec3::Y * steps.narrowed.cone_height_m,
        }
    }
}

/// The height above the feet at `feet_y` of a top hit at `hit_y` with a normal of height
/// `normal_y`, when the narrowed hull cannot step up it and the authored one can, and it is
/// walkable.
fn step_height(
    feet_y: f32,
    hit_y: f32,
    normal_y: f32,
    steps: &PlayerSteps,
    tuning: &StepUpTuning,
) -> Option<f32> {
    let height = hit_y - feet_y;
    (height.is_finite()
        && normal_y >= tuning.min_top_normal_y
        && height > steps.narrowed.cone_height_m
        && height <= steps.authored.cone_height_m)
        .then_some(height)
}

/// The lift that raises the feet at `feet_y` to above a step's top at `top_y` within the lift's
/// time, metres per second; `None` once they are above it.
fn lift_speed(feet_y: f32, top_y: f32, step_s: f32, tuning: &StepUpTuning) -> Option<f32> {
    let remaining = top_y + tuning.lift_margin_m - feet_y;
    (remaining > 0.0 && remaining.is_finite() && step_s > 0.0)
        .then(|| (remaining / tuning.lift_time_s.max(step_s)).min(tuning.max_lift_mps))
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Lift {
    top_y: f32,
    elapsed_s: f32,
}

struct State {
    lift: Option<Lift>,
    status: StepUpStatus,
}

impl State {
    /// Give the proxy its own velocity, rising at least at `speed`.
    ///
    /// # Safety
    ///
    /// As [`step`].
    unsafe fn apply(&mut self, current: Vec3, speed: f32) {
        match unsafe { proxy::set_velocity(lifted(current, speed)) } {
            Ok(()) => self.status.lifting = true,
            Err(error) => {
                tracing::debug!(%error, "human hull: the step up could not lift the body");
                self.finish();
            }
        }
    }

    fn finish(&mut self) {
        self.lift = None;
        self.status.lifting = false;
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    lift: None,
    status: StepUpStatus {
        enabled: true,
        lifting: false,
        steps: 0,
        last_height_m: None,
    },
});

/// Whether the player is on foot on the ground, not climbing with the hands.
///
/// # Safety
///
/// As [`step`].
unsafe fn on_foot() -> bool {
    if !jc3vr_core::hooks::in_gameplay() || climb::climbing() {
        return false;
    }
    let detection = mode::current();
    matches!(detection.mode, Mode::OnFoot(OnFootVariant::Ground)) && detection.traversal.is_none()
}

/// The local player's last measured velocity across the ground, world `(x, z)`.
///
/// # Safety
///
/// As [`step`].
unsafe fn measured_velocity() -> Vec2 {
    let Some(character) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        return Vec2::NAN;
    };
    let [x, _, z] = character.m_MeasuredWorldVelocity.data;
    Vec2::new(x, z)
}

/// Whether nothing solid lies between `from` and `to`, for the aim's ray filter, which never hits
/// the player. A cast that fails counts as blocked.
///
/// # Safety
///
/// As [`step`].
unsafe fn clear(from: Vec3, to: Vec3) -> bool {
    let Some(ray) = Ray::new(from, to - from) else {
        return false;
    };
    // SAFETY: forwarded from the caller; the player is live.
    let options = unsafe { CastOptions::ignoring_player(0) };
    matches!(
        unsafe { raycast::cast(&ray, from.distance(to), &options) },
        Ok(None)
    )
}

/// The first hit between `from` and `to`.
///
/// # Safety
///
/// As [`step`].
unsafe fn cast(from: Vec3, to: Vec3) -> Option<raycast::RayHit> {
    let ray = Ray::new(from, to - from)?;
    // SAFETY: forwarded from the caller; the player is live.
    let options = unsafe { CastOptions::ignoring_player(0) };
    unsafe { raycast::cast(&ray, from.distance(to), &options) }.ok()?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> &'static StepUpTuning {
        &HUMAN_HULL.get().step_up
    }

    fn steps() -> PlayerSteps {
        PlayerSteps {
            narrowed: StepProfile {
                cone_height_m: 0.225,
                half_width_m: 0.235,
            },
            authored: StepProfile {
                cone_height_m: 0.45,
                half_width_m: 0.46,
            },
        }
    }

    #[test]
    fn only_a_player_stopped_while_driven_looks_for_a_step() {
        let tuning = tuning();
        let wanted = Vec2::new(0.0, tuning.min_wanted_mps * 8.0);
        let along = |share: f32| Vec2::new(0.6, wanted.y * share);
        assert_eq!(
            blocked(wanted, along(tuning.blocked_share * 0.5), tuning),
            Some(Vec2::Y)
        );
        assert_eq!(
            blocked(wanted, along((1.0 + tuning.blocked_share) * 0.5), tuning),
            None
        );
        let slow = Vec2::new(tuning.min_wanted_mps * 0.5, 0.0);
        assert_eq!(blocked(slow, Vec2::ZERO, tuning), None);
        assert_eq!(blocked(wanted, Vec2::NAN, tuning), None);
    }

    #[test]
    fn the_probes_look_past_the_narrowed_edge_from_above_the_authored_cone() {
        let tuning = tuning();
        let steps = steps();
        let feet = Vec3::new(10.0, 2.0, 5.0);
        let probes = Probes::ahead(feet, Vec2::X, &steps, tuning);
        let ahead = feet.x + steps.narrowed.half_width_m + tuning.probe_ahead_m;
        let above = feet.y + steps.authored.cone_height_m + tuning.probe_above_m;
        assert!(
            probes
                .down_from
                .abs_diff_eq(Vec3::new(ahead, above, feet.z), 1e-5)
        );
        assert!(probes.down_to.abs_diff_eq(
            Vec3::new(ahead, feet.y + steps.narrowed.cone_height_m, feet.z),
            1e-5
        ));
        assert!(
            probes
                .clear_from
                .abs_diff_eq(Vec3::new(feet.x, above, feet.z), 1e-5)
        );
        assert_eq!(probes.clear_to, probes.down_from);
    }

    #[test]
    fn a_step_is_lifted_onto_between_the_narrowed_and_the_authored_cones() {
        let (steps, tuning) = (steps(), tuning());
        // A kerb the narrowed cone steps up on its own.
        assert_eq!(step_height(0.0, 0.15, 1.0, &steps, tuning), None);
        // A step only the authored cone would step up.
        assert_eq!(step_height(0.0, 0.35, 1.0, &steps, tuning), Some(0.35));
        // A ledge the authored hull would not step up either, or a top too steep to stand on.
        assert_eq!(step_height(0.0, 0.5, 1.0, &steps, tuning), None);
        let steep = tuning.min_top_normal_y * 0.5;
        assert_eq!(step_height(0.0, 0.35, steep, &steps, tuning), None);
    }

    #[test]
    fn the_lift_reaches_the_top_within_its_time_and_stops_above_it() {
        let tuning = tuning();
        let step_s = 1.0 / 30.0;
        let speed = lift_speed(0.0, 0.3, step_s, tuning).unwrap();
        let expected = (0.3 + tuning.lift_margin_m) / tuning.lift_time_s.max(step_s);
        assert!((speed - expected.min(tuning.max_lift_mps)).abs() < 1e-5);
        assert_eq!(
            lift_speed(0.0, 100.0, step_s, tuning),
            Some(tuning.max_lift_mps)
        );
        let above = 0.3 + tuning.lift_margin_m + 0.01;
        assert_eq!(lift_speed(above, 0.3, step_s, tuning), None);
    }
}
