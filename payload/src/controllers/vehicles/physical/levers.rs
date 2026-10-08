//! The levers' motion: a lever's handle follows the holding hand along its axis, with a detent
//! around neutral. The throttle stays where it is let go, and has an afterburner gate at the top of
//! its travel.
//!
//! A plane burns its nitrous while its thrust input is above
//! [`AirPlane::NITROUS_THRUST_THRESHOLD`]: there is no input of its own. A throttle that ran
//! straight to full would light it in the last stretch of every push, so with the gate the throttle
//! stops just short of that thrust below the gate, and is full only through it. The hand finds the
//! gate by the cue it gives going in and coming out.

use glam::Vec3;
use jc3gi::vehicle::vehicle::AirPlane;

use crate::controllers::vehicles::{
    config::LeverSpec,
    physical::{
        ControlKind, ControlState, Holding, Placement,
        cues::{self, ControlCue, TravelCues},
    },
};

/// A lever's position, `-1..=1`, for its handle pulled to `handle_now`.
pub fn lever_value(spec: &LeverSpec, rest: Vec3, handle_now: Vec3) -> f32 {
    let half = (spec.travel_m * 0.5).max(1e-3);
    let along = (handle_now - rest).dot(spec.axis.direction()) / half;
    if along.is_finite() {
        along.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// A lever's handle at position `value`.
pub fn lever_handle(spec: &LeverSpec, rest: Vec3, value: f32) -> Vec3 {
    rest + spec.axis.direction() * (spec.travel_m * 0.5 * value)
}

/// `value` with a detent around zero, rescaled so that it still reaches one.
pub fn detent(value: f32, detent: f32) -> f32 {
    let detent = detent.clamp(0.0, 0.95);
    let magnitude = value.abs();
    if magnitude <= detent {
        return 0.0;
    }
    value.signum() * ((magnitude - detent) / (1.0 - detent)).min(1.0)
}

/// What a lever at `value` outputs, with a detent of `detent` around neutral and its gate starting
/// at `gate`, if it has one: below the gate the positive half reaches [`DRY_FULL`], and in the gate
/// it is full.
pub fn lever_output(value: f32, detent_share: f32, gate: Option<f32>) -> f32 {
    let Some(gate) = gate.filter(|_| value > 0.0) else {
        return detent(value, detent_share);
    };
    if value >= gate {
        return 1.0;
    }
    let detent_share = detent_share.clamp(0.0, 0.95);
    let span = (gate - detent_share).max(1e-3);
    ((value - detent_share) / span).clamp(0.0, 1.0) * DRY_FULL
}

/// The highest thrust the throttle gives below its gate: a little under the thrust that lights the
/// afterburner, so that the game's threshold is never reached by the dry throttle.
pub const DRY_FULL: f32 = AirPlane::NITROUS_THRUST_THRESHOLD - 0.02;

/// What a lever produced this tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct LeverStep {
    pub(super) value: Option<f32>,
    pub(super) cue: Option<ControlCue>,
}

/// Move `kind`'s lever with the hand holding it. Let go, a throttle stays where it is and keeps
/// writing its value, except on a tick on which the tables write the same axis (`tables_move`):
/// the trigger then flies the vehicle, and the lever takes over again once the trigger is let go.
pub(super) fn step_lever(
    state: &mut ControlState,
    placement: &Placement,
    kind: ControlKind,
    holding: Holding,
    tables_move: bool,
) -> LeverStep {
    let Some((index, spec)) = placement.lever(kind) else {
        return LeverStep::default();
    };
    let gate = placement.gate(kind);
    let mut cue = None;
    match holding.into_iter().flatten().next() {
        Some((hold, point)) => {
            let value = lever_value(&spec, placement.mirror(spec.rest_m), point - hold.offset);
            state.levers[index] = value;
            cue = cues::lever_cue(&mut state.lever_cues[index], value, gate);
        }
        None if !spec.hold_on_release => {
            state.levers[index] = 0.0;
            state.lever_cues[index] = TravelCues::new();
            return LeverStep::default();
        }
        None if state.levers[index] == 0.0 || tables_move => return LeverStep::default(),
        None => {}
    }
    LeverStep {
        value: Some(lever_output(state.levers[index], spec.detent, gate)),
        cue,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dry_throttle_stays_short_of_the_afterburner_and_the_gate_is_full() {
        let gate = Some(0.85);
        assert_eq!(lever_output(0.05, 0.1, gate), 0.0);
        let below = lever_output(0.84, 0.1, gate);
        assert!(below < AirPlane::NITROUS_THRUST_THRESHOLD, "{below}");
        assert!(below > 0.8, "{below}");
        assert!(lever_output(0.5, 0.1, gate) < below);
        assert_eq!(lever_output(0.85, 0.1, gate), 1.0);
        assert_eq!(lever_output(1.0, 0.1, gate), 1.0);
        // The brake half is the plain lever's.
        assert_eq!(lever_output(-1.0, 0.1, gate), -1.0);
        assert_eq!(lever_output(-0.05, 0.1, gate), 0.0);
    }

    #[test]
    fn without_a_gate_the_lever_runs_straight_to_full() {
        assert_eq!(lever_output(1.0, 0.1, None), 1.0);
        assert!((lever_output(0.55, 0.1, None) - 0.5).abs() < 1e-6);
    }
}
