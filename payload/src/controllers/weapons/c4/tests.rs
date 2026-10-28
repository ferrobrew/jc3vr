use glam::Quat;

use super::*;

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
fn the_held_charge_sits_at_the_mirrored_offset() {
    let tuning = C4Config {
        hold_offset_m: Vec3::new(0.02, 0.0, -0.05),
        ..C4Config::new()
    };
    assert_eq!(
        hold_offset(Hand::Left, &tuning),
        Vec3::new(-0.02, 0.0, -0.05)
    );
    let grip = Pose {
        position: Vec3::new(1.0, 2.0, 3.0),
        orientation: Quat::IDENTITY,
    };
    let m = Mat4::from(held_matrix(grip, hold_offset(Hand::Right, &tuning)));
    assert!(m.w_axis.truncate().distance(Vec3::new(1.02, 2.0, 2.95)) < 1e-6);
}
