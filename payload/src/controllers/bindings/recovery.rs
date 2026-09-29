//! Per-hand neutral rearming after controller input or pose loss.

use crate::{
    controllers::{
        config::{BindingsConfig, InputConfig},
        frame::{ButtonSet, ControllerFrame, Edges, HandFrame},
    },
    headpose::config::VrTurnConfig,
};

pub(super) struct NeutralPolicy {
    trigger_release: f32,
    trigger_deadzone: f32,
    squeeze_release: f32,
    stick_deadzone: f32,
}

impl NeutralPolicy {
    pub fn new(input: &InputConfig, bindings: &BindingsConfig, turn: &VrTurnConfig) -> Self {
        Self {
            trigger_release: input.trigger_press.release,
            trigger_deadzone: bindings.trigger_deadzone.clamp(0.0, 0.99),
            squeeze_release: input.squeeze_press.release,
            stick_deadzone: input
                .stick_deadzone
                .min(bindings.move_deadzone.clamp(0.0, 0.99))
                .min(turn.deadzone)
                .min(turn.snap_threshold),
        }
    }
}

pub(super) struct Recovery {
    usable: [bool; 2],
    armed: [bool; 2],
}

impl Recovery {
    pub const fn new() -> Self {
        Self {
            usable: [false; 2],
            armed: [false; 2],
        }
    }

    /// Shared hold history cannot distinguish a continuing press after it is cleared.
    pub fn disarm(&mut self) {
        self.armed = [false; 2];
    }

    pub fn suspend(&mut self) {
        self.usable = [false; 2];
        self.disarm();
    }

    /// Observe pose loss even during an action-set transition. Inferred valid poses remain usable.
    pub fn observe(&mut self, frame: &ControllerFrame) -> bool {
        let mut lost = false;
        for i in 0..2 {
            let usable = frame.focused && hand_usable(&frame.hands[i]);
            lost |= self.usable[i] && !usable;
            if !usable {
                self.armed[i] = false;
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
            if self.usable[i] && neutral(&frame.hands[i], config) {
                self.armed[i] = true;
            }
            frame.hands[i].inputs = Default::default();
            frame.hands[i].buttons = ButtonSet::EMPTY;
            edges.pressed[i] = ButtonSet::EMPTY;
            edges.released[i] = ButtonSet::EMPTY;
        }
    }
}

fn hand_usable(hand: &HandFrame) -> bool {
    hand.active && hand.grip.is_valid()
}

fn neutral(hand: &HandFrame, config: &NeutralPolicy) -> bool {
    let input = &hand.inputs;
    // Force and touch positions have no gameplay binding; their noise must not prevent recovery.
    // Both digital release and analog deadzones matter: a released trigger can still accelerate.
    !hand.buttons.iter().any(|button| !button.is_touch())
        && input.trigger.is_finite()
        && input.trigger >= 0.0
        && input.trigger < config.trigger_release
        && input.trigger <= config.trigger_deadzone
        && input.squeeze.is_finite()
        && input.squeeze >= 0.0
        && input.squeeze < config.squeeze_release
        && input.thumbstick.is_finite()
        && (input.thumbstick.length() == 0.0 || input.thumbstick.length() < config.stick_deadzone)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

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
        )
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
                config.trigger_press,
                config.squeeze_press,
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
    fn configured_neutral_allows_index_resting_grip_and_ignores_touch_position() {
        let mut input = InputConfig::new();
        input.trigger_press.release = 0.4;
        input.squeeze_press.release = 0.6;
        input.stick_deadzone = 0.25;
        let mut bindings = BindingsConfig::new();
        bindings.trigger_deadzone = 0.35;
        bindings.move_deadzone = 0.3;
        let mut turn = VrTurnConfig::new();
        turn.deadzone = 0.3;
        let config = NeutralPolicy::new(&input, &bindings, &turn);
        let mut sample = published(1, [true, false], [0.0; 2]);
        let hand = &mut sample.hands[0];
        hand.profile = InteractionProfile::ValveIndex;
        hand.inputs.trigger = 0.3;
        hand.inputs.squeeze = 0.5;
        hand.inputs.thumbstick = Vec2::new(0.2, 0.0);
        hand.inputs.trackpad = Vec2::ONE;
        hand.buttons.set(Button::TrackpadTouch, true);
        assert!(neutral(hand, &config));
        hand.inputs.squeeze = config.squeeze_release;
        assert!(!neutral(hand, &config));
        hand.inputs.squeeze = 0.5;
        hand.inputs.trigger = config.trigger_release;
        assert!(!neutral(hand, &config));
        hand.inputs.trigger = 0.3;
        hand.inputs.thumbstick = Vec2::X;
        assert!(!neutral(hand, &config));
        hand.inputs.thumbstick = Vec2::ZERO;
        hand.inputs.squeeze_force = 0.01;
        hand.inputs.trackpad_force = 0.02;
        assert!(neutral(hand, &config));
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
    fn stick_neutral_uses_the_smallest_consumer_threshold() {
        let mut input = InputConfig::new();
        let mut bindings = BindingsConfig::new();
        let mut turn = VrTurnConfig::new();
        let mut sample = published(1, [true, false], [0.0; 2]);
        for thresholds in [[0.1, 0.3, 0.3], [0.3, 0.1, 0.3], [0.3, 0.3, 0.1]] {
            input.stick_deadzone = thresholds[0];
            bindings.move_deadzone = thresholds[1];
            turn.deadzone = thresholds[2];
            let policy = NeutralPolicy::new(&input, &bindings, &turn);
            sample.hands[0].inputs.thumbstick = Vec2::new(0.2, 0.0);
            assert!(!neutral(&sample.hands[0], &policy));
            sample.hands[0].inputs.thumbstick = Vec2::new(0.05, 0.0);
            assert!(neutral(&sample.hands[0], &policy));
        }
        turn.deadzone = 0.0;
        sample.hands[0].inputs.thumbstick = Vec2::ZERO;
        assert!(neutral(
            &sample.hands[0],
            &NeutralPolicy::new(&input, &bindings, &turn)
        ));
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
