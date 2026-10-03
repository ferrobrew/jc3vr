//! Per-hand neutral rearming after controller input or pose loss.
//!
//! When a hand loses its usable pose, or injection idles, the shared hold tracker is cleared, and
//! it cannot tell a control still held from before the loss from a new press. The whole hand is
//! therefore masked until a sample arrives with its controls at rest. "At rest" is judged against
//! what the bindings would act on: a value inside every consumer's deadzone does nothing in play,
//! so it cannot be a held press, and demanding more would leave a worn, drifting control masking
//! the hand for good.
//!
//! A masked hand is visible in the tick report ([`HandRecovery`]) and, once it has waited longer
//! than a routine re-acquisition, through a rate-limited log ([`MaskLog`]).

use std::time::{Duration, Instant};

use crate::{
    controllers::{
        config::{BindingsConfig, InputConfig},
        frame::{Button, ButtonSet, ControllerFrame, Edges, Hand, HandFrame},
    },
    headpose::config::{VrTurnConfig, VrTurnMode},
};

/// One hand's recovery state, for the tick report.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum HandRecovery {
    /// The hand has no usable pose, or injection is idle.
    #[default]
    Unusable,
    /// The hand is usable and masked until its controls rest, held by `Some` control, or `None`
    /// before its first sample has been judged.
    Masked(Option<Blocker>),
    /// The hand's input passes through.
    Ready,
}

/// The control that keeps a masked hand from rearming, with its value.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Blocker {
    /// A digital button other than a capacitive touch is down.
    Button(Button),
    /// The trigger's analog value.
    Trigger(f32),
    /// The grip reading its press is derived from: the squeeze force on a force-sensing grip, the
    /// squeeze value elsewhere ([`InputConfig::grip_press`]).
    Squeeze(f32),
    /// The thumbstick's magnitude and the neutral threshold it must fall under.
    Stick { magnitude: f32, neutral: f32 },
}

/// The thresholds a hand's controls must fall under to count as at rest.
pub(super) struct NeutralPolicy {
    trigger_release: f32,
    trigger_deadzone: f32,
    /// The grip press thresholds, per profile ([`InputConfig::grip_press`]).
    input: InputConfig,
    /// Per hand, in [`Hand::index`] order.
    stick_neutral: [f32; 2],
}

impl NeutralPolicy {
    pub fn new(
        input: &InputConfig,
        bindings: &BindingsConfig,
        turn: &VrTurnConfig,
        dominant: Hand,
    ) -> Self {
        let move_deadzone = finite_or_zero(bindings.move_deadzone).clamp(0.0, 0.99);
        // The dominant stick also turns the body: continuously past the smooth deadzone, or by a
        // snap that rearms once the stick relaxes under half its threshold.
        let turn_threshold = match turn.mode {
            VrTurnMode::Smooth => turn.deadzone,
            VrTurnMode::Snap => turn.snap_threshold * 0.5,
        };
        let dominant_stick = move_deadzone.min(finite_or_zero(turn_threshold).max(0.0));
        let mut stick_neutral = [0.0; 2];
        stick_neutral[dominant.index()] = dominant_stick.max(STICK_NEUTRAL_FLOOR);
        stick_neutral[dominant.other().index()] = move_deadzone.max(STICK_NEUTRAL_FLOOR);
        Self {
            trigger_release: input.trigger_press.release,
            trigger_deadzone: bindings.trigger_deadzone.clamp(0.0, 0.99),
            input: *input,
            stick_neutral,
        }
    }
}

pub(super) struct Recovery {
    usable: [bool; 2],
    armed: [bool; 2],
    blockers: [Option<Blocker>; 2],
}

impl Recovery {
    pub const fn new() -> Self {
        Self {
            usable: [false; 2],
            armed: [false; 2],
            blockers: [None; 2],
        }
    }

    /// Shared hold history cannot distinguish a continuing press after it is cleared.
    pub fn disarm(&mut self) {
        self.armed = [false; 2];
        self.blockers = [None; 2];
    }

    pub fn suspend(&mut self) {
        self.usable = [false; 2];
        self.disarm();
    }

    /// Whether this hand has produced a neutral sample since it became usable again. Consumers that
    /// own a separate click path must use this gate as well as the filtered frame, because filtering
    /// a held trigger makes the copied frame look released.
    pub fn ready(&self, hand: Hand) -> bool {
        self.usable[hand.index()] && self.armed[hand.index()]
    }

    /// Each hand's state, in [`Hand::index`] order.
    pub fn status(&self) -> [HandRecovery; 2] {
        std::array::from_fn(|i| match (self.usable[i], self.armed[i]) {
            (false, _) => HandRecovery::Unusable,
            (true, true) => HandRecovery::Ready,
            (true, false) => HandRecovery::Masked(self.blockers[i]),
        })
    }

    /// Observe pose loss even during an action-set transition. Inferred valid poses remain usable.
    pub fn observe(&mut self, frame: &ControllerFrame) -> bool {
        let mut lost = false;
        for i in 0..2 {
            let usable = frame.focused && hand_usable(&frame.hands[i]);
            lost |= self.usable[i] && !usable;
            if !usable {
                self.armed[i] = false;
                self.blockers[i] = None;
            }
            self.usable[i] = usable;
        }
        lost
    }

    /// Mask the entire affected hand until neutral, including the neutral sample's queued edges.
    /// Only call for a fresh frame from the action set currently being consumed.
    pub fn filter(
        &mut self,
        frame: &mut ControllerFrame,
        edges: &mut Edges,
        config: &NeutralPolicy,
    ) {
        for i in 0..2 {
            if self.usable[i] && self.armed[i] {
                continue;
            }
            if self.usable[i] {
                self.blockers[i] = blocker(&frame.hands[i], config, i);
                self.armed[i] = self.blockers[i].is_none();
            }
            frame.hands[i].inputs = Default::default();
            frame.hands[i].buttons = ButtonSet::EMPTY;
            edges.pressed[i] = ButtonSet::EMPTY;
            edges.released[i] = ButtonSet::EMPTY;
        }
    }
}

/// A hand's masked wait, to log.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) enum MaskEvent {
    /// The hand has been masked for `waited`, held by the blocker.
    StillMasked {
        waited: Duration,
        blocker: Option<Blocker>,
    },
    /// A hand whose wait was logged has rearmed after `waited`.
    Rearmed { waited: Duration },
}

/// Rate-limits the masked-hand log: a hand re-acquired with its controls at rest rearms within a
/// tick or two and is never logged; one that keeps waiting is logged after [`MASK_LOG_DELAY`] and
/// then every [`MASK_LOG_INTERVAL`], and its rearm is logged only if its wait was.
pub(super) struct MaskLog {
    hands: [MaskWait; 2],
}

impl MaskLog {
    pub const fn new() -> Self {
        Self {
            hands: [MaskWait::IDLE; 2],
        }
    }

    pub fn step(&mut self, status: [HandRecovery; 2], now: Instant) -> [Option<MaskEvent>; 2] {
        std::array::from_fn(|i| self.hands[i].step(status[i], now))
    }
}

/// A resting stick magnitude that always counts as neutral, even when a consumer's deadzone is
/// smaller. Worn sticks rest this far off centre, and a stick this close to centre can only feed a
/// continuous consumer (a slow walk or turn), never an edge.
const STICK_NEUTRAL_FLOOR: f32 = 0.1;

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn hand_usable(hand: &HandFrame) -> bool {
    hand.active && hand.grip.is_valid()
}

/// The first control of `hand` that is not at rest, or `None` when the hand is neutral.
fn blocker(hand: &HandFrame, config: &NeutralPolicy, index: usize) -> Option<Blocker> {
    let input = &hand.inputs;
    // Touch positions and forces below their press thresholds act on nothing; their noise must not
    // prevent recovery. A trackpad pressed past its threshold is a derived button, caught here.
    if let Some(button) = hand.buttons.iter().find(|button| !button.is_touch()) {
        return Some(Blocker::Button(button));
    }
    // Both digital release and analog deadzones matter: a released trigger can still accelerate.
    if !(input.trigger.is_finite()
        && input.trigger >= 0.0
        && input.trigger < config.trigger_release
        && input.trigger <= config.trigger_deadzone)
    {
        return Some(Blocker::Trigger(input.trigger));
    }
    let (grip, grip_thresholds) = config
        .input
        .grip_press(hand.profile, Hand::BOTH[index])
        .reading(input);
    if !(grip.is_finite() && grip >= 0.0 && grip < grip_thresholds.release) {
        return Some(Blocker::Squeeze(grip));
    }
    let magnitude = input.thumbstick.length();
    let neutral = config.stick_neutral[index];
    if !(magnitude.is_finite() && magnitude < neutral) {
        return Some(Blocker::Stick { magnitude, neutral });
    }
    None
}

/// How long a usable hand waits for neutral before the wait is logged.
const MASK_LOG_DELAY: Duration = Duration::from_secs(1);
/// How often a hand that stays masked is logged again.
const MASK_LOG_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Copy, Clone)]
struct MaskWait {
    since: Option<Instant>,
    /// When the wait is next due to be logged, relative to `since`.
    next_log: Duration,
}

impl MaskWait {
    const IDLE: Self = Self {
        since: None,
        next_log: MASK_LOG_DELAY,
    };

    fn step(&mut self, status: HandRecovery, now: Instant) -> Option<MaskEvent> {
        match status {
            HandRecovery::Masked(blocker) => {
                let since = *self.since.get_or_insert(now);
                let waited = now.saturating_duration_since(since);
                if waited < self.next_log {
                    return None;
                }
                self.next_log = waited + MASK_LOG_INTERVAL;
                Some(MaskEvent::StillMasked { waited, blocker })
            }
            HandRecovery::Ready => {
                let logged = self.next_log > MASK_LOG_DELAY;
                let waited = self.since.map(|since| now.saturating_duration_since(since));
                *self = Self::IDLE;
                logged
                    .then_some(waited)
                    .flatten()
                    .map(|waited| MaskEvent::Rearmed { waited })
            }
            HandRecovery::Unusable => {
                *self = Self::IDLE;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec2;
    use jc3gi::input::input_action_map::Action;

    use super::*;
    use crate::controllers::{
        bindings::{
            State, TickReport,
            controls::Controls,
            hold::{Desired, HoldTracker, Phase, Target, Want},
            tables::Bindings,
            yank::YankDetector,
        },
        config::BindingsConfig,
        frame::{ActionSetKind, Button, Hand, InteractionProfile, derive_buttons},
    };

    fn policy() -> NeutralPolicy {
        NeutralPolicy::new(
            &InputConfig::new(),
            &BindingsConfig::new(),
            &VrTurnConfig::new(),
            Hand::Right,
        )
    }

    fn neutral(hand: &HandFrame, config: &NeutralPolicy, which: Hand) -> bool {
        blocker(hand, config, which.index()).is_none()
    }

    fn published(sequence: u64, usable: [bool; 2], triggers: [f32; 2]) -> ControllerFrame {
        let config = InputConfig::new();
        let hands = std::array::from_fn(|i| {
            let mut hand = HandFrame {
                active: usable[i],
                ..HandFrame::default()
            };
            hand.grip.position_valid = usable[i];
            hand.grip.orientation_valid = usable[i];
            hand.inputs.trigger = triggers[i];
            hand.buttons = derive_buttons(
                &hand.inputs,
                ButtonSet::EMPTY,
                &config.press_rules(hand.profile, Hand::BOTH[i]),
            );
            hand
        });
        ControllerFrame {
            sequence,
            display_time_ns: sequence as i64,
            published_at: Instant::now(),
            focused: true,
            action_set: ActionSetKind::OnFoot,
            hands,
        }
    }

    fn consume(
        recovery: &mut Recovery,
        published: &ControllerFrame,
    ) -> (bool, ControllerFrame, Edges) {
        let lost = recovery.observe(published);
        if lost {
            recovery.disarm();
        }
        let mut frame = published.clone();
        let mut edges = Edges {
            pressed: published.hands.each_ref().map(|h| h.buttons),
            ..Edges::default()
        };
        recovery.filter(&mut frame, &mut edges, &policy());
        (lost, frame, edges)
    }

    #[test]
    fn other_hand_loss_cannot_turn_a_held_trigger_into_a_new_click() {
        let mut state = State {
            recovery: Recovery::new(),
            mask_log: MaskLog::new(),
            tracker: HoldTracker::new(),
            bindings: Bindings::new(),
            yank: YankDetector::new(),
            report: TickReport::default(),
        };
        let samples = [
            published(1, [true, true], [0.0, 0.0]),
            published(2, [true, true], [0.0, 1.0]),
            published(3, [false, true], [0.0, 1.0]),
            published(4, [false, true], [0.0, 1.0]),
            published(5, [false, true], [0.0, 0.0]),
            published(6, [false, true], [0.0, 1.0]),
        ];
        for (i, sample) in samples.iter().enumerate() {
            if state.recovery.observe(sample) {
                state.reset();
            }
            let mut frame = sample.clone();
            let mut edges = Edges::default();
            state.recovery.filter(&mut frame, &mut edges, &policy());
            let controls = Controls::from_frame(
                &frame,
                &edges,
                &BindingsConfig::new(),
                Hand::Right,
                0.0,
                [None; 2],
            );
            let desired = if controls.dom().trigger_press.held() {
                vec![Desired {
                    target: Target::player(Action::FIRE_RIGHT),
                    want: Want::Hold(1.0),
                }]
            } else {
                Vec::new()
            };
            let writes = state.tracker.resolve(&desired, 0.01);
            if matches!(i, 1 | 5) {
                assert_eq!(writes.len(), 1);
                assert_eq!(writes[0].phase, Phase::Clicked);
            } else {
                assert!(
                    writes.is_empty(),
                    "sample {} must not fire or synthesize release",
                    i + 1
                );
            }
        }
        assert_eq!(samples[2].hands[1].inputs.trigger, 1.0);
        assert_eq!(state.recovery.armed, [false, true]);
    }

    #[test]
    fn recovering_hand_needs_a_real_neutral_sample_not_a_masked_copy() {
        let mut recovery = Recovery::new();
        consume(&mut recovery, &published(1, [true, false], [0.0; 2]));
        assert!(consume(&mut recovery, &published(2, [false; 2], [0.0; 2])).0);
        for sequence in 3..6 {
            let sample = published(sequence, [true, false], [1.0, 0.0]);
            let (_, filtered, edges) = consume(&mut recovery, &sample);
            assert_eq!(filtered.hands[0].inputs.trigger, 0.0);
            assert_eq!(edges, Edges::default());
            assert_eq!(recovery.armed, [false; 2]);
            assert_eq!(sample.hands[0].inputs.trigger, 1.0);
        }
        consume(&mut recovery, &published(6, [true, false], [0.0; 2]));
        let (_, filtered, _) = consume(&mut recovery, &published(7, [true, false], [1.0, 0.0]));
        assert_eq!(filtered.hands[0].inputs.trigger, 1.0);
        assert_eq!(recovery.armed, [true, false]);
    }

    #[test]
    fn configured_neutral_allows_a_resting_grip_and_ignores_touch_position() {
        let mut input = InputConfig::new();
        input.trigger_press.release = 0.4;
        input.squeeze_press.release = 0.6;
        input.stick_deadzone = 0.25;
        let mut bindings = BindingsConfig::new();
        bindings.trigger_deadzone = 0.35;
        bindings.move_deadzone = 0.3;
        let mut turn = VrTurnConfig::new();
        turn.deadzone = 0.3;
        let config = NeutralPolicy::new(&input, &bindings, &turn, Hand::Right);
        let mut sample = published(1, [true, false], [0.0; 2]);
        let hand = &mut sample.hands[0];
        hand.profile = InteractionProfile::OculusTouch;
        hand.inputs.trigger = 0.3;
        hand.inputs.squeeze = 0.5;
        hand.inputs.thumbstick = Vec2::new(0.2, 0.0);
        hand.inputs.trackpad = Vec2::ONE;
        hand.buttons.set(Button::TrackpadTouch, true);
        assert!(neutral(hand, &config, Hand::Left));
        hand.inputs.squeeze = input.squeeze_press.release;
        assert!(!neutral(hand, &config, Hand::Left));
        hand.inputs.squeeze = 0.5;
        hand.inputs.trigger = input.trigger_press.release;
        assert!(!neutral(hand, &config, Hand::Left));
        hand.inputs.trigger = 0.3;
        hand.inputs.thumbstick = Vec2::X;
        assert!(!neutral(hand, &config, Hand::Left));
        hand.inputs.thumbstick = Vec2::ZERO;
        hand.inputs.squeeze_force = 0.01;
        hand.inputs.trackpad_force = 0.02;
        assert!(neutral(hand, &config, Hand::Left));
    }

    #[test]
    fn a_force_grip_rests_on_its_force_not_its_value() {
        let input = InputConfig::new();
        let config = policy();
        let mut hand = published(1, [true, false], [0.0; 2]).hands[0].clone();
        hand.profile = InteractionProfile::ValveIndex;
        // The hand closed round the capacitive grip reads a full squeeze value, and is at rest.
        hand.inputs.squeeze = 1.0;
        hand.inputs.squeeze_force = input.squeeze_force_press.release * 0.5;
        assert!(neutral(&hand, &config, Hand::Left));
        hand.inputs.squeeze_force = input.squeeze_force_press.release;
        assert_eq!(
            blocker(&hand, &config, Hand::Left.index()),
            Some(Blocker::Squeeze(input.squeeze_force_press.release))
        );
        // The same reading on a value grip is a held squeeze.
        hand.profile = InteractionProfile::OculusTouch;
        hand.inputs.squeeze_force = 0.0;
        assert!(!neutral(&hand, &config, Hand::Left));
    }

    #[test]
    fn sub_release_trigger_above_analog_deadzone_does_not_rearm() {
        let mut recovery = Recovery::new();
        for sequence in 1..4 {
            let sample = published(sequence, [true, false], [0.2, 0.0]);
            assert!(!sample.hands[0].buttons.contains(Button::TriggerPress));
            let (_, frame, _) = consume(&mut recovery, &sample);
            assert_eq!(frame.hands[0].inputs.trigger, 0.0);
            assert_eq!(recovery.armed, [false; 2]);
        }
        consume(&mut recovery, &published(4, [true, false], [0.0; 2]));
        assert_eq!(recovery.armed, [true, false]);
    }

    #[test]
    fn unused_force_noise_does_not_prevent_rearming() {
        let mut recovery = Recovery::new();
        let mut sample = published(1, [true, false], [0.0; 2]);
        sample.hands[0].inputs.squeeze = 0.4;
        sample.hands[0].inputs.squeeze_force = 0.01;
        sample.hands[0].inputs.trackpad_force = 0.02;
        consume(&mut recovery, &sample);
        assert_eq!(recovery.armed, [true, false]);
    }

    #[test]
    fn each_stick_rests_under_its_own_consumers_deadzone() {
        let input = InputConfig::new();
        let mut bindings = BindingsConfig::new();
        bindings.move_deadzone = 0.2;
        let mut turn = VrTurnConfig::new();
        turn.mode = VrTurnMode::Smooth;
        turn.deadzone = 0.15;
        let config = NeutralPolicy::new(&input, &bindings, &turn, Hand::Right);
        let mut hand = published(1, [true, true], [0.0; 2]).hands[0].clone();
        // The off hand only walks, so drift under the move deadzone is at rest.
        hand.inputs.thumbstick = Vec2::new(0.18, 0.0);
        assert!(neutral(&hand, &config, Hand::Left));
        assert!(!neutral(&hand, &config, Hand::Right));
        hand.inputs.thumbstick = Vec2::new(0.12, 0.0);
        assert!(neutral(&hand, &config, Hand::Right));
        hand.inputs.thumbstick = Vec2::new(0.25, 0.0);
        assert_eq!(
            blocker(&hand, &config, Hand::Left.index()),
            Some(Blocker::Stick {
                magnitude: 0.25,
                neutral: 0.2
            })
        );
        // A left-handed layout swaps the roles.
        let config = NeutralPolicy::new(&input, &bindings, &turn, Hand::Left);
        hand.inputs.thumbstick = Vec2::new(0.18, 0.0);
        assert!(!neutral(&hand, &config, Hand::Left));
        assert!(neutral(&hand, &config, Hand::Right));
        // Snap turning acts only past its threshold and rearms under half of it.
        turn.mode = VrTurnMode::Snap;
        turn.snap_threshold = 0.3;
        let config = NeutralPolicy::new(&input, &bindings, &turn, Hand::Right);
        hand.inputs.thumbstick = Vec2::new(0.14, 0.0);
        assert!(neutral(&hand, &config, Hand::Right));
        hand.inputs.thumbstick = Vec2::new(0.16, 0.0);
        assert!(!neutral(&hand, &config, Hand::Right));
    }

    #[test]
    fn slight_drift_rests_even_under_tiny_deadzones() {
        let input = InputConfig::new();
        let mut bindings = BindingsConfig::new();
        bindings.move_deadzone = 0.0;
        let mut turn = VrTurnConfig::new();
        turn.mode = VrTurnMode::Smooth;
        turn.deadzone = 0.0;
        let config = NeutralPolicy::new(&input, &bindings, &turn, Hand::Right);
        let mut hand = published(1, [true, true], [0.0; 2]).hands[0].clone();
        for which in Hand::BOTH {
            hand.inputs.thumbstick = Vec2::new(0.0, STICK_NEUTRAL_FLOOR * 0.9);
            assert!(neutral(&hand, &config, which));
            hand.inputs.thumbstick = Vec2::new(0.0, STICK_NEUTRAL_FLOOR);
            assert!(!neutral(&hand, &config, which));
            hand.inputs.thumbstick = Vec2::new(f32::NAN, 0.0);
            assert!(!neutral(&hand, &config, which));
        }
    }

    #[test]
    fn a_drifting_off_stick_rearms_with_default_settings() {
        let mut recovery = Recovery::new();
        let mut sample = published(1, [true, true], [0.0; 2]);
        sample.hands[0].inputs.thumbstick = Vec2::new(0.1, 0.05);
        consume(&mut recovery, &sample);
        assert_eq!(recovery.status(), [HandRecovery::Ready; 2]);
    }

    #[test]
    fn status_reports_the_control_holding_a_masked_hand() {
        let mut recovery = Recovery::new();
        assert_eq!(recovery.status(), [HandRecovery::Unusable; 2]);
        recovery.observe(&published(1, [true, true], [0.0; 2]));
        assert_eq!(recovery.status(), [HandRecovery::Masked(None); 2]);
        consume(&mut recovery, &published(2, [true, true], [0.0, 0.2]));
        assert_eq!(
            recovery.status(),
            [
                HandRecovery::Ready,
                HandRecovery::Masked(Some(Blocker::Trigger(0.2)))
            ]
        );
        consume(&mut recovery, &published(3, [true, true], [0.0, 1.0]));
        assert_eq!(
            recovery.status()[1],
            HandRecovery::Masked(Some(Blocker::Button(Button::TriggerPress)))
        );
        consume(&mut recovery, &published(4, [true, false], [0.0; 2]));
        assert_eq!(recovery.status()[1], HandRecovery::Unusable);
    }

    #[test]
    fn the_mask_log_skips_routine_waits_and_rate_limits_long_ones() {
        let masked = HandRecovery::Masked(Some(Blocker::Trigger(0.5)));
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut log = MaskLog::new();
        assert_eq!(log.step([masked, HandRecovery::Ready], at(0)), [None; 2]);
        assert_eq!(log.step([HandRecovery::Ready; 2], at(200)), [None; 2]);

        assert_eq!(log.step([masked; 2], at(1_000)), [None; 2]);
        assert_eq!(log.step([masked; 2], at(1_900)), [None; 2]);
        let [first, _] = log.step([masked, HandRecovery::Unusable], at(2_000));
        assert_eq!(
            first,
            Some(MaskEvent::StillMasked {
                waited: Duration::from_secs(1),
                blocker: Some(Blocker::Trigger(0.5)),
            })
        );
        for ms in (2_100..12_000).step_by(100) {
            assert_eq!(log.step([masked, HandRecovery::Unusable], at(ms))[0], None);
        }
        assert!(matches!(
            log.step([masked, HandRecovery::Unusable], at(12_000))[0],
            Some(MaskEvent::StillMasked { .. })
        ));
        assert_eq!(
            log.step([HandRecovery::Ready; 2], at(12_500))[0],
            Some(MaskEvent::Rearmed {
                waited: Duration::from_millis(11_500)
            })
        );
        assert_eq!(log.step([HandRecovery::Ready; 2], at(13_000)), [None; 2]);
    }

    #[test]
    fn unfocused_inactive_and_mismatched_observation_cannot_rearm() {
        let mut recovery = Recovery::new();
        let mut sample = published(1, [true, false], [0.0; 2]);
        sample.focused = false;
        consume(&mut recovery, &sample);
        assert_eq!(recovery.armed, [false; 2]);
        consume(&mut recovery, &published(2, [false; 2], [0.0; 2]));
        assert_eq!(recovery.armed, [false; 2]);
        recovery.observe(&published(3, [true, false], [0.0; 2]));
        assert_eq!(recovery.armed, [false; 2]);
        consume(&mut recovery, &published(4, [true, false], [1.0, 0.0]));
        assert_eq!(recovery.armed, [false; 2]);
        recovery.suspend();
        consume(&mut recovery, &published(5, [false, true], [0.0; 2]));
        assert_eq!(recovery.armed, [false, true]);
    }
}
