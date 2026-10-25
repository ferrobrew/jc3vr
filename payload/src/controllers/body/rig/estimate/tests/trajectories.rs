use super::*;

#[test]
fn selected_bend_and_squat_prior_allocates_continuously_at_multiple_rates() {
    let mut endpoints = Vec::new();
    for rate in [45, 72, 90, 120] {
        let dt = 1.0 / rate as f32;
        let fixture = Fixture::new();
        let mut bend = Estimator::new();
        let mut squat = Estimator::new();
        let mut last_bend: Option<f32> = None;
        let mut last_squat: Option<f32> = None;
        for step in 1..=rate * 3 {
            let time = step as f32 * dt;
            let progress = ((time - 0.5) / 1.5).clamp(0.0, 1.0);
            let progress = progress * progress * (3.0 - 2.0 * progress);
            let mut bending = Sample::standing(step as u64, dt);
            bending.head.position += Vec3::new(0.0, -0.28 * progress, -0.40 * progress);
            let mut squatting = Sample::standing(step as u64, dt);
            squatting.head.position.y -= 0.40 * progress;
            let bent = fixture
                .tick(&mut bend, bending, Transition::Continue)
                .unwrap();
            let low = fixture
                .tick(&mut squat, squatting, Transition::Continue)
                .unwrap();
            assert_anatomy(bent, fixture.anatomy);
            assert_anatomy(low, fixture.anatomy);
            if let Some(previous) = last_bend {
                assert!((bent.body.hip_flexion - previous).abs() <= 3.5 * dt + 1.0e-4);
            }
            if let Some(previous) = last_squat {
                assert!((low.body.hip_flexion - previous).abs() <= 3.5 * dt + 1.0e-4);
            }
            last_bend = Some(bent.body.hip_flexion);
            last_squat = Some(low.body.hip_flexion);
            assert_eq!(bent.gait.distance, 0.0);
            assert_eq!(low.gait.distance, 0.0);
            assert!(
                bent.stance.yaw.abs() < 0.12 && low.stance.yaw.abs() < 0.12,
                "rate {rate}, step {step}: bend={bent:?}, squat={low:?}"
            );
            assert!(
                bent.root
                    .displacement
                    .abs_diff_eq(fit::horizontal(bent.stance.origin), 1.0e-5)
            );
        }
        let bent = *bend.snapshot().unwrap();
        let low = *squat.snapshot().unwrap();
        assert!(bent.body.hip_flexion > 0.20, "rate {rate}: {bent:?}");
        assert!(low.body.knee_flexion[0] > 0.45, "rate {rate}: {low:?}");
        let bend_knees = bent.body.knee_flexion.into_iter().sum::<f32>() * 0.5;
        let squat_knees = low.body.knee_flexion.into_iter().sum::<f32>() * 0.5;
        assert!(
            squat_knees > bend_knees,
            "rate {rate}: bend={bent:?}, squat={low:?}"
        );
        assert!(low.body.hip_flexion > 0.0, "rate {rate}: {low:?}");
        for (estimator, snapshot) in [(&bend, bent), (&squat, low)] {
            let q = estimator.state_for_test().unwrap().0.0;
            let preferred_back = 0.30 * fixture.anatomy.character.torso() * q[6].sin()
                + 0.12
                    * fixture.anatomy.character.leg(0)
                    * (snapshot.body.knee_flexion.into_iter().sum::<f32>() * 0.5).sin();
            assert!(
                q[4] > 0.0 && (q[4] - preferred_back).abs() < preferred_back,
                "rate {rate}: q={q:?}, preferred_back={preferred_back}, snapshot={snapshot:?}"
            );
        }
        assert!(
            low.body.pelvis.y < bent.body.pelvis.y,
            "rate {rate}: bend={bent:?}, squat={low:?}"
        );
        assert!(bent.quality.neck_residual_m < 0.09, "rate {rate}: {bent:?}");
        assert!(low.quality.neck_residual_m < 0.09, "rate {rate}: {low:?}");
        endpoints.push((bent, low));
    }
    for pair in endpoints.windows(2) {
        assert!(pair[0].0.body.pelvis.distance(pair[1].0.body.pelvis) < 0.16);
        assert!(pair[0].1.body.pelvis.distance(pair[1].1.body.pelvis) < 0.16);
    }
}

#[test]
fn interior_hand_motion_and_head_glance_keep_weak_heading_evidence_weak() {
    let fixture = Fixture::new();
    let mut estimator = Estimator::new();
    let dt = 1.0 / 90.0;
    for step in 1..=180 {
        let mut sample = Sample::standing(step, dt);
        let phase = step as f32 * dt * 4.0;
        let orientation = Quat::from_rotation_y(0.45 * phase.sin());
        let neck = Vec3::new(0.0, 1.555, 0.0);
        sample.head = Pose {
            position: neck - orientation * EYES_TO_NECK_PIVOT,
            orientation,
        };
        sample.hands = [
            Some(Pose {
                position: Vec3::new(-0.2 + 0.12 * phase.sin(), 1.2, -0.20),
                orientation: Quat::IDENTITY,
            }),
            Some(Pose {
                position: Vec3::new(0.2 + 0.12 * phase.cos(), 1.2, -0.20),
                orientation: Quat::from_rotation_y(phase),
            }),
        ];
        let snapshot = fixture
            .tick(&mut estimator, sample, Transition::Continue)
            .unwrap();
        assert!(snapshot.body.chest_yaw.abs() < 0.12, "{snapshot:?}");
        assert!(snapshot.quality.yaw_ambiguity > 0.0);
        assert_eq!(snapshot.quality.yaw_confidence, Confidence::Low);
    }
}

#[test]
fn identical_endpoints_do_not_claim_identical_unobserved_histories() {
    let fixture = Fixture::new();
    let dt = 1.0 / 90.0;
    let mut forward_history = Estimator::new();
    let mut vertical_history = Estimator::new();
    for step in 1..=180 {
        let t = step as f32 / 180.0;
        let mut forward = Sample::standing(step, dt);
        let mut vertical = forward;
        forward.head.position += Vec3::new(0.0, -0.3 * t, -0.35 * (std::f32::consts::PI * t).sin());
        vertical.head.position.y -= 0.3 * t;
        let a = fixture
            .tick(&mut forward_history, forward, Transition::Continue)
            .unwrap();
        let b = fixture
            .tick(&mut vertical_history, vertical, Transition::Continue)
            .unwrap();
        assert_anatomy(a, fixture.anatomy);
        assert_anatomy(b, fixture.anatomy);
        assert_eq!(a.quality.lower_body_confidence, Confidence::Low);
        assert_eq!(b.quality.lower_body_confidence, Confidence::Low);
    }
    let a = forward_history.snapshot().unwrap();
    let b = vertical_history.snapshot().unwrap();
    assert!(a.quality.neck_residual_m < 0.10);
    assert!(b.quality.neck_residual_m < 0.10);
    assert!(a.body.pelvis.distance(b.body.pelvis) < 0.4);
}

#[test]
fn low_head_dwell_never_adds_a_pelvis_or_knee_support() {
    let fixture = Fixture::new();
    let mut estimator = Estimator::new();
    for step in 1..=360 {
        let mut sample = Sample::standing(step, 1.0 / 60.0);
        sample.head.position.y = 0.85;
        let snapshot = fixture.tick(&mut estimator, sample, Transition::Continue).unwrap_or_else(|error| {
            let state = estimator.state_for_test();
            let viability = state.map(|(parameters, rates, _)| (0..fit::PARAMETERS)
                .map(|axis| crate::controllers::body::rig::estimate::temporal::viable(parameters.0[axis], rates[axis], axis, &fixture.anatomy))
                .collect::<Vec<_>>());
            let mut context = regressions::context(&fixture, sample);
            context.root_yaw = crate::controllers::body::rig::estimate::contacts::yaw(sample.root.to_scale_rotation_translation().1);
            if let Some((parameters, rates, contacts)) = state {
                context.previous = parameters;
                context.predicted = parameters;
                context.rates = rates;
                context.contacts = contacts;
                context.continuity = true;
            }
            let temporal = crate::controllers::body::rig::estimate::temporal::prepare(&mut context);
            for (axis, rate) in context.rates.into_iter().enumerate() {
                context.predicted.0[axis] += rate * sample.dt * (-sample.dt / 0.18).exp();
            }
            let seeds = fit::recovery_seeds(&context).map(|mut seed| {
                let constrained = fit::constrain(&mut seed, &context);
                (seed, constrained, crate::controllers::body::rig::estimate::temporal::accepts(seed, &context), fit::evaluate(seed, &context))
            });
            panic!("step {step}: {error:?}, state={state:?}, viable={viability:?}, temporal={temporal:?}, seeds={seeds:?}, last={:?}", estimator.snapshot());
        });
        assert_anatomy(snapshot, fixture.anatomy);
        assert_eq!(snapshot.quality.lower_body_confidence, Confidence::Low);
        assert_eq!(snapshot.gait.distance, 0.0);
        assert_eq!(snapshot.contacts.len(), 2);
    }
}
