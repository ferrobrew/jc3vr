//! The haptic stops and detents of the sticks and levers: each fires once as the control reaches
//! it, and is re-armed only once the control has left it by a margin, so that a hand resting at a
//! stop or wobbling about a detent does not buzz. Pure.

/// A cue a stick or a lever gives the hand holding it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ControlCue {
    /// The control reached the end of its travel.
    Stop,
    /// A lever passed through neutral: the throttle's idle, the collective's middle.
    Neutral,
    /// The throttle passed into or out of its afterburner gate.
    Gate,
}

impl ControlCue {
    pub const fn label(self) -> &'static str {
        match self {
            ControlCue::Stop => "stop",
            ControlCue::Neutral => "neutral",
            ControlCue::Gate => "gate",
        }
    }
}

/// Which of a control's cues may fire.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TravelCues {
    stop: bool,
    /// The side of neutral the control was on when it last left neutral by the margin, until it
    /// comes back to neutral or past it.
    neutral_side: Option<f32>,
    /// Whether the control came back to neutral or past it since the neutral cue last fired.
    neutral_due: bool,
    /// Whether the lever is in its gate, with the hysteresis the gate's way out has.
    in_gate: bool,
}

impl TravelCues {
    /// The arming of a control at neutral: the stop may fire, neutral once the control has left it.
    pub const fn new() -> Self {
        Self {
            stop: true,
            neutral_side: None,
            neutral_due: false,
            in_gate: false,
        }
    }
}

impl Default for TravelCues {
    fn default() -> Self {
        Self::new()
    }
}

/// The cue a lever gives on moving to `after` (`-1..=1`), with its gate starting at `gate` along
/// the positive half, if it has one. One cue fires a tick, the gate's first and then the stop's; a
/// neutral passed on the same tick cues on the next.
pub fn lever_cue(arming: &mut TravelCues, after: f32, gate: Option<f32>) -> Option<ControlCue> {
    if !after.is_finite() {
        return None;
    }
    if arming.neutral_side.is_some_and(|side| after * side <= 0.0) {
        arming.neutral_due = true;
        arming.neutral_side = None;
    }
    if after.abs() > REARM {
        arming.neutral_side = Some(after.signum());
    }
    if let Some(gate) = gate {
        let entered = !arming.in_gate && after >= gate;
        let left = arming.in_gate && after < gate - REARM;
        if entered || left {
            arming.in_gate = entered;
            return Some(ControlCue::Gate);
        }
    }
    if let Some(cue) = stop_cue(arming, after.abs()) {
        return Some(cue);
    }
    if arming.neutral_due {
        arming.neutral_due = false;
        return Some(ControlCue::Neutral);
    }
    None
}

/// The cue a stick gives at `share` of its full deflection.
pub fn stick_cue(arming: &mut TravelCues, share: f32) -> Option<ControlCue> {
    if !share.is_finite() {
        return None;
    }
    stop_cue(arming, share)
}

/// How far a control must leave a stop, a detent, or the gate, as a share of its travel, before
/// the cue may fire again.
const REARM: f32 = 0.1;

/// The share of the travel that counts as at the stop.
const AT_STOP: f32 = 0.995;

fn stop_cue(arming: &mut TravelCues, share: f32) -> Option<ControlCue> {
    if share < 1.0 - REARM {
        arming.stop = true;
    }
    if arming.stop && share >= AT_STOP {
        arming.stop = false;
        return Some(ControlCue::Stop);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lever_stops_once_until_it_leaves_the_stop() {
        let mut arming = TravelCues::new();
        assert_eq!(lever_cue(&mut arming, 1.0, None), Some(ControlCue::Stop));
        assert_eq!(lever_cue(&mut arming, 0.97, None), None);
        assert_eq!(lever_cue(&mut arming, 1.0, None), None);
        assert_eq!(lever_cue(&mut arming, 0.5, None), None);
        assert_eq!(lever_cue(&mut arming, -1.0, None), Some(ControlCue::Stop));
    }

    #[test]
    fn neutral_cues_on_passing_through_it_once_the_lever_has_left() {
        let mut arming = TravelCues::new();
        // A lever starting at neutral and wobbling about it does not cue.
        assert_eq!(lever_cue(&mut arming, 0.05, None), None);
        assert_eq!(lever_cue(&mut arming, -0.03, None), None);
        assert_eq!(lever_cue(&mut arming, 0.3, None), None);
        assert_eq!(
            lever_cue(&mut arming, -0.1, None),
            Some(ControlCue::Neutral)
        );
        assert_eq!(lever_cue(&mut arming, 0.05, None), None);
        // Left by the margin and brought back to neutral, it cues there.
        assert_eq!(lever_cue(&mut arming, 0.3, None), None);
        assert_eq!(lever_cue(&mut arming, 0.0, None), Some(ControlCue::Neutral));
    }

    #[test]
    fn a_neutral_passed_with_the_gate_cues_on_the_next_tick() {
        let mut arming = TravelCues::new();
        let gate = Some(0.85);
        assert_eq!(lever_cue(&mut arming, 0.9, gate), Some(ControlCue::Gate));
        assert_eq!(lever_cue(&mut arming, 0.0, gate), Some(ControlCue::Gate));
        assert_eq!(
            lever_cue(&mut arming, -0.5, gate),
            Some(ControlCue::Neutral)
        );
    }

    #[test]
    fn the_gate_cues_going_in_and_coming_out_with_hysteresis() {
        let mut arming = TravelCues::new();
        let gate = Some(0.85);
        assert_eq!(lever_cue(&mut arming, 0.8, gate), None);
        assert_eq!(lever_cue(&mut arming, 0.9, gate), Some(ControlCue::Gate));
        // Wobbling just below the gate's edge stays in it.
        assert_eq!(lever_cue(&mut arming, 0.8, gate), None);
        assert_eq!(lever_cue(&mut arming, 0.9, gate), None);
        assert_eq!(lever_cue(&mut arming, 0.6, gate), Some(ControlCue::Gate));
    }

    #[test]
    fn a_stick_stops_at_full_deflection() {
        let mut arming = TravelCues::new();
        assert_eq!(stick_cue(&mut arming, 0.5), None);
        assert_eq!(stick_cue(&mut arming, 1.0), Some(ControlCue::Stop));
        assert_eq!(stick_cue(&mut arming, 0.95), None);
        assert_eq!(stick_cue(&mut arming, 1.0), None);
        assert_eq!(stick_cue(&mut arming, 0.5), None);
        assert_eq!(stick_cue(&mut arming, 1.0), Some(ControlCue::Stop));
    }
}
