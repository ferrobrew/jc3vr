//! The air brake's latch: its entry, what holds it through a flare, and what releases it.

use super::*;

/// Samples are 50 ms apart, so the brake's dwell is met on the fourth sample of a brake pose.
const ENTRY_SAMPLES: usize = 4;

fn palms(roll_deg: f32) -> FlightPose {
    FlightPose {
        palm_roll_deg: [roll_deg; 2],
        ..FLIGHT
    }
}

/// A suit entered with the arms drawn in, so the brake may engage.
fn ready_suit() -> History {
    let mut h = History::new(SUIT);
    h.hold(FOLDED, 2);
    h
}

fn braking(h: &mut History) {
    let entry = h.hold(BRAKE, ENTRY_SAMPLES);
    assert!(entry[..ENTRY_SAMPLES - 1].iter().all(|c| !c.brake));
    assert!(entry[ENTRY_SAMPLES - 1].brake);
    assert_eq!(h.state.phase(), GesturePhase::Braking);
}

#[test]
fn the_brake_engages_without_a_flight_pose_first() {
    let mut h = ready_suit();
    assert!(!h.state.armed);
    braking(&mut h);
    assert!(!h.state.armed);
}

#[test]
fn a_held_brake_survives_a_flare_to_palm_up_and_through_the_roll_wrap() {
    let mut h = ready_suit();
    braking(&mut h);
    for roll in [120.0, 150.1, 170.0, 179.9, -179.9, -170.0, -120.0, -45.0] {
        let command = h.observe(Some(palms(roll)));
        assert!(command.brake && command.transition.is_none(), "{roll}");
    }
}

#[test]
fn a_held_brake_ignores_one_flat_palm_and_releases_once_both_are_flat() {
    let mut h = ready_suit();
    braking(&mut h);
    let one_flat = FlightPose {
        palm_roll_deg: [0.0, 90.0],
        ..FLIGHT
    };
    assert!(h.observe(Some(one_flat)).brake);
    assert!(h.observe(Some(palms(config().flight_roll_deg + 1.0))).brake);
    assert!(!h.observe(Some(palms(-config().flight_roll_deg))).brake);
    assert_eq!(h.state.phase(), GesturePhase::AwaitingFlight);
}

#[test]
fn either_arm_drawn_in_past_the_hysteresis_releases_the_brake() {
    for hand in Hand::BOTH {
        let mut h = ready_suit();
        braking(&mut h);
        let mut inside = BRAKE;
        inside.lateral[hand.index()] = config().flight_lateral_span - config().pose_hysteresis_span;
        assert!(h.observe(Some(inside)).brake, "{hand:?}");
        inside.lateral[hand.index()] -= 0.001;
        assert!(!h.observe(Some(inside)).brake, "{hand:?}");
    }
}

#[test]
fn the_brake_is_entered_only_within_its_roll_band() {
    for roll in [
        config().brake_roll_deg - 0.01,
        config().brake_roll_max_deg + 0.01,
        175.0,
        -90.0,
    ] {
        let mut h = ready_suit();
        assert!(h.hold(palms(roll), 20).iter().all(|c| !c.brake), "{roll}");
    }
    let narrow = FlightPose {
        lateral: [config().flight_lateral_span - 0.001; 2],
        ..BRAKE
    };
    let mut h = ready_suit();
    assert!(h.hold(narrow, 20).iter().all(|c| !c.brake));
}

#[test]
fn a_released_brake_takes_the_whole_dwell_again() {
    let mut h = ready_suit();
    braking(&mut h);
    assert!(!h.observe(Some(FLIGHT)).brake);
    braking(&mut h);
}

#[test]
fn palms_already_rolled_as_the_suit_opens_do_not_brake_until_a_release() {
    let mut h = History::new(FALLING);
    // A fall takes the spread arms alone, so a spread with the palms to the wind opens the suit.
    assert_eq!(deploys(&h.hold(BRAKE, 6)), 1);
    h.mode = SUIT;
    assert!(h.hold(BRAKE, 20).iter().all(|c| !c.brake));
    assert_eq!(h.state.phase(), GesturePhase::AwaitingFlight);
    // One sample of the palms back to flat is enough, with no dwell or flight pose.
    assert!(!h.observe(Some(FLIGHT)).brake);
    braking(&mut h);
}

#[test]
fn a_suit_left_and_entered_again_needs_another_release() {
    let mut h = ready_suit();
    braking(&mut h);
    h.mode = FALLING;
    h.observe(Some(BRAKE));
    h.mode = SUIT;
    assert!(h.hold(BRAKE, 20).iter().all(|c| !c.brake));
    h.observe(Some(FOLDED));
    braking(&mut h);
}

#[test]
fn braking_neither_closes_the_suit_nor_arms_it() {
    let mut h = ready_suit();
    braking(&mut h);
    let held = h.hold(BRAKE, 40);
    assert!(held.iter().all(|c| c.brake && c.transition.is_none()));
    assert!(!h.state.armed);
}

#[test]
fn the_flight_rolls_keep_the_brake_entry_and_its_release_apart() {
    let mut config = *config();
    for flight_roll_deg in [15.0, 40.0, 59.9] {
        config.flight_roll_deg = flight_roll_deg;
        for roll in [config.brake_roll_deg, config.brake_roll_max_deg] {
            let pose = palms(roll);
            assert!(pose.enters_brake(&config) && !pose.releases_brake(&config));
        }
    }
}
