//! Exact same-context and continuing-trajectory checks for bounded computation reuse.

use std::fmt::Debug;

use super::*;
use crate::controllers::body::rig::estimate::{
    contacts::{Contact, ReleaseReason, SWING_SECONDS},
    reuse::Fits,
    temporal::{self, Envelope},
    work::Work,
};

pub(crate) fn assert_same_debug(left: &impl Debug, right: &impl Debug) {
    // Rust's round-trip float debug format distinguishes finite values and signed zero.
    assert_eq!(format!("{left:?}"), format!("{right:?}"));
}

pub(crate) fn assert_same_search(
    left: Result<fit::Candidate, EstimateError>,
    right: Result<fit::Candidate, EstimateError>,
) {
    assert_same_debug(&left, &right);
    if let (Ok(left), Ok(right)) = (left, right) {
        assert_eq!(left.parameters.0.map(f32::to_bits), right.parameters.0.map(f32::to_bits));
        assert_eq!(left.tracking_cost.to_bits(), right.tracking_cost.to_bits());
        assert_eq!(left.secondary_cost.to_bits(), right.secondary_cost.to_bits());
    }
}

pub(crate) fn assert_same_result(
    left: Result<Snapshot, EstimateError>,
    right: Result<Snapshot, EstimateError>,
) {
    match (left, right) {
        (Ok(mut left), Ok(mut right)) => {
            left.quality.evaluations = 0;
            right.quality.evaluations = 0;
            assert_same_snapshot(left, right);
        }
        (Err(left), Err(right)) => assert_eq!(left, right),
        (left, right) => panic!("optimized/reference outcome differs: {left:?} / {right:?}"),
    }
}

pub(crate) fn assert_same_snapshot(left: Snapshot, right: Snapshot) {
    assert_same_debug(&left, &right);
    assert_eq!(snapshot_bits(left), snapshot_bits(right));
}

fn snapshot_bits(snapshot: Snapshot) -> Vec<u32> {
    let mut values = Vec::new();
    let mut vector = |v: Vec3| values.extend(v.to_array());
    vector(snapshot.stance.origin);
    let body = snapshot.body;
    for v in [body.pelvis, body.chest, body.neck] { vector(v); }
    for pair in [body.shoulders, body.hips, body.knees, body.ankles,
        snapshot.player_shoulders, snapshot.gait.touchdown_hips] {
        for v in pair { vector(v); }
    }
    for v in snapshot.remapped_hands.into_iter().flatten() { vector(v); }
    for foot in snapshot.feet { vector(foot.position); }
    vector(snapshot.root.displacement);
    vector(snapshot.gait.completed_across_support);
    values.extend(body.pelvis_rotation.to_array());
    for foot in snapshot.feet { values.extend(foot.rotation.to_array()); }
    for state in snapshot.contacts {
        match state {
            ContactState::Released(_) => {}
            ContactState::Planted { local, .. } => {
                values.extend(local.position.to_array());
                values.extend(local.rotation.to_array());
            }
            ContactState::Swinging { progress, endpoint, .. } => {
                values.push(progress);
                values.extend(endpoint.position.to_array());
                values.extend(endpoint.rotation.to_array());
            }
        }
    }
    values.extend([snapshot.stance.yaw, body.chest_yaw, body.hip_flexion, body.spine_flexion,
        body.neck_twist, snapshot.root.turn, snapshot.gait.distance, snapshot.gait.phase,
        snapshot.quality.neck_residual_m, snapshot.quality.neck_limit_residual_rad,
        snapshot.quality.yaw_ambiguity]);
    values.extend(body.knee_flexion);
    values.extend(body.ankle_flexion);
    values.extend(snapshot.quality.contact_residual_m);
    values.extend(snapshot.quality.hand_reach_residual_m.into_iter().flatten());
    values.into_iter().map(f32::to_bits).collect()
}

pub(crate) fn assert_same_estimator(left: &Estimator, right: &Estimator) {
    left.assert_same_state_for_test(right);
    assert!(left.last_work().evaluations <= MAX_COMPUTED_EVALUATIONS);
    assert!(left.last_work().logical_evaluations <= MAX_EVALUATIONS);
}

#[test]
fn unchanged_admissible_fit_reuses_only_the_numerical_search() {
    let fixture = Fixture::new();
    let mut optimized = Estimator::new();
    let mut reference = Estimator::new();
    reference.set_reference_mode_for_test(true);
    let sample = Sample::standing(1, 1.0 / 90.0);
    let left = fixture.tick(&mut optimized, sample, Transition::Continue);
    let right = fixture.tick(&mut reference, sample, Transition::Continue);
    assert_same_result(left, right);
    assert_same_estimator(&optimized, &reference);
    let work = optimized.last_work();
    assert_eq!(work.preliminary_reuses, 1);
    assert_eq!(work.evaluations, fit::SEARCH_EVALUATIONS + 1 + fit::HEADING_PROBES);
    assert_eq!(work.logical_evaluations, reference.last_work().evaluations);
    assert_eq!(work.interval_constructions, fit::PARAMETERS);
    assert!(work.analytic_solves < reference.last_work().analytic_solves);
}

#[test]
fn temporal_envelope_preserves_bits_and_first_error_axis() {
    let fixture = Fixture::new();
    for dt in [1.0 / 45.0, 1.0 / 72.0, 1.0 / 90.0, 1.0 / 120.0] {
        let sample = Sample::standing(1, dt);
        let mut context = regressions::context(&fixture, sample);
        context.continuity = true;
        for axis in 0..fit::PARAMETERS {
            context.rates[axis] = 0.03 * (axis as f32 - 4.0);
        }
        temporal::prepare(&mut context);
        let mut work = Work::default();
        let envelope = Envelope::new(&context, &mut work);
        assert_eq!(work.interval_constructions, fit::PARAMETERS);
        for magnitude in [-1.0, -0.000001, 0.0, 0.000001, 1.0] {
            let mut expected = context.previous;
            for value in &mut expected.0 { *value += magnitude; }
            let mut actual = expected;
            let left = temporal::constrain(&mut expected, &context);
            let right = temporal::constrain_counted(&mut actual, &context, Some(&envelope), &mut work);
            assert_eq!(left, right);
            assert_eq!(actual.0.map(f32::to_bits), expected.0.map(f32::to_bits));
            assert_eq!(temporal::accepts(expected, &context), temporal::accepts_counted(actual, &context, Some(&envelope), &mut work));
        }
        context.previous.0[3] = 100.0;
        context.previous.0[4] = 100.0;
        let envelope = Envelope::new(&context, &mut work);
        let mut actual = context.previous;
        assert_eq!(temporal::constrain_counted(&mut actual, &context, Some(&envelope), &mut work),
            Err(EstimateError::EmptyTemporalIntersection { axis: 3 }));
    }
}

#[test]
fn release_masks_replay_with_zero_one_and_two_active_contacts() {
    let fixture = Fixture::new();
    for active in 0..=2 {
        let mut context = regressions::context(&fixture, Sample::standing(1, 1.0 / 90.0));
        // Anatomical height bounds prevent an admissible retained mask even when planar motion is free.
        context.neck.y -= 4.0;
        for side in active..2 { context.contacts[side].release(); }
        let mut left_work = Work::default();
        let mut right_work = Work::default();
        let mut left = Fits::new(&context, true, &mut left_work);
        let mut right = Fits::new(&context, false, &mut right_work);
        let expected = right.fit_contacts(context.contacts, &mut right_work);
        let actual = left.fit_contacts(context.contacts, &mut left_work);
        let (candidate, _) = expected.as_ref().expect("the full-mask fixture must retain a residual candidate");
        assert!(!fit::measurement_admissible(*candidate));
        assert_eq!(right_work.search_requests, 4);
        assert_same_debug(&actual, &expected);
        assert_eq!(left_work.search_requests, right_work.search_requests);
        assert_eq!(left_work.logical_evaluations, right_work.evaluations);
        assert_eq!(left_work.searches, 1 << active);
        assert_eq!(left_work.cache_hits, 4 - (1 << active));
    }
}

#[test]
fn failed_searches_are_cached_and_partial_failure_work_is_retained() {
    let fixture = Fixture::new();
    let mut context = regressions::context(&fixture, Sample::standing(1, 1.0 / 90.0));
    context.floor_y = f32::NAN;
    let mut work = Work::default();
    let mut fits = Fits::new(&context, true, &mut work);
    for _ in 0..2 {
        assert!(matches!(fits.fit_contacts(context.contacts, &mut work), Err(EstimateError::SearchExhausted)));
    }
    assert_eq!(work.searches, 4);
    assert_eq!(work.cache_hits, 4);
    assert_eq!(work.evaluations, 4 * fit::SEARCH_EVALUATIONS);
    assert_eq!(work.logical_evaluations, 8 * fit::SEARCH_EVALUATIONS);
    context.continuity = true;
    context.previous.0[3] = 100.0;
    let mut work = Work::default();
    let envelope = Envelope::new(&context, &mut work);
    assert!(matches!(fit::search_counted(&context, Some(&envelope), &mut work), Err(EstimateError::EmptyTemporalIntersection { axis: 3 })));
    assert_eq!(work.evaluations, 1);
    assert_eq!(work.analytic_solves, 0);
}

#[test]
fn numerical_signature_preserves_current_trial_metadata_and_planted_rotation() {
    let fixture = Fixture::new();
    let context = regressions::context(&fixture, Sample::standing(1, 1.0 / 90.0));
    let mut work = Work::default();
    let mut fits = Fits::new(&context, true, &mut work);
    let first = fits.fit_contacts(context.contacts, &mut work).unwrap();
    let mut changed = context.contacts;
    for contact in &mut changed {
        if let ContactState::Planted { support, .. } = &mut contact.state { *support = (80, 90); }
    }
    let second = fits.fit_contacts(changed, &mut work).unwrap();
    assert_same_debug(&first.0, &second.0);
    assert!(second.1.into_iter().zip(changed).all(|(left, right)| left.same_bits(right)));
    assert_eq!(work.cache_hits, 1);
    let searches = work.searches;
    changed[0].pose.rotation = Quat::from_rotation_y(0.0001);
    fits.fit_contacts(changed, &mut work).unwrap();
    assert!(work.searches > searches);
}

#[test]
fn changed_swing_history_and_released_payload_reuse_without_restoring_metadata() {
    let fixture = Fixture::new();
    let mut context = regressions::context(&fixture, Sample::standing(1, 1.0 / 90.0));
    context.neck.x += 4.0;
    let surface = fixture.support(0, 1);
    let mut changed = context.contacts;
    let mut endpoint = changed[0].pose;
    endpoint.position.z -= 0.1;
    changed[0].lift(surface, endpoint);
    changed[0].carry(Some(surface), SWING_SECONDS * 0.1);
    let mut work = Work::default();
    let mut fits = Fits::new(&context, true, &mut work);
    fits.fit_contacts(context.contacts, &mut work).unwrap();
    let before = work.searches;
    let actual = fits.fit_contacts(changed, &mut work).unwrap();
    let mut expected_work = Work::default();
    let mut baseline = Fits::new(&context, false, &mut expected_work);
    let expected = baseline.fit_contacts(changed, &mut expected_work).unwrap();
    assert_same_debug(&actual, &expected);
    assert!(work.searches - before <= 2);

    let mut released = context.contacts.map(|contact| Contact::released(contact.pose, ReleaseReason::NoSupport));
    let mut work = Work::default();
    let mut fits = Fits::new(&context, true, &mut work);
    fits.fit_contacts(released, &mut work).unwrap();
    released[0] = Contact::released(changed[0].pose, ReleaseReason::LostSupport);
    let actual = fits.fit_contacts(released, &mut work).unwrap();
    assert!(actual.1.into_iter().zip(released).all(|(left, right)| left.same_bits(right)));
    assert_eq!(work.searches, 1);
}

#[test]
fn identical_swing_position_reuses_search_without_restoring_ignored_metadata() {
    let fixture = Fixture::new();
    let context = regressions::context(&fixture, Sample::standing(1, 1.0 / 90.0));
    let surface = fixture.support(0, 1);
    let mut original = context.contacts;
    let original_endpoint = original[0].pose;
    original[0].lift(surface, original_endpoint);
    original[0].carry(Some(surface), SWING_SECONDS * 0.1);
    original[0].pose.position = context.contacts[0].pose.position;

    let mut changed_surface = surface;
    changed_surface.identity += 1;
    changed_surface.generation += 1;
    let mut changed = context.contacts;
    changed[0].pose.position += Vec3::new(0.03, 0.0, -0.04);
    changed[0].pose.rotation = Quat::from_rotation_y(-0.2);
    let mut endpoint = context.contacts[0].pose;
    endpoint.position.z -= 0.08;
    endpoint.rotation = Quat::from_rotation_y(0.4);
    changed[0].lift(changed_surface, endpoint);
    changed[0].carry(Some(changed_surface), SWING_SECONDS * 0.35);
    changed[0].pose.position = original[0].pose.position;
    changed[0].pose.rotation = Quat::from_rotation_y(0.75);

    let ContactState::Swinging { support: old_support, progress: old_progress, .. } = original[0].state else {
        panic!("the original contact must be swinging");
    };
    let ContactState::Swinging { support: new_support, progress: new_progress, .. } = changed[0].state else {
        panic!("the changed contact must be swinging");
    };
    assert_ne!(old_support, new_support);
    assert_ne!(old_progress.to_bits(), new_progress.to_bits());
    assert_eq!(original[0].pose.position.to_array().map(f32::to_bits),
        changed[0].pose.position.to_array().map(f32::to_bits));
    assert_ne!(original[0].pose.rotation.to_array().map(f32::to_bits),
        changed[0].pose.rotation.to_array().map(f32::to_bits));
    let mut history_only = changed[0];
    history_only.pose = original[0].pose;
    history_only.state = original[0].state;
    assert!(!history_only.same_bits(original[0]));

    let mut work = Work::default();
    let mut fits = Fits::new(&context, true, &mut work);
    let first = fits.fit_contacts(original, &mut work).unwrap();
    assert!(fit::measurement_admissible(first.0));
    assert!(first.1.into_iter().zip(original).all(|(left, right)| left.same_bits(right)));
    let before = work;
    let actual = fits.fit_contacts(changed, &mut work).unwrap();
    assert_eq!(work.cache_hits, before.cache_hits + 1);
    assert_eq!(work.search_requests, before.search_requests + 1);
    assert_eq!(work.searches, before.searches);
    assert_eq!(work.evaluations, before.evaluations);
    assert_eq!(work.logical_evaluations, before.logical_evaluations + fit::SEARCH_EVALUATIONS);

    let mut expected_work = Work::default();
    let mut baseline = Fits::new(&context, false, &mut expected_work);
    let expected = baseline.fit_contacts(changed, &mut expected_work).unwrap();
    assert_same_search(Ok(actual.0), Ok(expected.0));
    assert!(actual.1.into_iter().zip(expected.1).all(|(left, right)| left.same_bits(right)));
    assert!(actual.1.into_iter().zip(changed).all(|(left, right)| left.same_bits(right)));
    assert!(!actual.1[0].same_bits(first.1[0]));
}

#[test]
fn recovery_uses_fresh_intervals_and_reports_failed_normal_work() {
    let fixture = Fixture::new();
    let dt = 1.0 / 60.0;
    let mut estimator = profile::seeded_recovery_estimator(&fixture, Sample::standing(1, dt));
    let mut sample = Sample::standing(2, dt);
    sample.head.position.y = 0.85;
    let snapshot = fixture.tick(&mut estimator, sample, Transition::Continue).unwrap();
    assert_eq!(snapshot.quality.temporal, temporal::TemporalStatus::ConstraintRecovery);
    let work = estimator.last_work();
    assert_eq!(work.evaluations, work.searches * fit::SEARCH_EVALUATIONS + 4 + 1 + fit::HEADING_PROBES);
    assert_eq!(work.interval_constructions, 3 * fit::PARAMETERS);
    assert!(work.logical_evaluations >= work.evaluations);
    assert!(work.analytic_solves > 0);
}

#[test]
fn preliminary_guard_checks_private_history_bits_and_admissibility() {
    let fixture = Fixture::new();
    let context = regressions::context(&fixture, Sample::standing(1, 1.0 / 90.0));
    let candidate = fit::search(&context).unwrap().0;
    assert!(fit::measurement_admissible(candidate));
    let mut work = Work::default();
    let fits = Fits::new(&context, true, &mut work);
    assert!(fits.reuse_preliminary(candidate, context.contacts, context.contacts, &mut work));
    let mut changed = context.contacts;
    changed[0].pose.position.x = -changed[0].pose.position.x;
    assert!(!fits.reuse_preliminary(candidate, context.contacts, changed, &mut work));
    let mut residual = candidate;
    residual.tracking_cost = 0.1;
    assert!(!fits.reuse_preliminary(residual, context.contacts, context.contacts, &mut work));

    let surface = fixture.support(0, 1);
    let mut left = context.contacts[0];
    let mut endpoint = left.pose;
    endpoint.position.z -= 0.1;
    left.lift(surface, endpoint);
    let mut right = left;
    let mut other_endpoint = endpoint;
    other_endpoint.position.x += 0.01;
    right.retarget(surface, other_endpoint, context.dt);
    right.pose = left.pose;
    right.state = left.state;
    assert!(!left.same_bits(right));
    assert!(!fits.reuse_preliminary(candidate, [left, context.contacts[1]], [right, context.contacts[1]], &mut work));
}

#[test]
fn cache_keeps_its_numerical_context_when_the_source_context_changes() {
    let fixture = Fixture::new();
    let mut context = regressions::context(&fixture, Sample::standing(1, 1.0 / 90.0));
    let mut work = Work::default();
    let mut fits = Fits::new(&context, true, &mut work);
    let first = fits.fit_contacts(context.contacts, &mut work).unwrap();
    context.rates[6] = 0.75;
    context.predicted.0[6] = 0.3;
    context.continuity = true;
    context.neck.x += 0.2;
    let second = fits.fit_contacts(context.contacts, &mut work).unwrap();
    assert_same_debug(&first, &second);
    let mut fresh_work = Work::default();
    let mut fresh = Fits::new(&context, true, &mut fresh_work);
    fresh.fit_contacts(context.contacts, &mut fresh_work).unwrap();
    assert!(fresh_work.searches > 0);
    assert_eq!(fresh_work.cache_hits, 0);
}

#[test]
fn continuing_generated_multirate_root_support_and_contact_trajectories_are_exact() {
    for hz in [45.0, 72.0, 90.0, 120.0] {
        let fixture = Fixture::new();
        let mut optimized = Estimator::new();
        let mut reference = Estimator::new();
        reference.set_reference_mode_for_test(true);
        let dt = 1.0 / hz;
        for step in 1..=360 {
            let mut sample = Sample::standing(step, dt);
            let t = step as f32 * dt;
            sample.head.position += Vec3::new(0.08 * (t * 1.3).sin(), -0.18 * (t * 0.7).sin().abs(), -0.12 * (t * 0.9).sin().abs());
            sample.head.orientation = Quat::from_rotation_y(0.6 * (t * 0.8).sin()) * Quat::from_rotation_x(0.25 * t.sin());
            if step % 90 < 60 {
                sample.hands = [Some(Pose { position: Vec3::new(-0.15, 1.3, -0.2), orientation: Quat::IDENTITY }),
                    Some(Pose { position: Vec3::new(0.4, 1.1, -0.4), orientation: Quat::IDENTITY })];
            }
            let transform = |time: f32| Mat4::from_rotation_translation(Quat::from_rotation_y(0.08 * time.sin()), Vec3::new(0.03 * time, 0.0, -0.02 * time));
            sample.root = transform(t);
            sample.previous_root = transform(t - dt);
            let mut support = fixture.support(sample.previous_step, sample.step);
            support.current = transform(t * 0.2);
            support.previous = transform((t - dt) * 0.2);
            if step % 120 > 100 { support.half_extent = Vec2::splat(0.1); }
            let support = (step % 120 != 0).then_some(support);
            let observation = fixture.observation(sample);
            let update = || Update { observation: &observation, now: fixture.now, max_age: Duration::from_millis(100), dt,
                anatomy: &fixture.anatomy, support,
                transition: if step == 240 { Transition::Teleport } else { Transition::Continue } };
            let left = optimized.update(update());
            let right = reference.update(update());
            assert_same_result(left, right);
            assert_same_estimator(&optimized, &reference);
        }
    }
}
