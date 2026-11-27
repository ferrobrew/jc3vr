//! The levers' motion: a lever's handle follows the holding hand along its axis, with a detent
//! around neutral. The throttle stays where it is let go, and has an afterburner gate at the top of
//! its travel.
//!
//! The throttle's travel is split by a gate at zero: in front of it, the forward section runs from
//! no thrust to full; behind it, a short reverse section runs from nothing to full reverse. Pulled
//! back to zero, the throttle stops there; only a hand that pulls on past zero by
//! [`FlightControlsTuning::zero_pull`] takes it through into reverse, and the same push forward
//! takes it back out. Next to zero, a share of either section snaps to it, so a throttle let go
//! near zero sits at zero rather than giving a little thrust. The held throttle ticks every step of
//! its forward section, stops hard at zero, and buzzes going through the gate.
//!
//! A plane burns its nitrous while its thrust input is above
//! [`AirPlane::NITROUS_THRUST_THRESHOLD`]: there is no input of its own. A throttle that ran
//! straight to full would light it in the last stretch of every push, so with the gate the throttle
//! stops just short of that thrust below the gate, and is full only through it. The hand finds the
//! gate by the cue it gives going in and coming out.

use jc3gi::vehicle::vehicle::AirPlane;
use jc3vr_body::controllers::grab::cockpit::ControlKind;

use crate::vehicles::{
    driving::physical::{
        ControlState, Holding, Placement, cues,
        cues::{ControlCue, ThrottleCues, ThrottleMove, TravelCues},
    },
    tuning::FlightControlsTuning,
};

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
pub(crate) fn lever_output(value: f32, detent_share: f32, gate: Option<f32>) -> f32 {
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
pub(crate) const DRY_FULL: f32 = AirPlane::NITROUS_THRUST_THRESHOLD - 0.02;

/// The throttle's zero, as a lever position along its whole travel (`-1` at the back stop, `1` at
/// the front): the gate between the reverse section behind it and the forward section in front.
pub(crate) fn throttle_zero(flight: &FlightControlsTuning) -> f32 {
    -1.0 + 2.0 * reverse_share(flight)
}

/// The side of its zero gate the throttle is on.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ThrottleSection {
    #[default]
    Forward,
    Reverse,
}

/// The throttle's gate and its cues.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ThrottleState {
    pub section: ThrottleSection,
    pub(super) cues: ThrottleCues,
}

impl ThrottleState {
    pub const fn new() -> Self {
        Self {
            section: ThrottleSection::Forward,
            cues: ThrottleCues::new(),
        }
    }
}

/// Where the throttle sits, as an offset from its zero in lever positions, for a hand that would
/// put it at `hand` (the same measure), while it is in `section`. The hand moves it freely within
/// its section; at zero the throttle stops until the hand is past zero by the gate's pull, which
/// takes it through into the other section. Returns the offset and the section it is then in.
pub(crate) fn throttle_constrain(
    flight: &FlightControlsTuning,
    section: ThrottleSection,
    hand: f32,
) -> (f32, ThrottleSection) {
    if !hand.is_finite() {
        return (0.0, section);
    }
    let zero = throttle_zero(flight);
    let hand = hand.clamp(-1.0 - zero, 1.0 - zero);
    let pull = flight.zero_pull.clamp(0.0, 0.5) * 2.0;
    let has_reverse = reverse_share(flight) > 0.0;
    match section {
        ThrottleSection::Forward if hand >= 0.0 => (hand, section),
        ThrottleSection::Forward if hand < -pull && has_reverse => (hand, ThrottleSection::Reverse),
        ThrottleSection::Forward => (0.0, section),
        ThrottleSection::Reverse if hand <= 0.0 => (hand, section),
        ThrottleSection::Reverse if hand > pull => (hand, ThrottleSection::Forward),
        ThrottleSection::Reverse => (0.0, section),
    }
}

/// The shares of the forward and the reverse sections the throttle is at for an offset `offset`
/// from its zero, each `0..=1`.
pub(crate) fn throttle_shares(flight: &FlightControlsTuning, offset: f32) -> (f32, f32) {
    let zero = throttle_zero(flight);
    let forward = (offset / (1.0 - zero).max(1e-3)).clamp(0.0, 1.0);
    let reverse = (-offset / (1.0 + zero).max(1e-3)).clamp(0.0, 1.0);
    if offset.is_finite() {
        (forward, reverse)
    } else {
        (0.0, 0.0)
    }
}

/// What the throttle at `offset` from its zero outputs, `-1..=1`: nothing within the snap next to
/// zero, then forward thrust rising to full, reaching [`DRY_FULL`] below the afterburner gate at
/// `gate` along the forward section, if it has one, and full in it; behind zero, reverse rising to
/// full.
pub(crate) fn throttle_output(
    flight: &FlightControlsTuning,
    gate: Option<f32>,
    offset: f32,
) -> f32 {
    let snap = flight.zero_snap.clamp(0.0, 0.5);
    let (forward, reverse) = throttle_shares(flight, offset);
    if reverse > 0.0 {
        return -((reverse - snap) / (1.0 - snap)).clamp(0.0, 1.0);
    }
    match gate {
        Some(gate) if forward >= gate => 1.0,
        Some(gate) => ((forward - snap) / (gate - snap).max(1e-3)).clamp(0.0, 1.0) * DRY_FULL,
        None => ((forward - snap) / (1.0 - snap)).clamp(0.0, 1.0),
    }
}

fn reverse_share(flight: &FlightControlsTuning) -> f32 {
    if flight.reverse_share.is_finite() {
        flight.reverse_share.clamp(0.0, 0.9)
    } else {
        0.0
    }
}

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
    if kind == ControlKind::Throttle {
        return step_throttle(state, placement, holding, tables_move);
    }
    let Some((index, spec)) = placement.lever(kind) else {
        return LeverStep::default();
    };
    let gate = placement.gate(kind);
    let mut cue = None;
    match holding.into_iter().flatten().next() {
        Some((hold, point)) => {
            let value = placement.lever_value(kind, &spec, point - hold.offset);
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

/// Move the throttle with the hand holding it, through its zero gate only on a deliberate pull.
/// Let go, it stays where it is, but snaps to zero within the snap next to it.
fn step_throttle(
    state: &mut ControlState,
    placement: &Placement,
    holding: Holding,
    tables_move: bool,
) -> LeverStep {
    let Some((index, spec)) = placement.lever(ControlKind::Throttle) else {
        return LeverStep::default();
    };
    let flight = &placement.config.flight;
    let gate = placement.gate(ControlKind::Throttle);
    let mut cue = None;
    match holding.into_iter().flatten().next() {
        Some((hold, point)) => {
            let hand = placement.lever_value(ControlKind::Throttle, &spec, point - hold.offset);
            let before = state.throttle.section;
            let (offset, section) = throttle_constrain(flight, before, hand);
            state.levers[index] = offset;
            state.throttle.section = section;
            let (forward, reverse) = throttle_shares(flight, offset);
            cue = cues::throttle_cue(
                &mut state.throttle.cues,
                ThrottleMove {
                    forward,
                    reverse,
                    crossed: section != before,
                },
                gate,
                flight.thrust_step,
            );
        }
        None => {
            let (forward, reverse) = throttle_shares(flight, state.levers[index]);
            if forward.max(reverse) < flight.zero_snap {
                state.levers[index] = 0.0;
            }
            state.throttle.cues = ThrottleCues::new();
            if state.levers[index] == 0.0 || tables_move {
                return LeverStep::default();
            }
        }
    }
    LeverStep {
        value: Some(throttle_output(flight, gate, state.levers[index])),
        cue,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicles::tuning::PhysicalControlsTuning;

    #[test]
    fn the_throttle_stops_at_zero_until_pulled_through_into_reverse() {
        let flight = PhysicalControlsTuning::shipped().flight;
        let pull = flight.zero_pull * 2.0;
        let forward = ThrottleSection::Forward;
        assert_eq!(throttle_constrain(&flight, forward, 0.4), (0.4, forward));
        // A hand a little behind zero holds the throttle at zero.
        assert_eq!(
            throttle_constrain(&flight, forward, -pull * 0.5),
            (0.0, forward)
        );
        // Past the pull, it goes through into reverse, proportionally.
        let (offset, section) = throttle_constrain(&flight, forward, -pull * 1.5);
        assert_eq!(section, ThrottleSection::Reverse);
        assert!((offset + pull * 1.5).abs() < 1e-6);
        // In reverse, a little push forward holds it at zero, and a deliberate one takes it out.
        let reverse = ThrottleSection::Reverse;
        assert_eq!(
            throttle_constrain(&flight, reverse, pull * 0.5),
            (0.0, reverse)
        );
        assert_eq!(throttle_constrain(&flight, reverse, pull * 1.5).1, forward);
        // The travel ends at the stops.
        let zero = throttle_zero(&flight);
        assert_eq!(throttle_constrain(&flight, reverse, -5.0).0, -1.0 - zero);
        assert_eq!(throttle_constrain(&flight, forward, 5.0).0, 1.0 - zero);
    }

    #[test]
    fn a_third_of_the_travel_is_reverse_and_the_rest_forward() {
        let flight = PhysicalControlsTuning::shipped().flight;
        let zero = throttle_zero(&flight);
        assert!((zero - (-1.0 / 3.0)).abs() < 1e-6);
        assert_eq!(throttle_output(&flight, None, 1.0 - zero), 1.0);
        assert_eq!(throttle_output(&flight, None, -1.0 - zero), -1.0);
        assert_eq!(throttle_output(&flight, None, 0.0), 0.0);
        // Within the snap next to zero, nothing.
        let snap = flight.zero_snap * 0.5;
        assert_eq!(throttle_output(&flight, None, snap * (1.0 - zero)), 0.0);
        assert_eq!(throttle_output(&flight, None, -snap * (1.0 + zero)), 0.0);
        // Halfway through each section is about half.
        let half = throttle_output(&flight, None, 0.5 * (1.0 - zero));
        assert!((0.4..0.5).contains(&half), "{half}");
        let half = throttle_output(&flight, None, -0.5 * (1.0 + zero));
        assert!((-0.5..-0.4).contains(&half), "{half}");
    }

    #[test]
    fn without_a_reverse_section_the_zero_stop_is_the_end() {
        let flight = FlightControlsTuning {
            reverse_share: 0.0,
            ..PhysicalControlsTuning::shipped().flight
        };
        let (offset, section) = throttle_constrain(&flight, ThrottleSection::Forward, -1.0);
        assert_eq!((offset, section), (0.0, ThrottleSection::Forward));
    }

    #[test]
    fn the_dry_throttle_stays_short_of_the_afterburner_through_the_forward_section() {
        let flight = PhysicalControlsTuning::shipped().flight;
        let zero = throttle_zero(&flight);
        let at = |share: f32| throttle_output(&flight, Some(0.85), share * (1.0 - zero));
        let below = at(0.84);
        assert!(
            below < AirPlane::NITROUS_THRUST_THRESHOLD && below > 0.8,
            "{below}"
        );
        assert_eq!(at(0.85), 1.0);
        assert_eq!(at(1.0), 1.0);
    }

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
