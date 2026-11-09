use std::num::NonZeroUsize;

use glam::Vec3;

use crate::{
    controllers::frame::Hand,
    grapple::seat::{Act, FLIGHT_S, Phase, Refusal, SeatTickInput, WireReading, step},
};

#[test]
fn a_held_attachment_waits_for_release_without_timing_out() {
    let mut input = input(true, false, Some(AT));
    input.held = true;
    let (phase, act) = step(flying(0.1), &input, Some(WireReading::Attached));
    assert_eq!(act, Act::None);
    assert!(matches!(phase, Phase::Flying { flown_s: 0.0, .. }));
}

#[test]
fn a_long_hold_released_at_a_second_point_starts_a_dual_tether() {
    let second = AT + Vec3::X * 5.0;
    let phase = Phase::Flying {
        wire: wire(),
        flown_s: 0.0,
        held_s: 0.5,
        first: AT,
    };
    assert_eq!(
        step(
            phase,
            &input(true, false, Some(second)),
            Some(WireReading::Attached)
        ),
        (Phase::Idle, Act::Tether(wire(), Some(second)))
    );
}

#[test]
fn refire_waits_for_the_native_cancel_acknowledgement() {
    let cancelling = Phase::Cancelling {
        wire: wire(),
        target: Some(AT),
    };
    let input = input(true, false, None);
    assert_eq!(
        step(cancelling, &input, Some(WireReading::Gone)),
        (cancelling, Act::None)
    );
    assert_eq!(
        step(cancelling, &input, Some(WireReading::Ready)),
        (Phase::Idle, Act::Fire(AT))
    );
}

#[test]
fn retract_all_discards_a_pending_refire() {
    let cancelling = Phase::Cancelling {
        wire: wire(),
        target: Some(AT),
    };
    let mut input = input(true, false, None);
    input.release_all = true;
    assert_eq!(
        step(cancelling, &input, Some(WireReading::Ready)),
        (Phase::Idle, Act::None)
    );
}

const DT: f32 = 1.0 / 30.0;
const AT: Vec3 = Vec3::new(10.0, 2.0, -30.0);

fn input(seated: bool, fire: bool, target: Option<Vec3>) -> SeatTickInput {
    SeatTickInput {
        seated,
        fire,
        held: false,
        release_all: false,
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
        held_s: 0.0,
        first: AT,
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
        (Phase::Idle, Act::Tether(wire(), None))
    );
}

#[test]
fn a_press_while_the_hook_flies_cancels_before_refiring() {
    let (phase, act) = step(
        flying(0.1),
        &input(true, true, Some(AT)),
        Some(WireReading::Flying),
    );
    assert_eq!(act, Act::Cancel(wire()));
    assert_eq!(
        phase,
        Phase::Cancelling {
            wire: wire(),
            target: Some(AT)
        }
    );
}

#[test]
fn a_wire_that_ends_before_the_hook_holds_is_a_miss() {
    assert_eq!(
        step(flying(0.1), &input(true, false, None), None),
        (Phase::Idle, Act::Missed)
    );
    assert_eq!(
        step(
            flying(0.1),
            &input(true, false, None),
            Some(WireReading::Gone)
        ),
        (
            Phase::Cancelling {
                wire: wire(),
                target: None
            },
            Act::Cancel(wire())
        )
    );
}

#[test]
fn a_hook_that_flies_too_long_or_outlives_the_seat_is_called_back() {
    assert_eq!(
        step(
            flying(FLIGHT_S - DT * 0.5),
            &input(true, false, None),
            Some(WireReading::Flying)
        ),
        (
            Phase::Cancelling {
                wire: wire(),
                target: None
            },
            Act::Cancel(wire())
        )
    );
    for reading in [WireReading::Flying, WireReading::Attached] {
        assert_eq!(
            step(flying(0.1), &input(false, false, None), Some(reading)),
            (
                Phase::Cancelling {
                    wire: wire(),
                    target: None
                },
                Act::Cancel(wire())
            )
        );
    }
}
