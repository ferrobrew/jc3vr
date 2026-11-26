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
    /// A held stick came back to its centre.
    Centre,
    /// The throttle passed one of the steps of its forward section.
    Step,
    /// The throttle came back to zero and stopped there, at the gate in front of its reverse
    /// section.
    ZeroStop,
    /// The throttle went through its zero gate, into its reverse section or back out of it.
    ReverseGate,
}

impl ControlCue {
    pub const fn label(self) -> &'static str {
        match self {
            ControlCue::Stop => "stop",
            ControlCue::Neutral => "neutral",
            ControlCue::Gate => "gate",
            ControlCue::Centre => "centre",
            ControlCue::Step => "step",
            ControlCue::ZeroStop => "zero stop",
            ControlCue::ReverseGate => "reverse gate",
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

/// The cue a held stick gives at `share` of its full deflection: a stop at full deflection, and
/// centre on coming back to it once it has left it by the margin.
pub fn stick_cue(arming: &mut TravelCues, share: f32) -> Option<ControlCue> {
    if !share.is_finite() {
        return None;
    }
    if share > REARM {
        arming.neutral_due = true;
    }
    if let Some(cue) = stop_cue(arming, share) {
        return Some(cue);
    }
    centre_cue(arming, share)
}

/// The centre cue of a control at `share` of its travel from centre: armed once the control has
/// left centre by the margin, it fires on coming back within [`AT_CENTRE`].
pub fn centre_cue(arming: &mut TravelCues, share: f32) -> Option<ControlCue> {
    if share > REARM {
        arming.neutral_due = true;
    }
    if arming.neutral_due && share <= AT_CENTRE {
        arming.neutral_due = false;
        return Some(ControlCue::Centre);
    }
    None
}

/// Which of the throttle's cues may fire.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ThrottleCues {
    /// The step of the forward section the throttle was last in, `None` until it is moved.
    step: Option<i32>,
    /// Whether the zero stop may fire: the throttle has left zero by the margin since it last
    /// fired.
    zero: bool,
    in_gate: bool,
    stop: bool,
}

impl ThrottleCues {
    /// The arming of a throttle at zero.
    pub const fn new() -> Self {
        Self {
            step: None,
            zero: false,
            in_gate: false,
            stop: true,
        }
    }
}

impl Default for ThrottleCues {
    fn default() -> Self {
        Self::new()
    }
}

/// What the throttle did this tick, for its cue.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ThrottleMove {
    /// The share of the forward section the throttle is at, `0..=1`, zero in reverse.
    pub forward: f32,
    /// The share of the reverse section, `0..=1`, zero in forward.
    pub reverse: f32,
    /// Whether the throttle went through its zero gate this tick.
    pub crossed: bool,
}

/// The cue the throttle gives on a move, with its afterburner gate at `gate` along the forward
/// section and steps every `step` of it. One cue fires a tick: the zero gate's, then the zero
/// stop's, the afterburner gate's, a stop's, and a step's.
pub fn throttle_cue(
    arming: &mut ThrottleCues,
    moved: ThrottleMove,
    gate: Option<f32>,
    step: f32,
) -> Option<ControlCue> {
    let ThrottleMove {
        forward,
        reverse,
        crossed,
    } = moved;
    if !forward.is_finite() || !reverse.is_finite() {
        return None;
    }
    let bucket = (step > 1e-3).then(|| (forward / step + 1e-4).floor() as i32);
    let stepped = bucket.is_some_and(|b| arming.step.is_some_and(|last| last != b) && b > 0);
    arming.step = bucket;
    if crossed {
        arming.zero = false;
        return Some(ControlCue::ReverseGate);
    }
    if forward > REARM * 0.5 || reverse > REARM * 0.5 {
        arming.zero = true;
    }
    if arming.zero && forward <= 0.0 && reverse <= 0.0 {
        arming.zero = false;
        return Some(ControlCue::ZeroStop);
    }
    if let Some(gate) = gate {
        let entered = !arming.in_gate && forward >= gate;
        let left = arming.in_gate && forward < gate - REARM;
        if entered || left {
            arming.in_gate = entered;
            return Some(ControlCue::Gate);
        }
    }
    let at_end = forward.max(reverse);
    if at_end < 1.0 - REARM {
        arming.stop = true;
    }
    if arming.stop && at_end >= AT_STOP {
        arming.stop = false;
        return Some(ControlCue::Stop);
    }
    stepped.then_some(ControlCue::Step)
}

/// How far a control must leave a stop, a detent, or the gate, as a share of its travel, before
/// the cue may fire again.
const REARM: f32 = 0.1;

/// The share of the travel that counts as at the stop.
const AT_STOP: f32 = 0.995;

/// The share of the travel from centre that counts as at centre.
const AT_CENTRE: f32 = 0.03;

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
    fn a_stick_cues_centre_on_coming_back_once_it_has_left() {
        let mut arming = TravelCues::new();
        assert_eq!(stick_cue(&mut arming, 0.0), None);
        assert_eq!(stick_cue(&mut arming, 0.05), None);
        assert_eq!(stick_cue(&mut arming, 0.0), None);
        assert_eq!(stick_cue(&mut arming, 0.4), None);
        assert_eq!(stick_cue(&mut arming, 0.02), Some(ControlCue::Centre));
        assert_eq!(stick_cue(&mut arming, 0.0), None);
    }

    fn forward(share: f32) -> ThrottleMove {
        ThrottleMove {
            forward: share,
            reverse: 0.0,
            crossed: false,
        }
    }

    #[test]
    fn the_throttle_ticks_each_step_and_stops_hard_at_zero() {
        let mut arming = ThrottleCues::new();
        let cue = |a: &mut ThrottleCues, m| throttle_cue(a, m, Some(0.85), 0.1);
        // Taking the throttle at zero cues nothing.
        assert_eq!(cue(&mut arming, forward(0.0)), None);
        assert_eq!(cue(&mut arming, forward(0.05)), None);
        assert_eq!(cue(&mut arming, forward(0.12)), Some(ControlCue::Step));
        assert_eq!(cue(&mut arming, forward(0.15)), None);
        assert_eq!(cue(&mut arming, forward(0.31)), Some(ControlCue::Step));
        assert_eq!(cue(&mut arming, forward(0.0)), Some(ControlCue::ZeroStop));
        // Held against the stop, it does not buzz again.
        assert_eq!(cue(&mut arming, forward(0.0)), None);
        assert_eq!(cue(&mut arming, forward(0.9)), Some(ControlCue::Gate));
        assert_eq!(cue(&mut arming, forward(1.0)), Some(ControlCue::Stop));
    }

    #[test]
    fn the_zero_gate_cues_both_ways() {
        let mut arming = ThrottleCues::new();
        let crossing = ThrottleMove {
            forward: 0.0,
            reverse: 0.2,
            crossed: true,
        };
        assert_eq!(
            throttle_cue(&mut arming, crossing, None, 0.1),
            Some(ControlCue::ReverseGate)
        );
        let back = ThrottleMove {
            forward: 0.1,
            reverse: 0.0,
            crossed: true,
        };
        assert_eq!(
            throttle_cue(&mut arming, back, None, 0.1),
            Some(ControlCue::ReverseGate)
        );
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
