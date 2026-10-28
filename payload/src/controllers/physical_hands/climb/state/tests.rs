use super::*;
use crate::controllers::physical_hands::climb::engine::BodyIdentity;

fn grip(hand: Hand, body: u32, height: f32) -> Grip {
    Grip {
        hand,
        hold: Hold {
            point: Vec3::Y * height,
            normal: Vec3::Y,
            body,
            moving: true,
            identity: BodyIdentity {
                world: 3,
                bodies: 4,
                owner: body as usize,
                instance: body as usize,
                ..Default::default()
            },
        },
        ledge: true,
        body: Vec3::ZERO,
        palm: Vec3::ZERO,
        pin: (Vec3::ZERO, Quat::IDENTITY),
        frame: Some(Affine3A::IDENTITY),
        rig_epoch: 0,
        missing_steps: 0,
    }
}

fn context() -> Context {
    Context {
        character: 1,
        proxy: 2,
        world: 3,
        bodies: 4,
        epoch: 5,
    }
}

fn tuning() -> MantleTuning {
    MantleTuning {
        start_m: 0.4,
        clearance_m: 0.2,
        forward_m: 0.5,
        speed_mps: 2.0,
        min_speed_mps: 0.1,
        min_climb_m: 0.2,
        land_past_m: 0.3,
        step_s: 1.0 / 30.0,
    }
}

fn tick(tuning: &MantleTuning, feet: Vec3) -> MantleTick<'_> {
    MantleTick {
        feet,
        supported: false,
        stands: false,
        tuning,
        max_steps: MAX_MANTLE_STEPS,
        context: Some(context()),
    }
}

fn frame(transform: Affine3A) -> BodyFrame {
    BodyFrame {
        transform,
        is_static: false,
        is_keyframed: false,
        mass_kg: Some(100.0),
        centre: Vec3::ZERO,
        linear: Vec3::ZERO,
        angular: Vec3::ZERO,
    }
}

fn captured() -> (State, Grip, Grip) {
    let mut state = State::new();
    let a = grip(Hand::Left, 10, 1.0);
    let b = grip(Hand::Right, 20, 2.0);
    state.grips = [Some(a), None];
    state.smoothed = Vec3::Y;
    assert!(matches!(
        state.step_mantle(tick(&tuning(), Vec3::new(0.0, 0.8, 1.0)), |hold| {
            assert_eq!(hold, a.hold);
            (true, Some(frame(Affine3A::IDENTITY)))
        }),
        MantleStep::Move(_)
    ));
    assert_eq!(state.status.mantles, 1);
    assert_eq!(state.mantle_frame.unwrap().owner, a.hold);
    (state, a, b)
}

#[test]
fn mantle_trajectory_and_handoff_keep_the_captured_owner_when_the_handset_changes() {
    let (mut state, a, b) = captured();
    let tuning = tuning();
    for (hands, translation) in [([Some(a), Some(b)], 1.0), ([None, Some(b)], 2.0)] {
        state.grips = hands;
        assert_eq!(state.ledge_grip().unwrap().hold, b.hold);
        let moved = Affine3A::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Vec3::X * translation,
        );
        let result = state.step_mantle(tick(&tuning, Vec3::new(translation, 1.2, 0.0)), |hold| {
            assert_eq!(
                hold, a.hold,
                "advancement must not reselect the higher grip"
            );
            (true, Some(frame(moved)))
        });
        assert_eq!(state.mantle.unwrap().top, Vec3::new(translation, 1.0, 0.0));
        let over = state.mantle.unwrap().over;
        assert!((over - glam::Vec2::new(-1.0, 0.0)).length() < 1e-5);
        let MantleStep::Move(velocity) = result else {
            panic!("mantle must keep moving")
        };
        assert!(velocity.x < 0.0);
        assert!(velocity.z.abs() < 1e-5);
        assert_eq!(state.mantle_frame.unwrap().owner, a.hold);
    }
    let feet = Vec3::new(2.0, 1.0, 0.0);
    let MantleStep::Completed(Some((owner, landing))) = state.step_mantle(
        MantleTick {
            stands: true,
            ..tick(&tuning, feet)
        },
        |hold| {
            assert_eq!(hold, a.hold);
            (true, Some(frame(state_frame())))
        },
    ) else {
        panic!("mantle must complete")
    };
    assert_eq!(owner, a.hold);
    assert_eq!(landing.body, a.hold.body);
    assert_eq!(landing.identity, a.hold.identity);
    assert_ne!(landing.identity.owner, b.hold.identity.owner);
    assert_eq!(landing.feet, feet);
    assert_eq!((landing.character, landing.proxy, landing.epoch), (1, 2, 5));
}

fn state_frame() -> Affine3A {
    Affine3A::from_rotation_translation(
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        Vec3::X * 2.0,
    )
}

#[test]
fn an_invalid_captured_body_cancels_even_when_a_higher_valid_grip_remains() {
    let (mut state, a, b) = captured();
    state.grips = [None, Some(b)];
    assert!(matches!(
        state.step_mantle(tick(&tuning(), Vec3::new(0.0, 0.8, 1.0)), |hold| {
            assert_eq!(hold, a.hold);
            (false, None)
        }),
        MantleStep::Cancelled("the mantle's body changed")
    ));
}

#[test]
fn a_temporarily_missing_frame_keeps_the_captured_trajectory_and_owner() {
    let (mut state, a, _) = captured();
    let before = state.mantle;
    assert!(matches!(
        state.step_mantle(tick(&tuning(), Vec3::new(0.0, 0.8, 1.0)), |hold| {
            assert_eq!(hold, a.hold);
            (true, None)
        }),
        MantleStep::Move(_)
    ));
    assert_eq!(state.mantle, before);
    assert_eq!(state.mantle_frame.unwrap().owner, a.hold);
}
