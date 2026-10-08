//! The sticks' motion: a stick's top follows the holding hand across the horizontal plane within
//! its travel, and let go, a centring spring returns it.
//!
//! The grab is relative: the hand's offset from the top is taken when it grabs, wherever the top is
//! then, so a hand that takes the stick off-centre, or takes it again on its way back to centre,
//! moves it only by moving. The flight stick also works the rudder by its twist about the shaft,
//! measured from the hand's orientation when it took the stick, so a wrist held at any angle
//! starts from no rudder.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec2, Vec3};

use crate::controllers::{
    frame::Hand,
    vehicles::{
        config::{FlightControlsConfig, StickSpec},
        physical::{
            ControlKind, ControlState, Holding, Placement,
            cues::{self, ControlCue, TravelCues},
        },
    },
};

/// A stick's deflection (+X right, +Y back) for its top pulled to `top_now`, limited to the full
/// deflection.
pub fn stick_deflection(spec: &StickSpec, top: Vec3, top_now: Vec3) -> Vec2 {
    let d = top_now - top;
    let flat = Vec2::new(d.x, d.z);
    if !flat.is_finite() {
        return Vec2::ZERO;
    }
    flat.clamp_length_max(spec.max_deflection_m.max(0.0))
}

/// What a stick deflected by `deflection` outputs (+X rolls right, +Y pushes forward): its share of
/// the full deflection past the deadzone, rescaled so that it still reaches one.
pub fn stick_value(spec: &StickSpec, deflection: Vec2) -> Vec2 {
    let max = spec.max_deflection_m.max(1e-3);
    let share = Vec2::new(deflection.x, -deflection.y) / max;
    let length = share.length();
    let deadzone = spec.deadzone.clamp(0.0, 0.9);
    if !length.is_finite() || length <= deadzone {
        return Vec2::ZERO;
    }
    share / length * ((length - deadzone) / (1.0 - deadzone)).min(1.0)
}

/// The released stick's deflection after `dt` seconds of its centring spring.
pub fn spring_back(spec: &StickSpec, deflection: Vec2, dt: f32) -> Vec2 {
    if spec.return_s <= 0.0 || !deflection.is_finite() {
        return Vec2::ZERO;
    }
    let next = deflection * (-dt.max(0.0) / spec.return_s).exp();
    if next.length() < SETTLED_M {
        Vec2::ZERO
    } else {
        next
    }
}

/// The rudder, `-1..=1` with positive right, for a hand that took the stick at orientation `grab`
/// and holds it at `now`: its twist about the shaft (the cockpit frame's vertical), turning the top
/// to the right for a right rudder, past the deadzone and fully at the full twist.
pub fn twist_rudder(config: &FlightControlsConfig, grab: Quat, now: Quat) -> f32 {
    let twist = -twist_about(now * grab.inverse(), Vec3::Y);
    let deadzone = config.twist_deadzone_deg.to_radians().max(0.0);
    let full = config.twist_full_deg.to_radians().max(deadzone + 1e-3);
    let past = (twist.abs() - deadzone).max(0.0);
    twist.signum() * (past / (full - deadzone)).min(1.0)
}

pub(super) fn deflection3(d: Vec2) -> Vec3 {
    Vec3::new(d.x, 0.0, d.y)
}

/// What a stick produced this tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct StickStep {
    pub(super) value: Option<Vec2>,
    pub(super) rudder: Option<f32>,
    pub(super) cue: Option<ControlCue>,
}

/// Move `kind`'s stick with the hand holding it, or let go, spring it back to centre, still writing
/// its value on the way so the vehicle comes back with it. A tick on which the tables write the
/// stick's axes (`tables_move`) snaps a returning stick to centre, so the thumbstick flies at once.
pub(super) fn step_stick(
    state: &mut ControlState,
    placement: &Placement,
    kind: ControlKind,
    holding: Holding,
    (orientations, tables_move): ([Option<Quat>; 2], bool),
    dt: f32,
) -> StickStep {
    let Some(spec) = placement.stick(kind) else {
        return StickStep::default();
    };
    let held = Hand::BOTH
        .into_iter()
        .find_map(|hand| holding[hand.index()].map(|(hold, point)| (hand, hold, point)));
    let Some((hand, hold, point)) = held else {
        if state.stick == Vec2::ZERO {
            return StickStep::default();
        }
        if tables_move {
            state.stick = Vec2::ZERO;
            state.stick_cues = TravelCues::new();
            return StickStep::default();
        }
        state.stick = spring_back(&spec, state.stick, dt);
        return StickStep {
            value: Some(stick_value(&spec, state.stick)),
            ..StickStep::default()
        };
    };
    let top = placement.mirror(spec.top_m);
    state.stick = stick_deflection(&spec, top, point - hold.offset);
    let share = state.stick.length() / spec.max_deflection_m.max(1e-3);
    let flight = &placement.config.flight;
    let rudder = (kind == ControlKind::FlightStick && flight.twist_rudder)
        .then(|| hold.orientation.zip(orientations[hand.index()]))
        .flatten()
        .map(|(grab, now)| twist_rudder(flight, grab, now));
    StickStep {
        value: Some(stick_value(&spec, state.stick)),
        rudder,
        cue: cues::stick_cue(&mut state.stick_cues, share),
    }
}

/// A returning stick closer to centre than this, metres, is at centre.
const SETTLED_M: f32 = 0.002;

/// The rotation `delta`'s twist about the unit `axis`, radians, positive anticlockwise looking down
/// the axis toward its origin.
pub(super) fn twist_about(delta: Quat, axis: Vec3) -> f32 {
    let delta = if delta.w < 0.0 { -delta } else { delta };
    let along = Vec3::new(delta.x, delta.y, delta.z).dot(axis);
    let twist = 2.0 * along.atan2(delta.w);
    if twist.is_finite() {
        (twist + PI).rem_euclid(TAU) - PI
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::vehicles::config::PhysicalControlsConfig;

    fn spec() -> StickSpec {
        PhysicalControlsConfig::new().flight_stick
    }

    #[test]
    fn a_resting_hand_s_tremor_flies_nothing_and_full_deflection_is_full() {
        let spec = spec();
        let max = spec.max_deflection_m;
        assert_eq!(
            stick_value(&spec, Vec2::new(max * spec.deadzone * 0.5, 0.0)),
            Vec2::ZERO
        );
        let full = stick_value(&spec, Vec2::new(0.0, -max));
        assert!((full - Vec2::new(0.0, 1.0)).length() < 1e-5, "{full}");
        let half = stick_value(&spec, Vec2::new(max * 0.5, 0.0));
        assert!(half.x > 0.4 && half.x < 0.5, "{half}");
    }

    #[test]
    fn the_spring_brings_the_stick_back_to_centre() {
        let spec = spec();
        let mut d = Vec2::new(spec.max_deflection_m, 0.0);
        let after_one_tick = spring_back(&spec, d, 1.0 / 30.0);
        assert!(after_one_tick.x < d.x && after_one_tick.x > 0.0);
        for _ in 0..30 {
            d = spring_back(&spec, d, 1.0 / 30.0);
        }
        assert_eq!(d, Vec2::ZERO);
        let snap = StickSpec {
            return_s: 0.0,
            ..spec
        };
        assert_eq!(spring_back(&snap, Vec2::X, 0.01), Vec2::ZERO);
    }

    #[test]
    fn twisting_the_top_right_is_right_rudder_past_the_deadzone() {
        let config = FlightControlsConfig::new();
        let grab = Quat::from_rotation_x(0.4);
        let small = Quat::from_rotation_y(-(config.twist_deadzone_deg * 0.5).to_radians()) * grab;
        assert_eq!(twist_rudder(&config, grab, small), 0.0);
        // Turning the top clockwise seen from above, to the right.
        let right = Quat::from_rotation_y(-config.twist_full_deg.to_radians()) * grab;
        assert!((twist_rudder(&config, grab, right) - 1.0).abs() < 1e-4);
        let left = Quat::from_rotation_y(0.3) * grab;
        assert!(twist_rudder(&config, grab, left) < 0.0);
        // Tilting the wrist about the other axes is no rudder.
        let tilted = Quat::from_rotation_x(0.5) * grab;
        assert!(twist_rudder(&config, grab, tilted).abs() < 1e-4);
    }
}
