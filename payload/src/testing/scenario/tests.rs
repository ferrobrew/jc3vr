use super::*;
use crate::testing::replay::observation_tests::Session;

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
        run.update(session.observation(owned), &lines, Instant::now()),
        Next::Continue
    ));
    assert_eq!(run.frames, 0);
    assert_eq!(run.expectations[0].log_matches, 0);
    assert!(run.expectations[1].baseline.is_none());
    assert!(!run.expectations[0].report().verdict.passed);
    session.playing(owned);
    assert!(matches!(
        run.update(session.observation(owned), &[], Instant::now()),
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
            run.update(session.observation(owned), &[], Instant::now()),
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
            run.update(session.observation(owned), &lines, Instant::now()),
            Next::Abort(_)
        ));
        assert_eq!(run.frames, 1);
        assert_eq!(run.expectations[0].log_matches, 0);
        assert!(!run.expectations[0].report().verdict.passed);
        session.stopped_preserving_last();
        assert!(matches!(session.observation(owned), Observation::Replaced));
        assert!(matches!(
            run.update(session.observation(owned), &lines, Instant::now()),
            Next::Abort(_)
        ));
        assert_eq!(run.frames, 1);
    }
}
