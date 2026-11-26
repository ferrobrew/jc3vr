use super::*;

#[test]
fn a_hand_vector_mirrors_across_x_for_the_other_hand() {
    let v = Vec3::new(0.1, 0.2, 0.3);
    assert_eq!(Hand::Left.mirror_from_left(v), v);
    assert_eq!(Hand::Right.mirror_from_left(v), Vec3::new(-0.1, 0.2, 0.3));
    assert_eq!(Hand::Right.mirror_from_right(v), v);
    assert_eq!(Hand::Left.mirror_from_right(v), Vec3::new(-0.1, 0.2, 0.3));
}

#[test]
fn the_dominant_hand_is_tried_first() {
    for dominant in Hand::BOTH {
        assert_eq!(Hand::first_free(dominant, |_| true), Some(dominant));
        assert_eq!(
            Hand::first_free(dominant, |h| h != dominant),
            Some(dominant.other())
        );
        assert_eq!(Hand::first_free(dominant, |_| false), None);
    }
}

const THRESHOLDS: PressThresholds = PressThresholds {
    press: 0.7,
    release: 0.5,
};

fn set(buttons: &[Button]) -> ButtonSet {
    let mut s = ButtonSet::EMPTY;
    for &b in buttons {
        s.set(b, true);
    }
    s
}

#[test]
fn joint_tracked_count_requires_both_tracking_components() {
    for flags in 0..16 {
        let joint = HandJoint {
            position_valid: flags & 1 != 0,
            orientation_valid: flags & 2 != 0,
            position_tracked: flags & 4 != 0,
            orientation_tracked: flags & 8 != 0,
            ..HandJoint::default()
        };
        assert_eq!(joint.is_tracked(), flags & 12 == 12);
    }
}

#[test]
fn publication_identity_advances_even_when_display_time_does_not() {
    let now = Instant::now();
    let mut slot = Slot {
        frame: None,
        sequence: 40,
    };
    let frame = ControllerFrame {
        sequence: 0,
        display_time_ns: 90,
        published_at: now,
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands: Default::default(),
    };
    let first = slot.publish(frame.clone(), now).clone();
    let second = slot.publish(frame, now + Duration::from_nanos(1));
    assert_eq!(first.sequence, 41);
    assert_eq!(second.sequence, 42);
    assert_eq!(first.display_time_ns, second.display_time_ns);
    assert_eq!(second.published_at, now + Duration::from_nanos(1));
}

#[test]
fn press_thresholds_have_hysteresis() {
    assert!(!THRESHOLDS.apply(false, 0.6));
    assert!(THRESHOLDS.apply(false, 0.7));
    assert!(THRESHOLDS.apply(true, 0.6));
    assert!(THRESHOLDS.apply(true, 0.5));
    assert!(!THRESHOLDS.apply(true, 0.49));
}

const FORCE: PressThresholds = PressThresholds {
    press: 0.3,
    release: 0.1,
};

fn rules(grip: GripPress) -> PressRules {
    PressRules {
        trigger: THRESHOLDS,
        grip,
        trackpad: FORCE,
        trackpad_half_margin: MARGIN,
    }
}

const MARGIN: f32 = 0.2;

#[test]
fn derived_presses_follow_the_analog_values() {
    let value = rules(GripPress::Value(THRESHOLDS));
    let mut inputs = HandInputs {
        trigger: 0.8,
        squeeze: 0.2,
        a_click: true,
        ..Default::default()
    };
    let first = derive_buttons(&inputs, ButtonSet::EMPTY, &value);
    assert_eq!(first, set(&[Button::TriggerPress, Button::AClick]));

    inputs.trigger = 0.6;
    let held = derive_buttons(&inputs, first, &value);
    assert!(held.contains(Button::TriggerPress));
    let fresh = derive_buttons(&inputs, ButtonSet::EMPTY, &value);
    assert!(!fresh.contains(Button::TriggerPress));
}

#[test]
fn a_force_grip_presses_on_force_alone_with_hysteresis() {
    let force = rules(GripPress::Force(FORCE));
    // A hand resting round a capacitive grip reads a full squeeze value and no force.
    let mut inputs = HandInputs {
        squeeze: 1.0,
        squeeze_force: 0.05,
        ..Default::default()
    };
    let resting = derive_buttons(&inputs, ButtonSet::EMPTY, &force);
    assert!(!resting.contains(Button::SqueezePress));

    inputs.squeeze_force = 0.29;
    assert!(!derive_buttons(&inputs, resting, &force).contains(Button::SqueezePress));
    inputs.squeeze_force = 0.3;
    let pressed = derive_buttons(&inputs, resting, &force);
    assert!(pressed.contains(Button::SqueezePress));

    // Easing off to between the thresholds holds the press; below the release lets go, even
    // with the hand still closed round the grip.
    inputs.squeeze_force = 0.15;
    let held = derive_buttons(&inputs, pressed, &force);
    assert!(held.contains(Button::SqueezePress));
    inputs.squeeze_force = 0.09;
    assert!(!derive_buttons(&inputs, held, &force).contains(Button::SqueezePress));
    inputs.squeeze_force = f32::NAN;
    assert!(!derive_buttons(&inputs, held, &force).contains(Button::SqueezePress));
}

#[test]
fn a_value_grip_ignores_the_force() {
    let value = rules(GripPress::Value(THRESHOLDS));
    let inputs = HandInputs {
        squeeze: 0.2,
        squeeze_force: 1.0,
        ..Default::default()
    };
    assert!(!derive_buttons(&inputs, ButtonSet::EMPTY, &value).contains(Button::SqueezePress));
    let inputs = HandInputs {
        squeeze: 0.8,
        ..Default::default()
    };
    assert!(derive_buttons(&inputs, ButtonSet::EMPTY, &value).contains(Button::SqueezePress));
}

fn pad(x: f32, force: f32) -> HandInputs {
    HandInputs {
        trackpad: Vec2::new(x, 0.0),
        trackpad_force: force,
        trackpad_touch: true,
        ..Default::default()
    }
}

#[test]
fn a_trackpad_press_picks_the_half_under_the_thumb() {
    assert_eq!(
        trackpad_press(None, &pad(-0.6, 0.5), FORCE, MARGIN),
        Some(TrackpadHalf::Left)
    );
    assert_eq!(
        trackpad_press(None, &pad(0.6, 0.5), FORCE, MARGIN),
        Some(TrackpadHalf::Right)
    );
    // A touch without force, or a press on the centre line, is no press.
    assert_eq!(trackpad_press(None, &pad(0.6, 0.2), FORCE, MARGIN), None);
    assert_eq!(trackpad_press(None, &pad(0.1, 1.0), FORCE, MARGIN), None);
    assert_eq!(trackpad_press(None, &pad(-0.1, 1.0), FORCE, MARGIN), None);
    assert_eq!(
        trackpad_press(None, &pad(0.6, f32::NAN), FORCE, MARGIN),
        None
    );
}

#[test]
fn a_trackpad_press_keeps_its_half_until_released() {
    let left = Some(TrackpadHalf::Left);
    // The thumb slides across under pressure: the press stays on its half.
    assert_eq!(trackpad_press(left, &pad(0.8, 0.2), FORCE, MARGIN), left);
    assert_eq!(trackpad_press(left, &pad(0.8, 0.1), FORCE, MARGIN), left);
    assert_eq!(trackpad_press(left, &pad(0.8, 0.09), FORCE, MARGIN), None);
}

#[test]
fn trackpad_presses_derive_their_buttons() {
    let rules = rules(GripPress::Value(THRESHOLDS));
    let right = derive_buttons(&pad(0.7, 0.5), ButtonSet::EMPTY, &rules);
    assert!(right.contains(Button::TrackpadPressRight));
    assert!(!right.contains(Button::TrackpadPressLeft));
    assert!(right.contains(Button::TrackpadTouch));
    let slid = derive_buttons(&pad(-0.7, 0.2), right, &rules);
    assert!(slid.contains(Button::TrackpadPressRight));
    assert!(!slid.contains(Button::TrackpadPressLeft));
    let released = derive_buttons(&pad(-0.7, 0.0), slid, &rules);
    assert!(!released.contains(Button::TrackpadPressRight));
    assert!(!Button::TrackpadPressLeft.is_touch());
}

#[test]
fn edges_latch_a_tap_between_takes() {
    let mut edges = Edges::default();
    let down = [ButtonSet::EMPTY, set(&[Button::AClick])];
    let up = [ButtonSet::EMPTY; 2];
    edges.accumulate(up, down);
    edges.accumulate(down, up);
    assert!(edges.pressed(Hand::Right, Button::AClick));
    assert!(edges.released(Hand::Right, Button::AClick));
    assert!(!edges.pressed(Hand::Left, Button::AClick));
}

#[test]
fn edges_ignore_held_buttons() {
    let mut edges = Edges::default();
    let held = [set(&[Button::BClick]), ButtonSet::EMPTY];
    edges.accumulate(held, held);
    assert_eq!(edges, Edges::default());
}

#[test]
fn button_set_iterates_in_bit_order() {
    let s = set(&[Button::SystemClick, Button::TriggerClick]);
    let buttons: Vec<_> = s.iter().collect();
    assert_eq!(buttons, vec![Button::TriggerClick, Button::SystemClick]);
    assert_eq!(
        s.difference(set(&[Button::SystemClick])),
        set(&[Button::TriggerClick])
    );
}

fn empty_slot() -> Slot {
    Slot {
        frame: None,
        sequence: 0,
    }
}

fn frame_with(right: ButtonSet) -> ControllerFrame {
    let mut hands: [HandFrame; 2] = Default::default();
    hands[Hand::Right.index()].active = true;
    hands[Hand::Right.index()].buttons = right;
    ControllerFrame {
        sequence: 0,
        display_time_ns: 0,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands,
    }
}

#[test]
fn a_frame_goes_stale_past_the_age_bound() {
    let mut slot = empty_slot();
    let t0 = Instant::now();
    slot.publish(frame_with(ButtonSet::EMPTY), t0);
    assert!(slot.fresh(t0).is_some());
    assert!(slot.fresh(t0 + MAX_FRAME_AGE).is_some());
    assert!(
        slot.fresh(t0 + MAX_FRAME_AGE + Duration::from_millis(1))
            .is_none()
    );
    slot.publish(frame_with(ButtonSet::EMPTY), t0 + Duration::from_secs(5));
    assert!(slot.fresh(t0 + Duration::from_secs(5)).is_some());
}

#[test]
fn a_clear_hides_the_frame() {
    let mut slot = empty_slot();
    let t0 = Instant::now();
    slot.publish(frame_with(ButtonSet::EMPTY), t0);
    slot.clear();
    assert!(slot.fresh(t0).is_none());
}

#[test]
fn hands_index_and_mirror() {
    assert_eq!(Hand::Left.index(), 0);
    assert_eq!(Hand::Right.index(), 1);
    assert_eq!(Hand::Left.other(), Hand::Right);
}
