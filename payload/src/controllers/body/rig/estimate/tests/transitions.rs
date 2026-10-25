use super::*;

#[test]
fn duplicate_steps_and_render_reads_do_not_advance_state() {
    let fixture = Fixture::new();
    let sample = Sample::standing(1, 1.0 / 90.0);
    let mut estimator = Estimator::new();
    let first = fixture
        .tick(&mut estimator, sample, Transition::Continue)
        .unwrap();
    for _ in 0..8 {
        assert_eq!(
            fixture
                .tick(&mut estimator, sample, Transition::Continue)
                .unwrap(),
            first
        );
        assert_eq!(estimator.snapshot(), Some(&first));
    }
    let mut changed = sample;
    changed.head.position.x += 0.01;
    assert_eq!(
        fixture.tick(&mut estimator, changed, Transition::Continue),
        Err(EstimateError::ChangedCommittedStep)
    );
    assert_eq!(estimator.snapshot(), Some(&first));
}

#[test]
fn reused_tracking_does_not_become_new_measurement_evidence() {
    let fixture = Fixture::new();
    let mut estimator = Estimator::new();
    let first = Sample::standing(1, 1.0 / 90.0);
    fixture
        .tick(&mut estimator, first, Transition::Continue)
        .unwrap();
    let second = Sample {
        sequence: first.sequence,
        time: first.time,
        ..Sample::standing(2, first.dt)
    };
    let snapshot = fixture
        .tick(&mut estimator, second, Transition::Continue)
        .unwrap();
    assert!(!snapshot.provenance.fresh_measurement);
    assert_eq!(snapshot.provenance.sample.sequence, 1);
    assert_eq!(snapshot.provenance.current.step, 2);
    assert_eq!(snapshot.quality.lower_body_confidence, Confidence::Low);
}

#[test]
fn recenter_is_world_equivalent_and_preserves_contact_coordinates() {
    let mut fixture = Fixture::new();
    let mut unchanged = Estimator::new();
    let mut rebased = Estimator::new();
    let dt = 1.0 / 90.0;
    fixture
        .tick(
            &mut unchanged,
            Sample::standing(1, dt),
            Transition::Continue,
        )
        .unwrap();
    let before = fixture
        .tick(&mut rebased, Sample::standing(1, dt), Transition::Continue)
        .unwrap();
    let expected = fixture
        .tick(
            &mut unchanged,
            Sample::standing(2, dt),
            Transition::Continue,
        )
        .unwrap();
    fixture.epochs.origin += 1;
    let mut sample = Sample::standing(2, dt);
    sample.previous_step = 2;
    sample.basis = Quat::from_rotation_y(1.1);
    let actual = fixture
        .tick(&mut rebased, sample, Transition::Recenter)
        .unwrap();
    assert!(actual.body.pelvis.abs_diff_eq(expected.body.pelvis, 0.02));
    assert!(actual.body.neck.abs_diff_eq(expected.body.neck, 0.02));
    assert_eq!(actual.contacts, before.contacts);
    assert_eq!(actual.gait.distance, 0.0);
    assert_eq!(actual.gait.completed_across_support, Vec3::ZERO);
}

#[test]
fn support_transport_does_not_pace_walking_or_slide_planted_feet() {
    let fixture = Fixture::new();
    let dt = 1.0 / 90.0;
    let mut estimator = Estimator::new();
    let first = fixture
        .tick(
            &mut estimator,
            Sample::standing(1, dt),
            Transition::Continue,
        )
        .unwrap();
    let transport = Vec3::new(0.04, 0.0, 0.0);
    let mut sample = Sample::standing(2, dt);
    sample.root = Mat4::from_translation(transport);
    sample.head.position += transport;
    let observation = fixture.observation(sample);
    let mut support = fixture.support(1, 2);
    support.current = sample.root;
    let actual = estimator
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
    assert_eq!(actual.gait.completed_across_support, Vec3::ZERO);
    assert_eq!(actual.gait.distance, 0.0);
    for side in 0..2 {
        assert!(
            actual.feet[side]
                .position
                .abs_diff_eq(first.feet[side].position + transport, 1.0e-4)
        );
    }
}

#[test]
fn blocked_root_requests_and_pelvis_motion_do_not_credit_gait() {
    let fixture = Fixture::new();
    let mut estimator = Estimator::new();
    for step in 1..=120 {
        let mut sample = Sample::standing(step, 1.0 / 60.0);
        sample.head.position.x = 0.7 * (step as f32 / 120.0);
        let snapshot = fixture
            .tick(&mut estimator, sample, Transition::Continue)
            .unwrap();
        assert_eq!(snapshot.gait.completed_across_support, Vec3::ZERO);
        assert_eq!(snapshot.gait.distance, 0.0);
        assert_eq!(snapshot.gait.phase, 0.0);
    }
    let snapshot = estimator.snapshot().unwrap();
    assert!(snapshot.root.displacement.x > 0.1 || snapshot.quality.reduced_authority);
    assert!(snapshot.root.permitted);
}

#[test]
fn completed_root_translation_paces_gait_even_without_a_request() {
    let fixture = Fixture::new();
    let dt = 1.0 / 60.0;
    let mut estimator = Estimator::new();
    fixture
        .tick(
            &mut estimator,
            Sample::standing(1, dt),
            Transition::Continue,
        )
        .unwrap();
    let mut sample = Sample::standing(2, dt);
    sample.root = Mat4::from_translation(Vec3::new(0.02, 0.0, -0.03));
    sample.head.position += sample.root.w_axis.truncate();
    let snapshot = fixture
        .tick(&mut estimator, sample, Transition::Continue)
        .unwrap();
    assert!(
        snapshot
            .gait
            .completed_across_support
            .abs_diff_eq(Vec3::new(0.02, 0.0, -0.03), 1.0e-5)
    );
    assert!((snapshot.gait.distance - Vec3::new(0.02, 0.0, -0.03).length()).abs() < 1.0e-5);
    assert!(snapshot.gait.phase > 0.0);
}

#[test]
fn support_loss_releases_inferred_contacts_without_inventing_ground() {
    let fixture = Fixture::new();
    let mut estimator = Estimator::new();
    fixture
        .tick(
            &mut estimator,
            Sample::standing(1, 1.0 / 90.0),
            Transition::Continue,
        )
        .unwrap();
    let observation = fixture.observation(Sample::standing(2, 1.0 / 90.0));
    let snapshot = estimator
        .update(Update {
            observation: &observation,
            now: fixture.now,
            max_age: Duration::from_millis(100),
            dt: 1.0 / 90.0,
            anatomy: &fixture.anatomy,
            support: None,
            transition: Transition::Continue,
        })
        .unwrap();
    assert!(
        snapshot
            .contacts
            .iter()
            .all(|contact| matches!(contact, ContactState::Released(ReleaseReason::LostSupport)))
    );
    assert!(snapshot.quality.reduced_authority);
}

#[test]
fn explicit_teleport_reseeds_without_deriving_velocity_or_gait() {
    let mut fixture = Fixture::new();
    let mut estimator = Estimator::new();
    fixture
        .tick(
            &mut estimator,
            Sample::standing(1, 1.0 / 90.0),
            Transition::Continue,
        )
        .unwrap();
    fixture.epochs.origin += 1;
    let mut sample = Sample::standing(2, 1.0 / 90.0);
    let transport = Vec3::new(4.0, 0.0, 3.0);
    sample.root = Mat4::from_translation(transport);
    sample.previous_root = sample.root;
    sample.previous_step = sample.step;
    sample.head.position += transport;
    let snapshot = fixture
        .tick(&mut estimator, sample, Transition::Teleport)
        .unwrap();
    assert_eq!(snapshot.gait.distance, 0.0);
    assert!(snapshot.body.pelvis.x > 3.0);
    assert_eq!(snapshot.provenance.transition, Transition::Teleport);
}

#[test]
fn explicit_reseed_accepts_new_calibration_and_resets_unobserved_rates() {
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
    fixture.epochs.calibration += 1;
    fixture.anatomy.calibration = fixture.epochs.calibration;
    let reseeded = fixture
        .tick(
            &mut estimator,
            Sample::standing(2, 0.01),
            Transition::Reseed,
        )
        .unwrap();
    assert_eq!(reseeded.provenance.transition, Transition::Reseed);
    assert_eq!(estimator.rates_for_test(), [0.0; fit::PARAMETERS]);
    assert_eq!(reseeded.gait.distance, 0.0);
    assert_anatomy(reseeded, fixture.anatomy);
}

#[test]
fn invalid_inputs_preserve_the_last_committed_snapshot() {
    let fixture = Fixture::new();
    let mut estimator = Estimator::new();
    let first = fixture
        .tick(
            &mut estimator,
            Sample::standing(1, 1.0 / 90.0),
            Transition::Continue,
        )
        .unwrap();
    let mut missed = Sample::standing(3, 1.0 / 90.0);
    assert_eq!(
        fixture.tick(&mut estimator, missed, Transition::Continue),
        Err(EstimateError::MissedStep)
    );
    missed.step = 2;
    missed.previous_step = 1;
    missed.dt = f32::NAN;
    assert_eq!(
        fixture.tick(&mut estimator, missed, Transition::Continue),
        Err(EstimateError::InvalidTimeStep)
    );
    let observation = fixture.observation(Sample::standing(2, 1.0 / 90.0));
    assert_eq!(
        estimator.update(Update {
            observation: &observation,
            now: fixture.now + Duration::from_secs(1),
            max_age: Duration::from_millis(100),
            dt: 1.0 / 90.0,
            anatomy: &fixture.anatomy,
            support: Some(fixture.support(1, 2)),
            transition: Transition::Continue,
        }),
        Err(EstimateError::Observation(ObservationError::StaleSample))
    );
    assert_eq!(estimator.snapshot(), Some(&first));
}

#[test]
fn mismatched_support_pair_and_unannounced_origin_change_are_errors() {
    let mut fixture = Fixture::new();
    let mut estimator = Estimator::new();
    fixture
        .tick(
            &mut estimator,
            Sample::standing(1, 1.0 / 90.0),
            Transition::Continue,
        )
        .unwrap();
    let observation = fixture.observation(Sample::standing(2, 1.0 / 90.0));
    let mut support = fixture.support(1, 2);
    support.previous = Mat4::from_translation(Vec3::X);
    assert_eq!(
        estimator.update(Update {
            observation: &observation,
            now: fixture.now,
            max_age: Duration::from_millis(100),
            dt: 1.0 / 90.0,
            anatomy: &fixture.anatomy,
            support: Some(support),
            transition: Transition::Continue,
        }),
        Err(EstimateError::SupportPairMismatch)
    );
    fixture.epochs.origin += 1;
    assert_eq!(
        fixture.tick(
            &mut estimator,
            Sample::standing(2, 1.0 / 90.0),
            Transition::Continue
        ),
        Err(EstimateError::UnexpectedRecenter)
    );
}

#[test]
fn extreme_tracker_target_reports_residual_without_stretching() {
    let fixture = Fixture::new();
    let mut sample = Sample::standing(1, 1.0 / 90.0);
    sample.head.position.y = 4.0;
    let snapshot = fixture
        .tick(&mut Estimator::new(), sample, Transition::Continue)
        .unwrap();
    assert_anatomy(snapshot, fixture.anatomy);
    assert!(!snapshot.quality.feasible);
    assert!(snapshot.quality.reduced_authority);
    assert!(snapshot.quality.neck_residual_m > 1.0);
}

#[test]
fn unavailable_hand_evidence_does_not_poison_the_other_hand_or_head() {
    let fixture = Fixture::new();
    let mut sample = Sample::standing(1, 1.0 / 90.0);
    sample.hands = [Some(Pose {
        position: Vec3::new(-0.2, 1.2, -0.2),
        orientation: Quat::IDENTITY,
    }); 2];
    let observation = fixture.observation(sample);
    let mut raw = *observation.raw();
    raw.grips[0].as_mut().unwrap().tracked.position_valid = false;
    raw.grips[0].as_mut().unwrap().tracked.position_tracked = false;
    raw.grips[0].as_mut().unwrap().tracked.pose.position = Vec3::NAN;
    raw.grips[1].as_mut().unwrap().tracked.position_tracked = false;
    let observation = Observation::new(raw, *observation.pair()).unwrap();
    let snapshot = Estimator::new()
        .update(Update {
            observation: &observation,
            now: fixture.now,
            max_age: Duration::from_millis(100),
            dt: sample.dt,
            anatomy: &fixture.anatomy,
            support: Some(fixture.support(0, 1)),
            transition: Transition::Continue,
        })
        .unwrap();
    assert_eq!(snapshot.quality.hand_reach_residual_m[0], None);
    assert!(snapshot.quality.hand_reach_residual_m[1].is_some());
    assert!(!snapshot.quality.hands[1].unwrap().position_measured);
    assert!(snapshot.quality.head.position_measured);
}
