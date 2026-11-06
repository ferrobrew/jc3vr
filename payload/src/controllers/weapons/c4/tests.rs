use glam::Quat;

use super::*;
use crate::controllers::{hands::held::away_from_palm, weapons::c4::tuning::CHARGE_TOP_M};

fn fresh() -> State {
    State {
        stage: Stage::Idle,
        primed: None,
        thrown: [false; 2],
        status: C4Status::default(),
        grips: [None; 2],
        detonation: None,
        was_detonating: false,
        in_flight: None,
        held_since: None,
        held_since_s: None,
        stick: None,
        preview: None,
        wheel: None,
        placed: PlacedState::NEW,
    }
}

#[test]
fn a_started_detonation_is_credited_to_the_request_or_the_game() {
    let mut state = fresh();
    state.detonation = Some(PendingDetonation {
        source: DetonationSource::WristButton,
        remaining_s: DETONATION_TIMEOUT_S,
    });
    settle_detonation(&mut state, true, 3, 0.03);
    assert_eq!(
        state.status.last_detonation,
        Some(Detonation {
            source: DetonationSource::WristButton,
            started: true,
            charges: 3,
        })
    );
    assert!(state.detonation.is_none() && !state.status.detonation_pending);
    settle_detonation(&mut state, true, 2, 0.03);
    assert_eq!(
        state.status.last_detonation.map(|d| d.source),
        Some(DetonationSource::GameInput)
    );
    assert_eq!(state.status.detonations, 2);
}

#[test]
fn an_unanswered_request_times_out() {
    let mut state = fresh();
    state.detonation = Some(PendingDetonation {
        source: DetonationSource::WristButton,
        remaining_s: DETONATION_TIMEOUT_S,
    });
    settle_detonation(&mut state, false, 1, 0.1);
    assert!(state.status.detonation_pending);
    for _ in 0..10 {
        settle_detonation(&mut state, false, 1, 0.1);
    }
    assert!(!state.status.detonation_pending);
    assert_eq!(
        state.status.last_detonation,
        Some(Detonation {
            source: DetonationSource::WristButton,
            started: false,
            charges: 1,
        })
    );
    assert_eq!(state.status.detonations, 0);
}

#[test]
fn a_detonation_stays_in_flight_until_its_charges_go_off() {
    assert_eq!(InFlight::start(0), None);
    let started = InFlight::start(2).unwrap();
    // The act has not scheduled them yet, so both still count as placed.
    let held = started.step(2, 0.3).unwrap();
    let one_left = held.step(1, 0.3).unwrap();
    assert_eq!(one_left.lowest, 1);
    assert_eq!(one_left.step(0, 0.1), None);
}

#[test]
fn a_charge_placed_mid_detonation_ends_it() {
    let one_left = InFlight::start(2).unwrap().step(1, 0.2).unwrap();
    assert_eq!(one_left.step(2, 0.1), None);
}

#[test]
fn a_detonation_whose_charges_outlast_the_limit_is_over() {
    let mut flight = InFlight::start(1);
    let mut steps = 0;
    while let Some(f) = flight {
        flight = f.step(1, 0.1);
        steps += 1;
        assert!(steps < 100);
    }
    assert!((steps as f32 * 0.1 - DETONATION_IN_FLIGHT_S).abs() < 0.15);
}

#[test]
fn the_held_charge_lies_flat_on_the_palm_with_its_sticking_face_out() {
    for hand in Hand::BOTH {
        let local = hold_local(hand, &C4);
        let away = away_from_palm(hand);
        // The top face's normal points back into the palm, so the sticking face looks out.
        assert!((local.orientation * Vec3::Y).abs_diff_eq(-away, 1e-5));
        // The top face sits just out of the palm, not in it.
        let top = local.position + local.orientation * Vec3::new(0.0, CHARGE_TOP_M, 0.0);
        assert!(top.dot(away) > 0.0 && top.dot(away) < 0.01, "{hand:?}");
    }
    let grip = Pose {
        position: Vec3::new(1.0, 2.0, 3.0),
        orientation: Quat::IDENTITY,
    };
    let m = Mat4::from(held_matrix(grip, hold_local(Hand::Right, &C4)));
    let centre = m.transform_point3(C4.centre_m);
    // The middle of the brick is half its thickness out of the palm.
    assert!((centre.x - (1.0 - CHARGE_TOP_M / 2.0 - 0.003)).abs() < 1e-5);
}
