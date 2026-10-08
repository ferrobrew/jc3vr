//! The haptic cues the physical controls give the hands holding them: the wheel's stop and centre,
//! and the sticks' and levers' stops, the levers' neutral, and the throttle's afterburner gate.

use crate::controllers::{
    frame::Hand,
    vehicles::{
        physical::{ControlCue, ControlKind, Outputs, buttons::ButtonEvent},
        steering::WheelCue,
    },
    weapons::{
        config::PulseSpec,
        haptics::{self, PulseSource},
    },
};

/// Pulse the hands holding the controls whose cues fired this tick, and the hands that pressed or
/// let go of a cockpit button or a twist grip. `holds` is the control each hand holds, indexed by
/// [`Hand::index`].
pub(super) fn pulse(outputs: &Outputs, holds: [Option<ControlKind>; 2], buttons: &[ButtonEvent]) {
    for event in buttons {
        let spec = if event.engages() {
            PRESS_PULSE
        } else {
            CENTRE_PULSE
        };
        haptics::pulse(event.hand(), spec, PulseSource::VehicleControl);
    }
    let holding = |kinds: &[ControlKind]| -> Vec<Hand> {
        Hand::BOTH
            .into_iter()
            .filter(|h| holds[h.index()].is_some_and(|k| kinds.contains(&k)))
            .collect()
    };
    if let Some(cue) = outputs.wheel_cue {
        let spec = match cue {
            WheelCue::Stop => STOP_PULSE,
            WheelCue::Centre => CENTRE_PULSE,
        };
        for hand in holding(&[ControlKind::Wheel]) {
            haptics::pulse(hand, spec, PulseSource::SteeringWheel);
        }
    }
    let cues = [
        (
            outputs.stick_cue,
            [ControlKind::Cyclic, ControlKind::FlightStick],
        ),
        (
            outputs.lever_cue,
            [ControlKind::Collective, ControlKind::Throttle],
        ),
    ];
    for (cue, kinds) in cues {
        let Some(cue) = cue else {
            continue;
        };
        for hand in holding(&kinds) {
            haptics::pulse(hand, control_pulse(cue), PulseSource::VehicleControl);
        }
    }
}

/// The bump the held wheel gives at its lock.
const STOP_PULSE: PulseSpec = PulseSpec {
    amplitude: 0.5,
    duration_s: 0.03,
};

/// The click of a cockpit button pressed, or a twist grip twisted far enough to act.
const PRESS_PULSE: PulseSpec = PulseSpec {
    amplitude: 0.45,
    duration_s: 0.025,
};

/// The light detent the held wheel gives on passing centre.
const CENTRE_PULSE: PulseSpec = PulseSpec {
    amplitude: 0.15,
    duration_s: 0.012,
};

/// The pulse a stick's or a lever's cue gives: a bump at a stop, a light detent at neutral, and a
/// firm click at the afterburner gate, which the hand has to find without looking.
const fn control_pulse(cue: ControlCue) -> PulseSpec {
    match cue {
        ControlCue::Stop => STOP_PULSE,
        ControlCue::Neutral => CENTRE_PULSE,
        ControlCue::Gate => PulseSpec {
            amplitude: 0.7,
            duration_s: 0.04,
        },
    }
}
