use std::num::NonZeroUsize;

use glam::Vec3;
use jc3vr_body::controllers::intent::grapple_button::{SeatRequest, tap_s};
use jc3vr_input::frame::Hand;

use crate::grapple::{
    seat::{Act, Pending, Phase, Refusal, SeatTickInput, Target, WireReading, step},
    tuning::GRAPPLE,
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
        released: false,
        second: None,
    };
    assert_eq!(
        step(
            phase,
            &input(true, false, Some(second)),
            Some(WireReading::Attached)
        ),
        (Phase::Idle, Act::Tether(wire(), Some(target_at(second))))
    );
}

#[test]
fn refire_waits_for_the_native_cancel_acknowledgement() {
    let cancelling = Phase::Cancelling {
        wire: wire(),
        target: Some(pending_at(AT)),
    };
    let input = input(true, false, None);
    assert_eq!(
        step(cancelling, &input, Some(WireReading::Gone)),
        (cancelling, Act::None)
    );
    assert_eq!(
        step(cancelling, &input, Some(WireReading::Ready)),
        (Phase::Idle, Act::Fire(pending_at(AT)))
    );
}

#[test]
fn clear_all_cancels_a_seat_wire_even_when_the_native_hook_handles_fire() {
    for seated in [false, true] {
        let mut input = input(seated, true, Some(AT));
        input.release_all = true;
        input.held = true;
        assert_eq!(step(Phase::Idle, &input, None), (Phase::Idle, Act::None));
        let (phase, act) = step(flying(0.1), &input, Some(WireReading::Attached));
        assert_eq!(act, Act::Cancel(wire()));
        assert_eq!(
            phase,
            Phase::Cancelling {
                wire: wire(),
                target: None
            }
        );
        assert_eq!(
            step(phase, &input, Some(WireReading::Ready)),
            (Phase::Idle, Act::None)
        );
    }
}

#[test]
fn retract_all_discards_a_pending_refire() {
    let cancelling = Phase::Cancelling {
        wire: wire(),
        target: Some(pending_at(AT)),
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
        fire: fire.then_some(SeatRequest::Press),
        held: false,
        release_all: false,
        target: target.map(target_at),
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
        released: false,
        second: None,
    }
}

#[test]
fn a_press_in_the_seat_fires_at_the_seated_aims_point() {
    assert_eq!(
        step(Phase::Idle, &input(true, true, Some(AT)), None),
        (Phase::Idle, Act::Fire(pending_at(AT)))
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
            target: Some(pending_at(AT))
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
            flying(GRAPPLE.get().seat.flight_s - DT * 0.5),
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

fn target_at(point: Vec3) -> Target {
    Target {
        point,
        hit: None,
        body: None,
    }
}

#[test]
fn a_long_hold_release_without_a_target_cancels() {
    let phase = Phase::Flying {
        wire: wire(),
        flown_s: 0.0,
        held_s: 0.8,
        first: AT,
        released: false,
        second: None,
    };
    assert_eq!(
        step(
            phase,
            &input(true, false, None),
            Some(WireReading::Attached)
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
fn a_long_hold_release_at_the_first_point_is_still_a_valid_second_target() {
    let phase = Phase::Flying {
        wire: wire(),
        flown_s: 0.0,
        held_s: 0.8,
        first: AT,
        released: false,
        second: None,
    };
    assert_eq!(
        step(
            phase,
            &input(true, false, Some(AT)),
            Some(WireReading::Attached)
        ),
        (Phase::Idle, Act::Tether(wire(), Some(target_at(AT))))
    );
}

#[test]
fn the_release_target_is_latched_while_a_hook_is_still_flying() {
    let phase = Phase::Flying {
        wire: wire(),
        flown_s: 0.0,
        held_s: 0.8,
        first: AT,
        released: false,
        second: None,
    };
    let second = AT + Vec3::X * 5.0;
    let (phase, act) = step(
        phase,
        &input(true, false, Some(second)),
        Some(WireReading::Flying),
    );
    assert_eq!(act, Act::None);
    assert_eq!(
        step(
            phase,
            &input(true, false, None),
            Some(WireReading::Attached)
        ),
        (Phase::Idle, Act::Tether(wire(), Some(target_at(second))))
    );
}

fn pending_at(point: Vec3) -> Pending {
    Pending {
        target: target_at(point),
        request: SeatRequest::Press,
        held_s: 0.0,
        released: true,
        second: Some(target_at(point)),
    }
}

#[test]
fn a_delayed_hold_does_not_need_a_second_hold_threshold() {
    let mut input = input(true, false, Some(AT));
    input.fire = Some(SeatRequest::Hold);
    input.held = true;
    let (_, Act::Fire(pending)) = step(Phase::Idle, &input, None) else {
        panic!("the hold fires")
    };
    assert_eq!(pending.held_s, tap_s());
    input.fire = None;
    input.held = false;
    let second = AT + Vec3::X * 5.0;
    input.target = Some(target_at(second));
    assert_eq!(
        step(pending.phase(wire()), &input, Some(WireReading::Attached)),
        (Phase::Idle, Act::Tether(wire(), Some(target_at(second))))
    );
}

#[test]
fn cancellation_latches_the_first_hold_release_including_an_empty_target() {
    for release_target in [None, Some(AT + Vec3::X * 5.0)] {
        let mut input = input(true, false, Some(AT));
        input.fire = Some(SeatRequest::Hold);
        input.held = true;
        let (phase, _) = step(flying(0.1), &input, Some(WireReading::Flying));
        input.fire = None;
        input.held = false;
        input.target = release_target.map(target_at);
        let (phase, _) = step(phase, &input, Some(WireReading::Gone));
        input.target = Some(target_at(AT + Vec3::X * 20.0));
        let (_, act) = step(phase, &input, Some(WireReading::Ready));
        if let Some(second) = release_target {
            let Act::Fire(pending) = act else {
                panic!("the queued hold fires after acknowledgement")
            };
            assert!(pending.released);
            assert_eq!(pending.second, Some(target_at(second)));
            assert!(pending.held_s >= tap_s());
        } else {
            assert_eq!(act, Act::None);
        }
    }
}
