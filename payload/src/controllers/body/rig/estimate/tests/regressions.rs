use super::*;
use crate::controllers::body::rig::{
    estimate::{
        contacts::{ENDPOINT_COMMIT_PROGRESS, ENDPOINT_SPEED, rest_feet},
        equivalence::same_observation,
        state::{predict_hips, release_trial},
        temporal::{self, TemporalStatus},
    },
    legs::swing::FootPose,
};

pub(super) fn context<'a>(fixture: &'a Fixture, sample: Sample) -> Context<'a> {
    let stance = Stance {
        origin: Vec3::ZERO,
        yaw: 0.0,
    };
    let parameters = Parameters::neutral(stance, &fixture.anatomy);
    let feet = rest_feet(
        stance,
        fixture.anatomy.character,
        fixture.anatomy.ankle_height,
    );
    Context {
        anatomy: &fixture.anatomy,
        neck: sample.head.position + EYES_TO_NECK_PIVOT,
        head_yaw: 0.0,
        root_yaw: 0.0,
        hands: [None; 2],
        tracking_floor: Vec3::ZERO,
        world_scale: 1.0,
        predicted: parameters,
        previous: parameters,
        rates: [0.0; fit::PARAMETERS],
        dt: sample.dt,
        continuity: false,
        floor_y: 0.0,
        contacts: feet.map(|pose| {
            Contact::seed(
                pose,
                Some(fixture.support(0, 1)),
                fixture.anatomy.ankle_height,
            )
        }),
    }
}

#[test]
fn unavailable_nan_retry_is_idempotent_and_meaningful_changes_are_not() {
    let fixture = Fixture::new();
    let original = fixture.observation(Sample::standing(1, 0.01));
    let mut raw = *original.raw();
    let mut invalid = raw.head;
    invalid.tracked.position_valid = false;
    invalid.tracked.orientation_valid = false;
    invalid.tracked.pose.position = Vec3::NAN;
    invalid.tracked.pose.orientation = Quat::from_xyzw(f32::NAN, 0.0, 0.0, 0.0);
    raw.grips[0] = Some(invalid);
    let observation = Observation::new(raw, *original.pair()).unwrap();
    let mut estimator = Estimator::new();
    let mut run = |observation: &Observation| {
        estimator.update(Update {
            observation,
            now: fixture.now,
            max_age: Duration::from_millis(100),
            dt: 0.01,
            anatomy: &fixture.anatomy,
            support: Some(fixture.support(0, 1)),
            transition: Transition::Continue,
        })
    };
    let first = run(&observation).unwrap();
    assert_eq!(run(&observation).unwrap(), first);
    let mut changed = raw;
    changed.head.tracked.pose.position.x += 0.01;
    let changed = Observation::new(changed, *original.pair()).unwrap();
    assert_eq!(run(&changed), Err(EstimateError::ChangedCommittedStep));
    assert!(same_observation(&observation, &observation));
    assert!(
        !observation.raw().grips[0]
            .unwrap()
            .tracked
            .pose
            .position
            .is_finite()
    );
}

#[test]
fn release_alternatives_are_independent_and_publication_matches_final_requests() {
    let fixture = Fixture::new();
    let mut context = context(&fixture, Sample::standing(1, 0.01));
    let baseline = context.contacts;
    let left = release_trial(baseline, 1);
    let right = release_trial(baseline, 2);
    assert!(!left[0].active() && left[1].active());
    assert!(right[0].active() && !right[1].active());
    assert!(baseline.iter().all(|contact| contact.planted()));
    for contacts in [left, right, release_trial(baseline, 3)] {
        context.contacts = contacts;
        let (mut candidate, _) = fit::search(&context).unwrap();
        let requested = contacts.map(|contact| contact.pose.position);
        let mut published = contacts;
        fit::publish_feet(&mut candidate, &mut published);
        for side in 0..2 {
            assert_eq!(published[side].pose.position, candidate.body.ankles[side]);
            let expected = if contacts[side].active() {
                requested[side].distance(candidate.body.ankles[side])
            } else {
                0.0
            };
            assert!((candidate.contact_errors[side] - expected).abs() < 1.0e-6);
        }
    }
}

#[test]
fn opposite_single_release_is_not_replaced_by_accumulated_double_release() {
    let fixture = Fixture::new();
    let mut context = context(&fixture, Sample::standing(1, 0.01));
    context.continuity = true;
    let baseline = context.contacts;
    context.contacts = release_trial(baseline, 3);
    let neutral = fit::evaluate(context.previous, &context).unwrap();
    let mut candidate = neutral;
    candidate.tracking_cost = 100.0;
    let first = release_trial(baseline, 1);
    let mut helped = neutral;
    helped.tracking_cost = 50.0;
    let second = release_trial(baseline, 2);
    let mut sufficient = neutral;
    sufficient.tracking_cost = 0.0;
    // Compare the exact candidate/topology pairs in the order that exposed baseline mutation.
    assert!(
        crate::controllers::body::rig::estimate::state::topology_better(
            helped, first, candidate, baseline
        )
    );
    assert!(
        crate::controllers::body::rig::estimate::state::topology_better(
            sufficient, second, helped, first
        )
    );
    let both = release_trial(baseline, 3);
    assert!(
        !crate::controllers::body::rig::estimate::state::topology_better(
            sufficient, both, sufficient, second
        )
    );
    assert!(second[0].active() && !second[1].active());
}

#[test]
fn support_translation_and_yaw_preserve_the_whole_local_swing_without_local_lead() {
    let fixture = Fixture::new();
    let support = fixture.support(0, 1);
    let context = context(&fixture, Sample::standing(1, 0.01));
    let candidate = fit::evaluate(context.predicted, &context).unwrap();
    let mut foot = context.contacts[0];
    let mut endpoint = foot.endpoint();
    endpoint.position.z -= 0.1;
    foot.lift(support, endpoint);
    let mut static_foot = foot;
    static_foot.carry(Some(support), 0.16);
    let mut moving = support;
    moving.current =
        Mat4::from_rotation_translation(Quat::from_rotation_y(0.5), Vec3::new(0.4, 0.0, -0.2));
    foot.carry(Some(moving), 0.16);
    assert!(
        moving
            .local(foot.pose)
            .position
            .abs_diff_eq(static_foot.pose.position, 1.0e-5)
    );
    let mut carried_candidate = candidate;
    carried_candidate.body.hips = candidate.body.hips.map(|hip| moving.carry_point(hip));
    let carried_previous = candidate.body.hips.map(|hip| moving.carry_point(hip));
    assert_eq!(
        predict_hips(carried_candidate, Some(carried_previous), Vec3::ZERO, 0.01),
        carried_candidate.body.hips
    );
}

#[test]
fn swing_retargets_before_commit_and_revalidates_shrunk_patch_and_lost_support() {
    let fixture = Fixture::new();
    let surface = fixture.support(0, 1);
    let context = context(&fixture, Sample::standing(1, 0.01));
    let candidate = fit::evaluate(context.predicted, &context).unwrap();
    let mut contact = context.contacts[0];
    let mut endpoint = contact.endpoint();
    endpoint.position.z -= 0.12;
    contact.lift(surface, endpoint);
    contact.carry(Some(surface), 0.08);
    let before = contact.endpoint();
    let desired = FootPose {
        position: before.position + Vec3::Z * 0.2,
        rotation: Quat::from_rotation_y(0.7),
    };
    contact.retarget(surface, desired, 0.01);
    assert!(
        contact.endpoint().position.distance(before.position) <= ENDPOINT_SPEED * 0.01 + 1.0e-6
    );
    contact.carry(Some(surface), 0.20);
    let committed = contact.endpoint();
    assert!(
        matches!(contact.state, ContactState::Swinging { progress, .. } if progress >= ENDPOINT_COMMIT_PROGRESS)
    );
    contact.retarget(surface, endpoint, 0.01);
    assert_eq!(contact.endpoint(), committed);
    contact.carry(Some(surface), 0.1);
    assert!(matches!(
        contact.state,
        ContactState::Swinging { progress: 1.0, .. }
    ));
    let mut shrunk = surface;
    shrunk.half_extent = Vec2::splat(0.001);
    contact.touchdown(
        shrunk,
        fit::endpoint_feasible(
            candidate,
            shrunk.world(contact.endpoint()),
            context.contacts[1].pose.position,
            fixture.anatomy.character,
            0,
        ),
    );
    assert!(matches!(
        contact.state,
        ContactState::Released(ReleaseReason::InvalidTouchdown)
    ));
    let mut lost = context.contacts[0];
    lost.lift(surface, endpoint);
    lost.carry(None, 0.01);
    assert!(matches!(
        lost.state,
        ContactState::Released(ReleaseReason::LostSupport)
    ));
}

#[test]
fn full_swing_stop_reverse_turn_and_failed_schedule_use_final_realized_targets() {
    let fixture = Fixture::new();
    let sample = Sample::standing(1, 0.01);
    let observation = fixture.observation(sample);
    let surface = fixture.support(0, 1);
    let mut context = context(&fixture, sample);
    context.previous.0[5] -= 0.04;
    context.predicted = context.previous;
    let mut candidate = fit::evaluate(context.predicted, &context).unwrap();
    let mut contacts = context.contacts;
    let mut endpoint = contacts[0].endpoint();
    endpoint.position.z -= 0.08;
    assert!(fit::endpoint_feasible(
        candidate,
        surface.world(endpoint),
        contacts[1].pose.position,
        fixture.anatomy.character,
        0
    ));
    contacts[0].lift(surface, endpoint);
    let update = Update {
        observation: &observation,
        now: fixture.now,
        max_age: Duration::from_millis(100),
        dt: 0.01,
        anatomy: &fixture.anatomy,
        support: Some(surface),
        transition: Transition::Continue,
    };
    let mut planted = false;
    let mut history = Vec::new();
    for step in 0..40 {
        for contact in &mut contacts {
            contact.carry(Some(surface), 0.01);
        }
        context.contacts = contacts;
        let mut current = candidate;
        if step == 20 {
            let mut turned = candidate.parameters;
            turned.0[2] += 0.08;
            current = fit::evaluate(turned, &context).unwrap();
        }
        let mut predicted = current.body.hips;
        let lead = if step < 10 {
            -0.08
        } else if step < 20 {
            0.0
        } else {
            0.06
        };
        predicted[0].z += lead;
        let endpoint = surface.world(contacts[0].endpoint());
        let endpoint_valid = fit::endpoint_feasible(
            current,
            endpoint,
            contacts[1].pose.position,
            fixture.anatomy.character,
            0,
        );
        let carried = contacts.map(|contact| contact.state);
        crate::controllers::body::rig::estimate::state::transition_contacts(
            &mut contacts,
            current,
            predicted,
            &update,
        );
        let transitioned = contacts.map(|contact| contact.state);
        context.contacts = contacts;
        let (mut fitted, selected, _) =
            crate::controllers::body::rig::estimate::state::fit_contacts(&mut context, contacts)
                .unwrap();
        contacts = selected;
        fit::publish_feet(&mut fitted, &mut contacts);
        for (side, contact) in contacts.iter().enumerate() {
            assert_eq!(contact.pose.position, fitted.body.ankles[side]);
        }
        history.push((
            step,
            carried,
            transitioned,
            contacts.map(|contact| contact.state),
            endpoint_valid,
            fitted.parameters,
            fitted.contact_errors,
            fitted.tracking_cost,
            fitted.secondary_cost,
        ));
        planted |= contacts[0].planted();
        candidate = fitted;
        context.previous = fitted.parameters;
        context.predicted = fitted.parameters;
    }
    assert!(planted, "{history:#?}");
    let mut failed = release_trial(context.contacts, 3);
    let mut small = surface;
    small.half_extent = Vec2::splat(0.001);
    let update = Update {
        support: Some(small),
        ..update
    };
    crate::controllers::body::rig::estimate::state::transition_contacts(
        &mut failed,
        candidate,
        candidate.body.hips,
        &update,
    );
    assert!(failed.iter().all(|contact| !contact.active()));
    context.contacts = failed;
    let mut fitted = fit::evaluate(candidate.parameters, &context).unwrap();
    fit::publish_feet(&mut fitted, &mut failed);
    for (side, contact) in failed.iter().enumerate() {
        assert_eq!(contact.pose.position, fitted.body.ankles[side]);
        assert_eq!(fitted.contact_errors[side], 0.0);
    }
}

#[test]
fn endpoint_validation_rejects_twist_crossing_and_incompatible_hip_articulation() {
    let fixture = Fixture::new();
    let context = context(&fixture, Sample::standing(1, 0.01));
    let candidate = fit::evaluate(context.predicted, &context).unwrap();
    let foot = context.contacts[0].pose;
    let other = context.contacts[1].pose.position;
    assert!(fit::endpoint_feasible(
        candidate,
        foot,
        other,
        fixture.anatomy.character,
        0
    ));
    let twisted = FootPose {
        rotation: Quat::from_rotation_y(1.2),
        ..foot
    };
    assert!(!fit::endpoint_feasible(
        candidate,
        twisted,
        other,
        fixture.anatomy.character,
        0
    ));
    let crossed = FootPose {
        position: other + Vec3::X * 0.1,
        ..foot
    };
    assert!(!fit::endpoint_feasible(
        candidate,
        crossed,
        other,
        fixture.anatomy.character,
        0
    ));
    let mut high = candidate;
    high.body.hips[0].y += 0.3;
    assert!(!fit::endpoint_feasible(
        high,
        foot,
        other,
        fixture.anatomy.character,
        0
    ));
    let mut deep = candidate;
    deep.body.hips[0].y = 0.12;
    deep.parameters.0[6] = 1.7;
    assert!(!fit::endpoint_feasible(
        deep,
        foot,
        other,
        fixture.anatomy.character,
        0
    ));
}

#[test]
fn bounded_coordinates_brake_and_reverse_without_inverted_intervals() {
    let fixture = Fixture::new();
    for axis in [5, 6, 7, 8] {
        let mut context = context(&fixture, Sample::standing(1, 0.01));
        context.continuity = true;
        let (lower, upper) = temporal::bounds(axis, &fixture.anatomy);
        context.previous.0[axis] = (lower + upper) * 0.5;
        for step in 0..800 {
            let before = context.previous;
            let rate = context.rates[axis];
            let mut target = before;
            target.0[axis] = if step < 400 { upper + 1.0 } else { lower - 1.0 };
            fit::constrain(&mut target, &context).unwrap();
            let next_rate = (target.0[axis] - before.0[axis]) / context.dt;
            assert!((next_rate - rate).abs() <= fit::MAX_ACCELERATION[axis] * context.dt + 1.0e-4);
            assert!(temporal::viable(
                target.0[axis],
                next_rate,
                axis,
                &fixture.anatomy
            ));
            context.previous = target;
            context.rates[axis] = next_rate;
        }
        assert!(context.previous.0[axis] < (lower + upper) * 0.5);
    }
    let mut context = context(&fixture, Sample::standing(1, 0.01));
    context.continuity = true;
    context.previous.0[8] = 0.69;
    context.rates[8] = 2.0;
    assert!(matches!(
        temporal::interval(8, &context),
        Err(EstimateError::EmptyTemporalIntersection { axis: 8 })
    ));
    assert_eq!(
        temporal::prepare(&mut context),
        TemporalStatus::RateRecovery
    );
    let mut target = context.previous;
    fit::constrain(&mut target, &context).unwrap();
    assert!(temporal::accepts(target, &context));
}

#[test]
fn nonviable_inherited_rate_recovers_and_later_packet_commits() {
    let fixture = Fixture::new();
    let mut estimator = Estimator::new();
    fixture
        .tick(
            &mut estimator,
            Sample::standing(1, 0.01),
            Transition::Continue,
        )
        .unwrap();
    estimator.seed_nonviable_rate_for_test(8, 0.69, 2.0);
    let recovered = fixture
        .tick(
            &mut estimator,
            Sample::standing(2, 0.01),
            Transition::Continue,
        )
        .unwrap();
    assert_eq!(recovered.quality.temporal, TemporalStatus::RateRecovery);
    assert!(recovered.quality.reduced_authority);
    let next = fixture
        .tick(
            &mut estimator,
            Sample::standing(3, 0.01),
            Transition::Continue,
        )
        .unwrap();
    assert_eq!(next.provenance.current.step, 3);
    assert_anatomy(next, fixture.anatomy);
}

#[test]
fn generated_feasible_skeleton_observations_keep_hidden_pose_ambiguity_explicit() {
    let fixture = Fixture::new();
    for bend in [0.0_f32, 0.3, 0.6] {
        let mut context = context(&fixture, Sample::standing(1, 0.01));
        let mut hidden = context.predicted;
        hidden.0[5] -= 0.08 * bend;
        hidden.0[6] = bend;
        hidden.0[4] = 0.12 * bend;
        let generated = fit::evaluate(hidden, &context).unwrap();
        let player = fixture.anatomy.player_segments;
        let ratio = player.neutral_hip_height / fixture.anatomy.character.neutral_hip_height;
        let rotation = Quat::from_rotation_y(hidden.0[2]);
        let player_pelvis = Vec3::new(hidden.0[0], 0.0, hidden.0[1])
            + rotation
                * Vec3::new(
                    hidden.0[3] * ratio,
                    hidden.0[5] * ratio,
                    hidden.0[4] * ratio,
                );
        context.neck = player_pelvis
            + rotation * Quat::from_rotation_x(-hidden.0[6]) * Vec3::Y * player.lower_torso
            + Quat::from_rotation_y(hidden.0[2] + hidden.0[8])
                * Quat::from_rotation_x(-hidden.0[6] - hidden.0[7])
                * Vec3::Y
                * player.upper_torso;
        let mut sample = Sample::standing(1, 0.01);
        sample.head.position = context.neck - EYES_TO_NECK_PIVOT;
        let snapshot = fixture
            .tick(&mut Estimator::new(), sample, Transition::Continue)
            .unwrap();
        assert_anatomy(snapshot, fixture.anatomy);
        assert_eq!(snapshot.quality.lower_body_confidence, Confidence::Low);
        assert!(generated.body.neck.is_finite());
    }
    let mut context = context(&fixture, Sample::standing(1, 0.01));
    context.contacts = release_trial(context.contacts, 3);
    let mut first = context.predicted;
    first.0[5] = 0.82;
    let mut second = first;
    second.0[6] = 0.25;
    let player = fixture.anatomy.player_segments;
    let ratio = player.neutral_hip_height / fixture.anatomy.character.neutral_hip_height;
    context.neck = Vec3::Y * (first.0[5] * ratio + player.torso());
    second.0[5] = first.0[5] + player.torso() * (1.0 - 0.25_f32.cos()) / ratio;
    second.0[1] += player.torso() * 0.25_f32.sin();
    let a = fit::evaluate(first, &context).unwrap();
    let b = fit::evaluate(second, &context).unwrap();
    assert!(a.body.hip_flexion != b.body.hip_flexion);
    assert!((a.neck_error - b.neck_error).abs() < 1.0e-5);
    assert!(fit::measurement_admissible(a) && fit::measurement_admissible(b));
}

#[test]
fn chest_heading_probe_covers_twist_freedom_near_planted_stance_limit() {
    let fixture = Fixture::new();
    for hand in [
        None,
        Some(Pose {
            position: Vec3::new(0.2, 1.4, -0.2),
            orientation: Quat::IDENTITY,
        }),
    ] {
        let mut context = context(&fixture, Sample::standing(1, 0.01));
        let turn = Quat::from_rotation_y(1.0);
        context.contacts = context.contacts.map(|mut contact| {
            contact.pose.position = turn * contact.pose.position;
            contact
        });
        context.predicted.0[2] = 1.0;
        context.predicted.0[8] = 0.67;
        context.head_yaw = 1.92;
        context.hands[1] = hand.map(|mut hand| {
            hand.position = turn * hand.position;
            hand
        });
        let candidate = fit::evaluate(context.predicted, &context).unwrap();
        let mut alternative = candidate.parameters;
        alternative.0[8] -= 0.35;
        let alternative = fit::evaluate(alternative, &context).unwrap();
        assert!(fit::measurement_admissible(candidate));
        assert!(fit::measurement_admissible(alternative));
        let (spread, count) = fit::ambiguity(candidate, &context);
        assert_eq!(count, fit::HEADING_PROBES);
        assert!(spread > 0.0);
    }
}

#[test]
fn coupled_recovery_seed_survives_a_rejected_predicted_neighbourhood() {
    let fixture = Fixture::new();
    let mut context = context(&fixture, Sample::standing(1, 0.01));
    context.predicted.0[5] = 0.14;
    context.predicted.0[6] = 1.74;
    context.predicted.0[7] = 0.74;
    assert!(fit::evaluate(context.predicted, &context).is_none());
    let (candidate, count) = fit::search(&context).unwrap();
    assert!(candidate.neck_error <= fit::NECK_TOLERANCE);
    assert_eq!(count, fit::SEARCH_EVALUATIONS);
    let again = fit::search(&context).unwrap().0;
    assert_eq!(candidate.parameters, again.parameters);
    context.continuity = true;
    context.predicted = candidate.parameters;
    let candidate = fit::search(&context).unwrap().0;
    assert!(temporal::accepts(candidate.parameters, &context));
}
