//! The haptic cues the physical controls give the hands holding them: the wheel's stop and centre,
//! the sticks' and levers' stops, the levers' neutral, the throttle's afterburner gate, and the
//! cockpit buttons.

use crate::controllers::{
    frame::Hand,
    haptics::{self, Event},
    vehicles::{
        physical::{ControlCue, ControlKind, Outputs, buttons::ButtonEvent},
        steering::WheelCue,
    },
};

/// Play the cues of the controls whose cues fired this tick on the hands holding them, and the
/// cues of the hands that pressed or let go of a cockpit button or a twist grip. `holds` is the
/// control each hand holds, indexed by [`Hand::index`].
pub(super) fn pulse(outputs: &Outputs, holds: [Option<ControlKind>; 2], buttons: &[ButtonEvent]) {
    for event in buttons {
        let cue = if event.engages() {
            Event::ButtonPress
        } else {
            Event::ButtonRelease
        };
        haptics::play(event.hand(), cue);
    }
    let holding = |kinds: &[ControlKind]| -> Vec<Hand> {
        Hand::BOTH
            .into_iter()
            .filter(|h| holds[h.index()].is_some_and(|k| kinds.contains(&k)))
            .collect()
    };
    if let Some(cue) = outputs.wheel_cue {
        let cue = match cue {
            WheelCue::Stop => Event::SteeringStop,
            WheelCue::Centre => Event::SteeringCentre,
        };
        for hand in holding(&[ControlKind::Wheel]) {
            haptics::play(hand, cue);
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
            haptics::play(hand, control_cue(cue));
        }
    }
}

/// The cue a stick's or a lever's cue plays: a bump at a stop, a tick at neutral, and a firm bump
/// at the afterburner gate, which the hand has to find without looking.
const fn control_cue(cue: ControlCue) -> Event {
    match cue {
        ControlCue::Stop => Event::ControlStop,
        ControlCue::Neutral => Event::ControlNeutral,
        ControlCue::Gate => Event::ThrottleGate,
    }
}
