use std::num::NonZeroUsize;

use glam::Vec3;

use crate::{
    controllers::frame::Hand,
    grapple::seat::{Act, FLIGHT_S, Phase, Refusal, SeatTickInput, WireReading, step},
};

const DT: f32 = 1.0 / 30.0;
const AT: Vec3 = Vec3::new(10.0, 2.0, -30.0);

fn input(seated: bool, fire: bool, target: Option<Vec3>) -> SeatTickInput {
    SeatTickInput {
        seated,
        fire,
        target,
        dt: DT,
        hand: Hand::Left,
    }
}

fn wire() -> NonZeroUsize {
    NonZeroUsize::new(0x1000).unwrap()
}

fn flying(flown_s: f32) -> Phase {
    Phase::Flying {
        wire: wire(),
        flown_s,
    }
}

#[test]
fn a_press_in_the_seat_fires_at_the_seated_aims_point() {
    assert_eq!(
        step(Phase::Idle, &input(true, true, Some(AT)), None),
        (Phase::Idle, Act::Fire(AT))
    );
}

#[test]
fn a_press_with_nothing_in_range_is_refused() {
    assert_eq!(
        step(Phase::Idle, &input(true, true, None), None),
        (Phase::Idle, Act::Refuse(Refusal::NoTarget))
    );
}

#[test]
fn nothing_fires_out_of_a_seat_or_without_a_press() {
    assert_eq!(
        step(Phase::Idle, &input(false, true, Some(AT)), None),
        (Phase::Idle, Act::None)
    );
    assert_eq!(
        step(Phase::Idle, &input(true, false, Some(AT)), None),
        (Phase::Idle, Act::None)
    );
}

#[test]
fn the_hook_holding_tethers_the_wire_to_the_vehicle() {
    assert_eq!(
        step(
            flying(0.1),
            &input(true, false, None),
            Some(WireReading::Attached)
        ),
        (Phase::Idle, Act::Tether(wire()))
    );
}

#[test]
fn a_press_while_the_hook_flies_does_not_fire_again() {
    let (phase, act) = step(
        flying(0.1),
        &input(true, true, Some(AT)),
        Some(WireReading::Flying),
    );
    assert_eq!(act, Act::None);
    assert!(matches!(phase, Phase::Flying { .. }));
}

#[test]
fn a_wire_that_ends_before_the_hook_holds_is_a_miss() {
    for reading in [None, Some(WireReading::Gone)] {
        assert_eq!(
            step(flying(0.1), &input(true, false, None), reading),
            (Phase::Idle, Act::Missed)
        );
    }
}

#[test]
fn a_hook_that_flies_too_long_or_outlives_the_seat_is_called_back() {
    assert_eq!(
        step(
            flying(FLIGHT_S - DT * 0.5),
            &input(true, false, None),
            Some(WireReading::Flying)
        ),
        (Phase::Idle, Act::Cancel(wire()))
    );
    for reading in [WireReading::Flying, WireReading::Attached] {
        assert_eq!(
            step(flying(0.1), &input(false, false, None), Some(reading)),
            (Phase::Idle, Act::Cancel(wire()))
        );
    }
}
