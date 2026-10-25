use super::*;
use crate::controllers::body::rig::{
    estimate::{
        contacts::{Contact, SWING_SECONDS},
        state::{fit_contacts, recover_constraints},
        temporal::{self, TemporalStatus},
    },
    legs::swing::FootPose,
    observation::PairSide,
};

#[test]
fn weak_heading_prior_does_not_override_tracking_driven_turn() {
    let fixture = Fixture::new();
    let mut context = regressions::context(&fixture, Sample::standing(1, 0.01));
    let (held, _) = fit::search(&context).unwrap();
    assert!(held.body.chest_yaw.abs() < 0.12, "{held:?}");
    context.head_yaw = 2.0;
    let unturned = fit::evaluate(context.previous, &context).unwrap();
    assert!(unturned.tracking_cost > 0.0);
    let (turned, _) = fit::search(&context).unwrap();
    assert!(turned.tracking_cost < unturned.tracking_cost, "{turned:?}");
    assert!(
        turned.body.chest_yaw > held.body.chest_yaw + 0.3,
        "{turned:?}"
    );
}

#[test]
fn near_extension_swing_rejects_invalid_touchdown_instead_of_inventing_a_plant() {
    let fixture = Fixture::new();
    let surface = fixture.support(0, 1);
    let context = regressions::context(&fixture, Sample::standing(1, 0.01));
    let candidate = fit::evaluate(context.previous, &context).unwrap();
    let mut contact = context.contacts[0];
    let mut endpoint = contact.endpoint();
    endpoint.position.z -= 0.08;
    assert!(!fit::endpoint_feasible(
        candidate,
        surface.world(endpoint),
        context.contacts[1].pose.position,
        fixture.anatomy.character,
        0
    ));
    contact.lift(surface, endpoint);
    contact.carry(Some(surface), 0.4);
    contact.touchdown(surface, false);
    assert_eq!(
        contact.state,
        ContactState::Released(ReleaseReason::InvalidTouchdown)
    );
}

#[test]
fn coupled_geometry_failure_can_hold_previous_pose_with_explicit_rate_reset() {
    let fixture = Fixture::new();
    let mut context = captured_coupled_failure(&fixture);
    let baseline = context.contacts;
    assert_eq!(temporal::prepare(&mut context), TemporalStatus::Normal);
    assert!(fit::evaluate(context.previous, &context).is_some());
    let mut advanced = baseline;
    for contact in &mut advanced {
        contact.carry(Some(fixture.support(0, 1)), context.dt);
    }
    assert!(matches!(
        fit_contacts(&mut context, advanced),
        Err(EstimateError::SearchExhausted)
    ));
    let (mut candidate, mut contacts, checks) =
        recover_constraints(&mut context, baseline).unwrap();
    assert_eq!(candidate.parameters, context.previous);
    assert_eq!(context.rates, [0.0; fit::PARAMETERS]);
    assert_eq!(
        contacts.map(|contact| contact.state),
        baseline.map(|contact| contact.state)
    );
    assert_eq!(checks, 4);
    assert!(temporal::accepts(candidate.parameters, &context));
    let requested = contacts[1].pose.position;
    fit::publish_feet(&mut candidate, &mut contacts);
    assert!(
        (candidate.contact_errors[1] - requested.distance(candidate.body.ankles[1])).abs() < 1.0e-6
    );
    assert_eq!(contacts[1].pose.position, candidate.body.ankles[1]);
}

#[test]
fn coupled_recovery_never_publishes_an_invalid_previous_pose() {
    let fixture = Fixture::new();
    let mut context = regressions::context(&fixture, Sample::standing(1, 0.01));
    context.continuity = true;
    context.previous.0[6] = 2.0;
    context.rates[0] = 0.5;
    let predicted = context.predicted;
    let rates = context.rates;
    let contacts = context.contacts;
    assert!(matches!(
        recover_constraints(&mut context, contacts),
        Err(EstimateError::SearchExhausted)
    ));
    assert_eq!(context.predicted, predicted);
    assert_eq!(context.rates, rates);
    assert_eq!(context.contacts, contacts);
}

#[test]
fn full_update_coupled_recovery_commits_degraded_then_resumes() {
    let fixture = Fixture::new();
    let mut estimator = Estimator::new();
    let dt = 1.0 / 60.0;
    let mut sample = Sample::standing(1, dt);
    sample.head.position.y = 0.85;
    fixture
        .tick(&mut estimator, sample, Transition::Continue)
        .unwrap();
    let context = captured_coupled_failure(&fixture);
    let mut candidate = fit::evaluate(context.previous, &context).unwrap();
    let mut contacts = context.contacts;
    fit::publish_feet(&mut candidate, &mut contacts);
    estimator.seed_candidate_for_test(candidate, context.rates, contacts);
    sample = Sample::standing(2, dt);
    sample.head.position.y = 0.85;
    let recovered = fixture
        .tick(&mut estimator, sample, Transition::Continue)
        .unwrap();
    assert_eq!(
        recovered.quality.temporal,
        TemporalStatus::ConstraintRecovery,
        "{recovered:?}"
    );
    assert!(recovered.quality.reduced_authority);
    assert_eq!(estimator.rates_for_test(), [0.0; fit::PARAMETERS]);
    assert_anatomy(recovered, fixture.anatomy);
    sample = Sample::standing(3, dt);
    sample.head.position.y = 0.85;
    let next = fixture
        .tick(&mut estimator, sample, Transition::Continue)
        .unwrap();
    assert_eq!(next.quality.temporal, TemporalStatus::Normal);
    assert_anatomy(next, fixture.anatomy);
}

#[test]
fn valid_normal_fit_is_not_replaced_by_constraint_recovery() {
    let fixture = Fixture::new();
    let mut estimator = Estimator::new();
    fixture
        .tick(
            &mut estimator,
            Sample::standing(1, 0.1),
            Transition::Continue,
        )
        .unwrap();
    let (_, _, contacts) = estimator.state_for_test().unwrap();
    estimator.seed_contacts_for_test(contacts, 0.82);
    estimator.seed_nonviable_rate_for_test(3, 0.0, 1.7);
    let normal = fixture
        .tick(
            &mut estimator,
            Sample::standing(2, 0.1),
            Transition::Continue,
        )
        .unwrap();
    assert_eq!(normal.quality.temporal, TemporalStatus::Normal);
    assert_anatomy(normal, fixture.anatomy);
}

#[test]
fn swinging_constraint_recovery_reconstructs_arc_after_publication_and_support_transport() {
    let fixture = Fixture::new();
    for turn in [0.0, 0.63] {
        let mut context = captured_coupled_failure(&fixture);
        let surface = fixture.support(0, 1);
        let mut previous = fit::evaluate(context.previous, &context).unwrap();
        let requested = context.contacts[1].pose.position;
        assert!(previous.contact_errors[1] > 0.0, "{previous:?}");
        let mut published = context.contacts;
        fit::publish_feet(&mut previous, &mut published);
        let transport =
            Mat4::from_rotation_translation(Quat::from_rotation_y(turn), Vec3::new(0.4, 0.0, -0.2));
        let mut moved = surface;
        moved.current = transport;
        let origin = transport.transform_point3(Vec3::new(
            context.previous.0[0],
            0.0,
            context.previous.0[1],
        ));
        context.previous.0[0] = origin.x;
        context.previous.0[1] = origin.z;
        context.previous.0[2] += turn;
        let original_rates = context.rates;
        let planar =
            Quat::from_rotation_y(turn) * Vec3::new(original_rates[0], 0.0, original_rates[1]);
        context.root_yaw = turn;
        context.tracking_floor = transport.w_axis.truncate();
        context.neck = transport.transform_point3(context.neck);
        context.head_yaw = turn;
        let original_state = published[1].state;
        let endpoint = published[1].endpoint();
        for iteration in 0..3 {
            if iteration > 0 {
                moved.previous = transport;
            }
            for contact in &mut published {
                contact.carry(Some(moved), 0.0);
            }
            let arc = transport.transform_point3(requested);
            assert!(published[1].pose.position.abs_diff_eq(arc, 1.0e-6));
            assert_eq!(published[1].endpoint(), endpoint);
            assert!(matches!((original_state, published[1].state),
                (ContactState::Swinging { progress: a, .. }, ContactState::Swinging { progress: b, .. }) if a == b));
            context.rates = original_rates;
            context.rates[0] = planar.x;
            context.rates[1] = planar.z;
            context.predicted = context.previous;
            for (axis, rate) in context.rates.into_iter().enumerate() {
                context.predicted.0[axis] += rate * context.dt * (-context.dt / 0.18).exp();
            }
            let baseline = published;
            let mut advanced = baseline;
            for contact in &mut advanced {
                contact.carry(Some(moved), context.dt);
            }
            assert_eq!(temporal::prepare(&mut context), TemporalStatus::Normal);
            assert!(matches!(
                fit_contacts(&mut context, advanced),
                Err(EstimateError::SearchExhausted)
            ));
            context.contacts = baseline;
            let seed = fit::evaluate(context.previous, &context).unwrap();
            let mut estimator = Estimator::new();
            let mut initial = Sample::standing(1, context.dt);
            initial.head.position.y = 0.85;
            let initial_observation = transported_observation(&fixture, initial, transport, turn);
            let mut initial_support = fixture.support(0, 1);
            initial_support.previous = transport;
            initial_support.current = transport;
            estimator
                .update(Update {
                    observation: &initial_observation,
                    now: fixture.now,
                    max_age: Duration::from_millis(100),
                    dt: context.dt,
                    anatomy: &fixture.anatomy,
                    support: Some(initial_support),
                    transition: Transition::Continue,
                })
                .unwrap();
            estimator.seed_candidate_for_test(seed, context.rates, baseline);
            let mut sample = Sample::standing(2, context.dt);
            sample.head.position.y = 0.85;
            let observation = transported_observation(&fixture, sample, transport, turn);
            let mut current_support = fixture.support(1, 2);
            current_support.previous = transport;
            current_support.current = transport;
            let recovered = estimator
                .update(Update {
                    observation: &observation,
                    now: fixture.now,
                    max_age: Duration::from_millis(100),
                    dt: context.dt,
                    anatomy: &fixture.anatomy,
                    support: Some(current_support),
                    transition: Transition::Continue,
                })
                .unwrap();
            assert_eq!(
                recovered.quality.temporal,
                TemporalStatus::ConstraintRecovery,
                "{recovered:?}"
            );
            assert!(matches!((original_state, recovered.contacts[1]),
                (ContactState::Swinging { progress: a, .. }, ContactState::Swinging { progress: b, .. }) if a == b));
            let residual = recovered.body.ankles[1].distance(arc);
            assert!(residual > 0.0 && recovered.body.ankles[1] != arc);
            assert!(
                (recovered.quality.contact_residual_m[1] - residual).abs()
                    <= (residual * 1.0e-4).max(1.0e-10)
            );
            assert_anatomy(recovered, fixture.anatomy);
            published = estimator.state_for_test().unwrap().2;
        }
    }
}

fn transported_observation(
    fixture: &Fixture,
    sample: Sample,
    transport: Mat4,
    turn: f32,
) -> Observation {
    let source = fixture.observation(sample);
    let mut before = *source.pair().side(PairSide::Previous);
    let mut after = *source.pair().side(PairSide::Current);
    for side in [&mut before, &mut after] {
        side.root = transport;
        side.anchor_world = transport.w_axis.truncate();
        side.basis.body = Quat::from_rotation_y(turn);
    }
    Observation::new(*source.raw(), CompositionPair::new(before, after).unwrap()).unwrap()
}

fn captured_coupled_failure(fixture: &Fixture) -> Context<'_> {
    let dt = 1.0 / 60.0;
    let mut sample = Sample::standing(1, dt);
    sample.head.position.y = 0.85;
    let mut context = regressions::context(fixture, sample);
    context.continuity = true;
    context.previous = Parameters([
        0.13810518,
        0.09052158,
        -0.26495385,
        -0.14843628,
        0.3054687,
        0.25164732,
        0.5945355,
        -0.061914213,
        -0.11875844,
    ]);
    context.rates = [
        -1.1220028,
        -0.6637157,
        -1.3925027,
        1.1028296,
        0.57812685,
        -0.14103055,
        -0.09394526,
        -0.15369011,
        0.038065907,
    ];
    context.predicted = context.previous;
    for (axis, rate) in context.rates.into_iter().enumerate() {
        context.predicted.0[axis] += rate * dt * (-dt / 0.18).exp();
    }
    let surface = fixture.support(0, 1);
    let left = FootPose {
        position: Vec3::new(0.016501963, 0.030000031, 0.05752656),
        rotation: Quat::from_xyzw(0.0, -0.12964113, 0.0, 0.991561),
    };
    let from = FootPose {
        position: Vec3::new(0.35596043, 0.03, 0.15614219),
        rotation: Quat::from_xyzw(0.0, 0.039419807, 0.0, 0.99922276),
    };
    let endpoint = FootPose {
        position: Vec3::new(0.22451141, 0.03, 0.1339234),
        rotation: Quat::from_xyzw(0.0, -0.12057862, 0.0, 0.9927038),
    };
    let mut right = Contact::released(from, ReleaseReason::Unreachable);
    right.lift(surface, endpoint);
    right.carry(Some(surface), SWING_SECONDS * 0.31250003);
    context.contacts = [Contact::released(left, ReleaseReason::Unreachable), right];
    context
}
