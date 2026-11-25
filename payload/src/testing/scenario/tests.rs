use tracing_subscriber::layer::SubscriberExt as _;

use super::*;
use crate::testing::{
    replay::observation_tests::Session,
    scenario::expect::{Check, Condition},
};

fn observed_run(replay: RunId) -> Run {
    let scenario = file::parse(Path::new("observation.toml"), "recording = 'observation.tracking'\nsettle_s = 10\n[[expect]]\nlog = 'replacement evidence'\n[[expect]]\nsignal = 'vr.recenters'\nat_least = 1\n").unwrap();
    Run {
        expectations: scenario
            .expectations
            .iter()
            .map(|expectation| Observed {
                expectation: expectation.clone(),
                tracker: Tracker::new(expectation.check, expectation.condition),
                baseline: None,
                log_matches: 0,
                lost_log_lines: 0,
            })
            .collect(),
        scenario,
        replay,
        started_at: String::new(),
        frames: 0,
        lost_log_lines: 0,
        settle_until: None,
    }
}

#[test]
fn pending_scenario_does_not_observe_the_current_replacement() {
    let mut session = Session::new();
    let older = session.request();
    session.playing(older);
    let owned = session.request();
    let mut run = observed_run(owned);
    let lines = [LogLine {
        level: tracing::Level::INFO,
        target: "fixture".into(),
        text: "replacement evidence".into(),
    }];
    assert!(matches!(session.observation(owned), Observation::Pending));
    assert!(matches!(
        run.update(session.observation(owned), &lines, 0, Instant::now()),
        Next::Continue
    ));
    assert_eq!(run.frames, 0);
    assert_eq!(run.expectations[0].log_matches, 0);
    assert!(run.expectations[1].baseline.is_none());
    assert!(!run.expectations[0].report().verdict.passed);
    session.playing(owned);
    assert!(matches!(
        run.update(session.observation(owned), &[], 0, Instant::now()),
        Next::Continue
    ));
    assert_eq!(run.frames, 1);
    assert!(run.expectations[1].baseline.is_some());
}

#[test]
fn completed_scenario_settlement_excludes_loading_playing_and_finished_replacements() {
    let lines = [LogLine {
        level: tracing::Level::INFO,
        target: "fixture".into(),
        text: "replacement evidence".into(),
    }];
    for loading in [true, false] {
        let mut session = Session::new();
        let owned = session.request();
        session.playing(owned);
        session.complete(owned);
        let mut run = observed_run(owned);
        assert!(matches!(
            run.update(session.observation(owned), &[], 0, Instant::now()),
            Next::Continue
        ));
        assert!(run.settle_until.is_some());
        let replacement = session.request();
        if loading {
            session.loading(replacement);
        } else {
            session.playing(replacement);
        }
        assert!(matches!(
            run.update(session.observation(owned), &lines, 0, Instant::now()),
            Next::Abort(_)
        ));
        assert_eq!(run.frames, 1);
        assert_eq!(run.expectations[0].log_matches, 0);
        assert!(!run.expectations[0].report().verdict.passed);
        session.stopped_preserving_last();
        assert!(matches!(session.observation(owned), Observation::Replaced));
        assert!(matches!(
            run.update(session.observation(owned), &lines, 0, Instant::now()),
            Next::Abort(_)
        ));
        assert_eq!(run.frames, 1);
    }
}

#[test]
fn an_evicted_match_cannot_pass_an_absence_check_and_does_not_taint_signals() {
    let scenario = file::parse(
        Path::new("fixture.toml"),
        "recording = 'fixture.tracking'\n[[expect]]\nlog = 'forbidden'\nat_most = 0\n",
    )
    .unwrap();
    let mut run = Run {
        expectations: vec![Observed {
            expectation: scenario.expectations[0].clone(),
            tracker: Tracker::new(Check::AtEnd, Condition::AtMost(0.0)),
            baseline: None,
            log_matches: 0,
            lost_log_lines: 0,
        }],
        scenario,
        replay: replay::start("unused".into(), Default::default()),
        started_at: String::new(),
        frames: 0,
        lost_log_lines: 0,
        settle_until: None,
    };
    let subscriber = tracing_subscriber::registry().with(log_tap::TapLayer);
    tracing::subscriber::with_default(subscriber, || {
        log_tap::start();
        tracing::info!(target: "fixture", "forbidden");
        for _ in 0..8192 {
            tracing::info!(target: "fixture", "other");
        }
        let (lines, lost) = log_tap::drain();
        log_tap::stop();
        assert_eq!(lost, 1);
        assert!(!lines.iter().any(|line| line.text.contains("forbidden")));
        run.observe(&lines, lost);
    });
    let report = run.expectations[0].report();
    assert!(!report.verdict.passed);
    assert!(report.verdict.detail.contains("inconclusive"));
    let mut independent = Observed {
        expectation: run.expectations[0].expectation.clone(),
        tracker: Tracker::new(Check::AtEnd, Condition::Is(true)),
        baseline: None,
        log_matches: 0,
        lost_log_lines: 0,
    };
    independent.expectation.probe = Probe::Status {
        label: "fixture",
        health: None,
        detail: None,
    };
    independent.tracker.observe(Value::Bool(true));
    assert!(independent.report().verdict.passed);
    replay::stop_owned(run.replay);
}
