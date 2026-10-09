//! The swim heading's telemetry: once a second while the local player swims, an info line with
//! what steers the body and what the body did, so a swim that curves away from where the player
//! points it can be read from the log. The swim cores' heading rotation records its inputs and
//! output ([`record_rotate`]); the input tick logs ([`log_tick`]).
//!
//! The line carries the body-yaw target, the body's heading read two ways (the euler yaw the
//! accumulator clamps against and the ground projection of its forward, with the forward's height,
//! which together show a body frame pitched onto its gimbal lock), how fast the body and the target
//! turned since the last line, the turn input, the swim counters' advance, the core's last rotation
//! (its own target and what it returned), and the procedural body's swim torso and the hand tiers.

use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use glam::Vec3;
use parking_lot::Mutex;

use crate::{
    controllers::{body::procedural, hands::tiers},
    headpose::{self, sim::body_yaw_of},
    hooks::input::swim::{
        SWIM_BACKWARD_REFUSALS, SWIM_DIRECTION_OVERRIDES, SWIM_EVAL_CALLS, SWIM_ROTATE_OVERRIDES,
        SWIM_TURN_SUPPRESSIONS, SWIM_UNTARGETED, SwimCore, swimming,
    },
};

/// Record one heading rotation of a swim core: the body's forward before it (`from`), the core's
/// own target (`game_to`, the camera-relative move direction underwater), the target it was given
/// (`given_to`, the body-yaw target while engaged), and its output.
pub(super) fn record_rotate(core: SwimCore, from: Vec3, game_to: Vec3, given_to: Vec3, out: Vec3) {
    *LAST_ROTATE.lock() = Some(RotateSample {
        core,
        from,
        game_to,
        given_to,
        out,
    });
}

/// Log the swim heading's state, at most once per [`PERIOD`], while the local player swims.
/// `look_x` is the turn input the body-yaw accumulator took this tick. Called from the input tick
/// on the game thread, after the headpose has advanced, holding no lock.
pub(crate) fn log_tick(look_x: f32) {
    if !swimming() {
        *STATE.lock() = TelemetryState::NEW;
        return;
    }
    let now = Instant::now();
    let counters = Counters::now();
    let body = headpose::xr::body_rotation_raw();
    let body_forward = body * Vec3::NEG_Z;
    let body_euler_yaw = body_yaw_of(body);
    let target = headpose::body_yaw_target().map(heading_of);
    let mut state = STATE.lock();
    state.look_x_peak = state.look_x_peak.max(look_x.abs());
    let due = state
        .logged
        .is_none_or(|(at, ..)| now.saturating_duration_since(at) >= PERIOD);
    if !due {
        return;
    }
    let look_x_peak = std::mem::take(&mut state.look_x_peak);
    let previous = state
        .logged
        .replace((now, counters, body_euler_yaw, target));
    drop(state);
    let Some((at, before, body_before, target_before)) = previous else {
        return;
    };
    let elapsed = now
        .saturating_duration_since(at)
        .as_secs_f32()
        .max(f32::EPSILON);
    let rate = |now: f32, then: f32| wrap_deg((now - then).to_degrees()) / elapsed;
    let advanced = counters.since(&before);
    let rotate = *LAST_ROTATE.lock();
    let procedural = procedural::published_status();
    let tiers = tiers::status();
    tracing::info!(
        target: "swim",
        target_yaw_deg = target.map(f32::to_degrees),
        target_turn_deg_s = target.zip(target_before).map(|(now, then)| rate(now, then)),
        body_yaw_deg = body_euler_yaw.to_degrees(),
        body_heading_deg = heading_of(body_forward).to_degrees(),
        body_forward_y = body_forward.y,
        body_turn_deg_s = rate(body_euler_yaw, body_before),
        look_x,
        look_x_peak,
        cores = advanced.evals,
        direction_overrides = advanced.directions,
        rotate_overrides = advanced.rotates,
        turn_suppressions = advanced.turns,
        backward_refusals = advanced.backward,
        untargeted = advanced.untargeted,
        core = rotate.map(|r| r.core.label()),
        core_from_deg = rotate.map(|r| heading_of(r.from).to_degrees()),
        core_own_target_deg = rotate.map(|r| heading_of(r.game_to).to_degrees()),
        core_own_target_y = rotate.map(|r| r.game_to.y),
        core_given_target_deg = rotate.map(|r| heading_of(r.given_to).to_degrees()),
        core_out_deg = rotate.map(|r| heading_of(r.out).to_degrees()),
        core_out_y = rotate.map(|r| r.out.y),
        body_case = procedural.case.label(),
        torso_weight = procedural.weight,
        legs_weight = procedural.legs_weight,
        hand_tiers = ?tiers.effective,
        hand_weights = ?tiers.weights,
        "swim: the heading"
    );
}

/// How often the line is logged while swimming.
const PERIOD: Duration = Duration::from_secs(1);

/// The ground-plane heading of `direction`, radians, as the headpose's yaws are: about +Y from -Z.
/// Zero for a vertical direction.
fn heading_of(direction: Vec3) -> f32 {
    if direction.x == 0.0 && direction.z == 0.0 {
        return 0.0;
    }
    (-direction.x).atan2(-direction.z)
}

/// `degrees` wrapped into `-180..=180`.
fn wrap_deg(degrees: f32) -> f32 {
    let wrapped = (degrees + 180.0).rem_euclid(360.0) - 180.0;
    if wrapped == -180.0 { 180.0 } else { wrapped }
}

#[derive(Debug, Copy, Clone)]
struct RotateSample {
    core: SwimCore,
    from: Vec3,
    game_to: Vec3,
    given_to: Vec3,
    out: Vec3,
}

static LAST_ROTATE: Mutex<Option<RotateSample>> = Mutex::new(None);

/// The swim counters at one moment.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Counters {
    evals: u64,
    directions: u64,
    rotates: u64,
    turns: u64,
    backward: u64,
    untargeted: u64,
}

impl Counters {
    fn now() -> Self {
        Self {
            evals: SWIM_EVAL_CALLS.load(Ordering::Relaxed),
            directions: SWIM_DIRECTION_OVERRIDES.load(Ordering::Relaxed),
            rotates: SWIM_ROTATE_OVERRIDES.load(Ordering::Relaxed),
            turns: SWIM_TURN_SUPPRESSIONS.load(Ordering::Relaxed),
            backward: SWIM_BACKWARD_REFUSALS.load(Ordering::Relaxed),
            untargeted: SWIM_UNTARGETED.load(Ordering::Relaxed),
        }
    }

    fn since(&self, before: &Self) -> Self {
        Self {
            evals: self.evals.wrapping_sub(before.evals),
            directions: self.directions.wrapping_sub(before.directions),
            rotates: self.rotates.wrapping_sub(before.rotates),
            turns: self.turns.wrapping_sub(before.turns),
            backward: self.backward.wrapping_sub(before.backward),
            untargeted: self.untargeted.wrapping_sub(before.untargeted),
        }
    }
}

struct TelemetryState {
    /// When the last line was logged, with the counters, the body's euler yaw, and the target's
    /// heading then.
    logged: Option<(Instant, Counters, f32, Option<f32>)>,
    /// The largest turn input since the last line.
    look_x_peak: f32,
}

impl TelemetryState {
    const NEW: TelemetryState = TelemetryState {
        logged: None,
        look_x_peak: 0.0,
    };
}

static STATE: Mutex<TelemetryState> = Mutex::new(TelemetryState::NEW);

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    #[test]
    fn the_heading_matches_the_headpose_yaw() {
        for yaw in [-2.5_f32, -1.0, 0.0, 0.4, 3.0] {
            let forward = Quat::from_rotation_y(yaw) * Vec3::NEG_Z;
            assert!((heading_of(forward) - yaw).abs() < 1e-5, "{yaw}");
            // A pitched forward keeps its heading.
            let pitched = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-0.6) * Vec3::NEG_Z;
            assert!((heading_of(pitched) - yaw).abs() < 1e-5, "{yaw}");
        }
        assert_eq!(heading_of(Vec3::Y), 0.0);
    }

    #[test]
    fn turn_rates_wrap_across_the_seam() {
        assert!((wrap_deg(350.0) + 10.0).abs() < 1e-4);
        assert!((wrap_deg(-350.0) - 10.0).abs() < 1e-4);
        assert_eq!(wrap_deg(180.0), 180.0);
        assert_eq!(wrap_deg(-180.0), 180.0);
    }
}
