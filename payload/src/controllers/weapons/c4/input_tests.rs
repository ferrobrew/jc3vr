//! Input ownership through the actual wheel and charge state transitions.

use std::time::Duration;

use super::*;

fn state() -> State {
    State {
        stage: Stage::Idle,
        primed: None,
        thrown: [false; 2],
        status: C4Status::default(),
        grips: [None; 2],
        detonation: None,
        was_detonating: false,
        held_since: None,
        held_since_s: None,
        stick: None,
        preview: None,
        wheel: None,
    }
}

#[test]
fn wheel_chooses_available_offhand_instead_of_unavailable_dominant() {
    for dominant in Hand::BOTH {
        let mut state = state();
        state.status.planting_mode = true;
        let unavailable = Hand::BOTH.map(|hand| hand == dominant);
        let hands = WheelHands::available([true; 2], unavailable, dominant);
        let now = Instant::now();
        assert_eq!(
            state.step_wheel(hands, now),
            WheelStep::Leave(dominant.other())
        );
        state.status.planting_mode = false;
        assert_eq!(
            state.step_wheel(hands, now),
            WheelStep::Take(dominant.other())
        );
        assert_eq!(state.stage.hand(), Some(dominant.other()));
    }
}

#[test]
fn pending_wheel_owner_loss_cancels_before_planting_ends_even_after_recovery() {
    for owner in Hand::BOTH {
        let mut state = state();
        state.status.planting_mode = true;
        let hands = WheelHands::available([true; 2], [false; 2], owner);
        let now = Instant::now();
        assert_eq!(state.step_wheel(hands, now), WheelStep::Leave(owner));
        assert_eq!(state.stage, Stage::Idle);
        state.input_unavailable(Hand::BOTH.map(|hand| hand == owner));
        assert!(matches!(state.wheel, Some(WheelRequest::Cancelled)));
        assert_eq!(state.step_wheel(hands, now), WheelStep::Wait);
        state.status.planting_mode = false;
        assert_eq!(
            state.step_wheel(hands, now + Duration::from_millis(10)),
            WheelStep::Wait
        );
        assert_eq!(state.stage, Stage::Idle);
    }
}

#[test]
fn unavailable_other_hand_preserves_pending_and_held_charge_owner() {
    for owner in Hand::BOTH {
        let mut state = state();
        state.status.planting_mode = true;
        let hands = WheelHands::available([true; 2], [false; 2], owner);
        let now = Instant::now();
        assert_eq!(state.step_wheel(hands, now), WheelStep::Leave(owner));
        state.input_unavailable(Hand::BOTH.map(|hand| hand != owner));
        assert!(state.wheel.is_some());
        state.status.planting_mode = false;
        assert_eq!(state.step_wheel(hands, now), WheelStep::Take(owner));
        state.stage = Stage::Held {
            hand: owner,
            release: None,
        };
        state.input_unavailable(Hand::BOTH.map(|hand| hand != owner));
        assert_eq!(
            state.stage,
            Stage::Held {
                hand: owner,
                release: None
            }
        );
        state.input_unavailable(Hand::BOTH.map(|hand| hand == owner));
        assert_ne!(
            state.stage,
            Stage::Held {
                hand: owner,
                release: None
            }
        );
    }
}
