use std::{
    cell::Cell,
    collections::BTreeMap,
    hint::black_box,
    time::{Duration, Instant},
};

use super::*;
use crate::controllers::body::rig::{
    estimate::{
        contacts::{Contact, ContactState, ReleaseReason, SWING_SECONDS},
        fit,
        work::Work,
    },
    legs::swing::FootPose,
};

thread_local! {
    static TIMING_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn timing_active() -> bool {
    TIMING_ACTIVE.get()
}

struct TimingGuard;

impl TimingGuard {
    fn enter() -> Self {
        TIMING_ACTIVE.set(true);
        Self
    }
}

impl Drop for TimingGuard {
    fn drop(&mut self) {
        TIMING_ACTIVE.set(false);
    }
}

const UPDATES: usize = 250;
const DT: f32 = 1.0 / 90.0;

#[test]
#[ignore = "manual bounded synthetic timing profile; run explicitly in release mode"]
fn synthetic_update_profile() {
    let fixture = Fixture::new();
    profile_sequence(&fixture, "neutral", UPDATES, |step| {
        Sample::standing(step, DT)
    });
    profile_sequence(&fixture, "bend", UPDATES, |step| {
        let phase = step as f32 * 0.035;
        let mut sample = Sample::standing(step, DT);
        sample.head.position += Vec3::new(0.04 * phase.sin(), -0.12, -0.08);
        sample.head.orientation = Quat::from_rotation_x(0.22 + 0.08 * phase.sin());
        sample
    });

    let standing = Sample::standing(1, DT);
    let mut planted = Estimator::new();
    require_setup(
        fixture.tick(&mut planted, standing, Transition::Continue),
        "planting the support-loss profile state",
    );
    profile_repeated(
        &fixture,
        &planted,
        "low_release",
        UPDATES,
        Sample::standing(2, DT),
        None,
    );

    let retarget = seeded_swinging_estimator(&fixture, standing);
    let mut moving_head = Sample::standing(2, DT);
    moving_head.head.position.x = 0.04;
    profile_repeated(
        &fixture,
        &retarget,
        "retarget",
        UPDATES,
        moving_head,
        Some(fixture.support(1, 2)),
    );

    let recovery = seeded_recovery_estimator(&fixture, Sample::standing(1, 1.0 / 60.0));
    let mut recovery_sample = Sample::standing(2, 1.0 / 60.0);
    recovery_sample.head.position.y = 0.85;
    profile_repeated(
        &fixture,
        &recovery,
        "search_failure_recovery",
        UPDATES,
        recovery_sample,
        Some(fixture.support(1, 2)),
    );
}

fn profile_sequence(
    fixture: &Fixture,
    scenario: &'static str,
    updates: usize,
    mut sample_for_step: impl FnMut(u64) -> Sample,
) {
    let mut estimator = Estimator::new();
    let mut reference = Estimator::new();
    reference.set_reference_mode_for_test(true);
    let mut metrics = ProfileMetrics::new(scenario, "optimized", updates);
    let mut baseline_metrics = ProfileMetrics::new(scenario, "baseline", updates);
    for step in 1..=updates as u64 {
        let sample = sample_for_step(step);
        let observation = fixture.observation(sample);
        let support = fixture.support(sample.previous_step, sample.step);
        let (result, baseline) = if step % 2 == 0 {
            let baseline = timed_update(fixture, &mut reference, &observation, sample.dt, Some(support));
            (timed_update(fixture, &mut estimator, &observation, sample.dt, Some(support)), baseline)
        } else {
            let result = timed_update(fixture, &mut estimator, &observation, sample.dt, Some(support));
            (result, timed_update(fixture, &mut reference, &observation, sample.dt, Some(support)))
        };
        cost::assert_same_result(result.1, baseline.1);
        cost::assert_same_estimator(&estimator, &reference);
        metrics.record(result);
        baseline_metrics.record(baseline);
    }
    baseline_metrics.report();
    metrics.report();
}

fn profile_repeated(
    fixture: &Fixture,
    baseline: &Estimator,
    scenario: &'static str,
    updates: usize,
    sample: Sample,
    support: Option<crate::controllers::body::rig::estimate::Support>,
) {
    let observation = fixture.observation(sample);
    let mut metrics = ProfileMetrics::new(scenario, "optimized", updates);
    let mut baseline_metrics = ProfileMetrics::new(scenario, "baseline", updates);
    for sample_index in 0..updates {
        let mut estimator = baseline.clone();
        let mut reference = baseline.clone();
        reference.set_reference_mode_for_test(true);
        let (result, expected) = if sample_index % 2 == 0 {
            let expected = timed_update(fixture, &mut reference, &observation, sample.dt, support);
            (timed_update(fixture, &mut estimator, &observation, sample.dt, support), expected)
        } else {
            let result = timed_update(fixture, &mut estimator, &observation, sample.dt, support);
            (result, timed_update(fixture, &mut reference, &observation, sample.dt, support))
        };
        cost::assert_same_result(result.1, expected.1);
        cost::assert_same_estimator(&estimator, &reference);
        metrics.record(result);
        baseline_metrics.record(expected);
    }
    baseline_metrics.report();
    metrics.report();
}

fn timed_update(
    fixture: &Fixture,
    estimator: &mut Estimator,
    observation: &Observation,
    dt: f32,
    support: Option<crate::controllers::body::rig::estimate::Support>,
) -> (u128, Result<Snapshot, EstimateError>, Work) {
    let _guard = TimingGuard::enter();
    let start = Instant::now();
    let result = black_box(estimator.update(Update {
        observation,
        now: fixture.now,
        max_age: Duration::from_millis(100),
        dt,
        anatomy: &fixture.anatomy,
        support,
        transition: Transition::Continue,
    }));
    (start.elapsed().as_nanos(), result, estimator.last_work())
}

fn seeded_swinging_estimator(fixture: &Fixture, sample: Sample) -> Estimator {
    let mut estimator = Estimator::new();
    require_setup(
        fixture.tick(&mut estimator, sample, Transition::Continue),
        "seeding the retarget profile state",
    );
    let context = regressions::context(fixture, sample);
    let mut candidate = fit::evaluate(context.previous, &context)
        .expect("the synthetic standing fit must provide a retarget seed");
    let mut contacts = context.contacts;
    let mut endpoint = contacts[0].endpoint();
    endpoint.position.z -= 0.12;
    contacts[0].lift(fixture.support(0, 1), endpoint);
    contacts[0].carry(Some(fixture.support(0, 1)), SWING_SECONDS * 0.08);
    fit::publish_feet(&mut candidate, &mut contacts);
    estimator.seed_candidate_for_test(candidate, context.rates, contacts);
    estimator
}

pub(super) fn seeded_recovery_estimator(fixture: &Fixture, mut sample: Sample) -> Estimator {
    sample.head.position.y = 0.85;
    let mut estimator = Estimator::new();
    require_setup(
        fixture.tick(&mut estimator, sample, Transition::Continue),
        "seeding the search-recovery profile state",
    );

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
        context.predicted.0[axis] += rate * sample.dt * (-sample.dt / 0.18).exp();
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
    let mut candidate = fit::evaluate(context.previous, &context)
        .expect("the captured recovery fixture must have a finite previous candidate");
    let mut contacts = context.contacts;
    fit::publish_feet(&mut candidate, &mut contacts);
    estimator.seed_candidate_for_test(candidate, context.rates, contacts);
    estimator
}

fn require_setup<T>(result: Result<T, EstimateError>, context: &str) {
    if let Err(error) = result {
        panic!("body profile setup failed while {context}: {error:?}");
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WorkCounts {
    evaluations: usize,
    logical: usize,
    requests: usize,
    searches: usize,
    hits: usize,
    preliminary_reuses: usize,
    intervals: usize,
    solves: usize,
    error: bool,
}

struct ProfileMetrics {
    scenario: &'static str,
    mode: &'static str,
    durations_ns: Vec<u128>,
    work_counts: BTreeMap<WorkCounts, usize>,
    evaluations: Vec<usize>,
    evaluation_counts: BTreeMap<usize, usize>,
    errors: BTreeMap<String, usize>,
    recovered_updates: usize,
    swinging_updates: usize,
}

impl ProfileMetrics {
    fn new(scenario: &'static str, mode: &'static str, requested_updates: usize) -> Self {
        Self {
            scenario,
            mode,
            work_counts: BTreeMap::new(),
            durations_ns: Vec::with_capacity(requested_updates),
            evaluations: Vec::with_capacity(requested_updates),
            evaluation_counts: BTreeMap::new(),
            errors: BTreeMap::new(),
            recovered_updates: 0,
            swinging_updates: 0,
        }
    }

    fn record(&mut self, (duration_ns, result, work): (u128, Result<Snapshot, EstimateError>, Work)) {
        self.durations_ns.push(duration_ns);
        *self.work_counts.entry(WorkCounts {
            evaluations: work.evaluations,
            logical: work.logical_evaluations,
            requests: work.search_requests,
            searches: work.searches,
            hits: work.cache_hits,
            preliminary_reuses: work.preliminary_reuses,
            intervals: work.interval_constructions,
            solves: work.analytic_solves,
            error: result.is_err(),
        }).or_default() += 1;
        match result {
            Ok(snapshot) => {
                self.evaluations.push(snapshot.quality.evaluations);
                *self
                    .evaluation_counts
                    .entry(snapshot.quality.evaluations)
                    .or_default() += 1;
                if snapshot.quality.temporal
                    == crate::controllers::body::rig::estimate::temporal::TemporalStatus::ConstraintRecovery
                {
                    self.recovered_updates += 1;
                }
                if snapshot
                    .contacts
                    .iter()
                    .any(|contact| matches!(contact, ContactState::Swinging { .. }))
                {
                    self.swinging_updates += 1;
                }
            }
            Err(error) => *self.errors.entry(format!("{error:?}")).or_default() += 1,
        }
    }

    fn report(mut self) {
        self.durations_ns.sort_unstable();
        let p50_ns = percentile(&self.durations_ns, 50);
        let p95_ns = percentile(&self.durations_ns, 95);
        let max_ns = self.durations_ns.last().copied().unwrap_or_default();
        let eval_min = self.evaluations.iter().copied().min().unwrap_or_default();
        let eval_max = self.evaluations.iter().copied().max().unwrap_or_default();
        let eval_mean_milli = if self.evaluations.is_empty() {
            0
        } else {
            (self.evaluations.iter().sum::<usize>() as u128 * 1000 / self.evaluations.len() as u128)
                as usize
        };
        eprintln!(
            "BODY_PROFILE|scenario={}|mode={}|updates={}|p50_ns={}|p95_ns={}|max_ns={}|eval_n={}|eval_min={}|eval_mean_milli={}|eval_max={}|eval_counts={:?}|errors={:?}|recovered_updates={}|swinging_updates={}|work_fields=evaluations,logical,requests,searches,hits,preliminary_reuses,intervals,solves,error|work_counts={:?}",
            self.scenario,
            self.mode,
            self.durations_ns.len(),
            p50_ns,
            p95_ns,
            max_ns,
            self.evaluations.len(),
            eval_min,
            eval_mean_milli,
            eval_max,
            self.evaluation_counts,
            self.errors,
            self.recovered_updates,
            self.swinging_updates,
            self.work_counts,
        );
        self.durations_ns.clear();
    }
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let index = (sorted.len() * percentile).div_ceil(100).saturating_sub(1);
    sorted[index]
}
