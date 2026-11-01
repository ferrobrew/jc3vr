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
        held_since: None,
        held_since_s: None,
        stick: None,
        preview: None,
        wheel: None,
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
