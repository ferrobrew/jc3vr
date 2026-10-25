use super::*;
use crate::controllers::body::rig::estimate::{
    contacts::{rest_feet, yaw},
    state::carry_planar_rates,
    tests::regressions::context,
};

fn overlap_contacts(fixture: &Fixture, previous_step: u64, step: u64) -> [Contact; 2] {
    let rest = rest_feet(
        Stance {
            origin: Vec3::ZERO,
            yaw: 0.0,
        },
        fixture.anatomy.character,
        fixture.anatomy.ankle_height,
    );
    let mut left = rest[0];
    left.position.x = 0.12;
    let mut old_right = rest[1];
    old_right.position.x = 0.50;
    [
        Contact::seed(
            left,
            Some(fixture.support(previous_step, step)),
            fixture.anatomy.ankle_height,
        ),
        Contact::released(old_right, ReleaseReason::Unreachable),
    ]
}

#[test]
fn final_realized_separation_rejects_stale_released_counterexample() {
    let fixture = Fixture::new();
    let mut context = context(&fixture, Sample::standing(1, 0.01));
    context.predicted.0[5] = 0.84;
    context.contacts = overlap_contacts(&fixture, 0, 1);
    let right = rest_feet(
        context.predicted.stance(0.0),
        fixture.anatomy.character,
        fixture.anatomy.ankle_height,
    )[1]
    .position;
    assert!(
        (context.contacts[1].pose.position.x - context.contacts[0].pose.position.x - 0.38).abs()
            < 1.0e-6
    );
    assert!((right.x - 0.126).abs() < 1.0e-6);
    assert!(!fit::realized_separation(
        [context.contacts[0].pose.position, right],
        0.0,
        fixture.anatomy.character.hip_width
    ));
    assert!(fit::evaluate(context.predicted, &context).is_none());
    context.previous = context.predicted;
    context.continuity = true;
    let contacts = context.contacts;
    let (candidate, selected, _) =
        crate::controllers::body::rig::estimate::state::fit_contacts(&mut context, contacts)
            .unwrap();
    assert!(!selected[0].active());
    assert!(fit::realized_separation(
        candidate.body.ankles,
        candidate.parameters.0[2],
        fixture.anatomy.character.hip_width
    ));
}

#[test]
fn full_update_relaxes_overlap_instead_of_publishing_feasible_crossed_feet() {
    let fixture = Fixture::new();
    let mut estimator = Estimator::new();
    fixture
        .tick(
            &mut estimator,
            Sample::standing(1, 0.01),
            Transition::Continue,
        )
        .unwrap();
    estimator.seed_contacts_for_test(overlap_contacts(&fixture, 1, 2), 0.84);
    let mut sample = Sample::standing(2, 0.01);
    let ratio = fixture.anatomy.player_segments.neutral_hip_height
        / fixture.anatomy.character.neutral_hip_height;
    sample.head.position.y =
        0.84 * ratio + fixture.anatomy.player_segments.torso() - EYES_TO_NECK_PIVOT.y;
    let snapshot = fixture
        .tick(&mut estimator, sample, Transition::Continue)
        .unwrap();
    assert_anatomy(snapshot, fixture.anatomy);
    assert!(fit::realized_separation(
        snapshot.body.ankles,
        yaw(snapshot.body.pelvis_rotation),
        fixture.anatomy.character.hip_width
    ));
    assert!(!matches!(
        snapshot.contacts[0],
        ContactState::Planted { .. }
    ));
    assert!(snapshot.quality.reduced_authority);
}

#[test]
fn planar_rate_transport_rotates_only_world_stance_corrections_before_temporal_bounds() {
    let fixture = Fixture::new();
    let mut rates = [0.0; fit::PARAMETERS];
    rates[0] = 1.0;
    rates[3] = 0.2;
    rates[4] = -0.3;
    rates[8] = 0.4;
    let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let carried = carry_planar_rates(rates, turn);
    assert!(Vec3::new(carried[0], 0.0, carried[1]).abs_diff_eq(turn * Vec3::X, 1.0e-6));
    assert_eq!(&carried[2..], &rates[2..]);
    let mut context = context(&fixture, Sample::standing(1, 0.01));
    context.continuity = true;
    context.rates = carried;
    let (minimum_x, maximum_x) =
        crate::controllers::body::rig::estimate::temporal::interval(0, &context).unwrap();
    assert!(minimum_x <= 0.0 && maximum_x >= 0.0);
    let (minimum_z, maximum_z) =
        crate::controllers::body::rig::estimate::temporal::interval(1, &context).unwrap();
    assert!(minimum_z < -0.008 && maximum_z < -0.008);
}

#[test]
fn planar_envelope_is_equivalent_under_arbitrary_world_yaw_and_translation() {
    let fixture = Fixture::new();
    let mut base = context(&fixture, Sample::standing(1, 0.01));
    base.continuity = true;
    base.rates[0] = 0.8;
    base.rates[1] = -0.35;
    let mut target = base.previous;
    target.0[0] += 0.02;
    target.0[1] -= 0.01;
    let turn = Quat::from_rotation_y(0.63);
    let transport = Mat4::from_rotation_translation(turn, Vec3::new(0.3, 0.0, -0.2));
    let mut moved = context(&fixture, Sample::standing(1, 0.01));
    moved.continuity = true;
    moved.root_yaw = 0.63;
    moved.rates = carry_planar_rates(base.rates, turn);
    moved.previous.0[0] = transport.w_axis.x;
    moved.previous.0[1] = transport.w_axis.z;
    moved.previous.0[2] = 0.63;
    let transformed_target = transport.transform_point3(target.stance(0.0).origin);
    let mut actual = target;
    actual.0[0] = transformed_target.x;
    actual.0[1] = transformed_target.z;
    actual.0[2] = 0.63;
    fit::constrain(&mut target, &base).unwrap();
    fit::constrain(&mut actual, &moved).unwrap();
    assert!(crate::controllers::body::rig::estimate::temporal::accepts(
        target, &base
    ));
    assert!(crate::controllers::body::rig::estimate::temporal::accepts(
        actual, &moved
    ));
    assert!(actual.stance(0.0).origin.abs_diff_eq(
        transport.transform_point3(target.stance(0.0).origin),
        1.0e-6
    ));
}

#[test]
fn recenter_explicitly_resets_nonzero_correction_rates() {
    let mut fixture = Fixture::new();
    let mut estimator = Estimator::new();
    fixture
        .tick(
            &mut estimator,
            Sample::standing(1, 0.01),
            Transition::Continue,
        )
        .unwrap();
    estimator.seed_nonviable_rate_for_test(0, 0.0, 1.0);
    estimator.seed_nonviable_rate_for_test(3, 0.0, 0.2);
    fixture.epochs.origin += 1;
    let sample = Sample {
        previous_step: 2,
        basis: Quat::from_rotation_y(0.9),
        ..Sample::standing(2, 0.01)
    };
    let snapshot = fixture
        .tick(&mut estimator, sample, Transition::Recenter)
        .unwrap();
    assert_eq!(estimator.rates_for_test(), [0.0; fit::PARAMETERS]);
    assert_eq!(snapshot.gait.completed_across_support, Vec3::ZERO);
    assert_anatomy(snapshot, fixture.anatomy);
}

#[test]
fn nonzero_correction_stream_is_equivalent_through_root_and_support_yaw_translation() {
    let fixture = Fixture::new();
    let dt = 0.01;
    let mut fixed = Estimator::new();
    let mut carried = Estimator::new();
    let initial = Sample::standing(1, dt);
    fixture
        .tick(&mut fixed, initial, Transition::Continue)
        .unwrap();
    fixture
        .tick(&mut carried, initial, Transition::Continue)
        .unwrap();
    fixed.seed_nonviable_rate_for_test(0, 0.0, 1.0);
    carried.seed_nonviable_rate_for_test(0, 0.0, 1.0);
    let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let transport = Mat4::from_rotation_translation(turn, Vec3::new(0.3, 0.0, -0.2));
    for step in 2..=4 {
        let base = Sample::standing(step, dt);
        let expected = fixture
            .tick(&mut fixed, base, Transition::Continue)
            .unwrap();
        let mut moved = base;
        moved.head.position = transport.transform_point3(base.head.position);
        moved.head.orientation = turn * base.head.orientation;
        moved.root = transport;
        moved.previous_root = if step == 2 { Mat4::IDENTITY } else { transport };
        let source = fixture.observation(moved);
        let raw = *fixture.observation(base).raw();
        let mut previous = *source
            .pair()
            .side(crate::controllers::body::rig::observation::PairSide::Previous);
        let mut current = *source
            .pair()
            .side(crate::controllers::body::rig::observation::PairSide::Current);
        previous.anchor_world = moved.previous_root.w_axis.truncate();
        previous.basis.body = if step == 2 { Quat::IDENTITY } else { turn };
        current.anchor_world = transport.w_axis.truncate();
        current.basis.body = turn;
        let observation =
            Observation::new(raw, CompositionPair::new(previous, current).unwrap()).unwrap();
        let mut support = fixture.support(step - 1, step);
        support.previous = moved.previous_root;
        support.current = transport;
        let actual = carried
            .update(Update {
                observation: &observation,
                now: fixture.now,
                max_age: Duration::from_millis(100),
                dt,
                anatomy: &fixture.anatomy,
                support: Some(support),
                transition: Transition::Continue,
            })
            .unwrap();
        assert_anatomy(actual, fixture.anatomy);
        assert!(
            transport
                .inverse()
                .transform_point3(actual.stance.origin)
                .abs_diff_eq(expected.stance.origin, 0.003)
        );
        if step == 2 {
            let transported_control = transport.transform_point3(expected.stance.origin);
            assert!(
                (actual.stance.origin.x - transported_control.x).abs() < 0.002,
                "step {step}: expected={expected:?}, actual={actual:?}, fixed_state={:?}, carried_state={:?}",
                fixed.state_for_test(),
                carried.state_for_test()
            );
            assert!(carried.rates_for_test()[1] < -0.8);
        }
        assert!(
            transport
                .inverse()
                .transform_point3(actual.body.pelvis)
                .abs_diff_eq(expected.body.pelvis, 0.004)
        );
        assert!(
            transport
                .inverse()
                .transform_point3(actual.body.neck)
                .abs_diff_eq(expected.body.neck, 0.004)
        );
        for side in 0..2 {
            match (actual.contacts[side], expected.contacts[side]) {
                (
                    ContactState::Planted {
                        support: a,
                        local: left,
                    },
                    ContactState::Planted {
                        support: b,
                        local: right,
                    },
                ) => {
                    assert_eq!(a, b);
                    assert!(left.position.abs_diff_eq(right.position, 1.0e-5));
                    assert!(left.rotation.abs_diff_eq(right.rotation, 1.0e-5));
                }
                (ContactState::Released(a), ContactState::Released(b)) => assert_eq!(a, b),
                (
                    ContactState::Swinging {
                        support: a,
                        progress: left,
                        ..
                    },
                    ContactState::Swinging {
                        support: b,
                        progress: right,
                        ..
                    },
                ) => {
                    assert_eq!(a, b);
                    assert!((left - right).abs() < 1.0e-5);
                }
                states => panic!("different inferred contacts after frame transport: {states:?}"),
            }
        }
        assert!(
            actual
                .gait
                .completed_across_support
                .abs_diff_eq(Vec3::ZERO, 1.0e-6)
        );
        assert_eq!(actual.quality.temporal, expected.quality.temporal);
        let expected_rates = carry_planar_rates(fixed.rates_for_test(), turn);
        let actual_rates = carried.rates_for_test();
        assert!(
            Vec3::new(actual_rates[0], 0.0, actual_rates[1])
                .abs_diff_eq(Vec3::new(expected_rates[0], 0.0, expected_rates[1]), 0.03)
        );
        for axis in 2..fit::PARAMETERS {
            assert!((actual_rates[axis] - expected_rates[axis]).abs() < 0.03);
        }
        let local = transport.inverse().transform_point3(actual.stance.origin);
        assert!((local.z - expected.stance.origin.z).abs() < 0.001);
    }
}
