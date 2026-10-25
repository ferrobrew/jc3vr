use glam::Vec2;

use super::*;
use crate::controllers::body::rig::{
    lean::Lean,
    root::{chase::Measurement, ledger::LedgerCounts, pushback::PushbackOutcome},
};

#[test]
fn initial_snapshot_preserves_unsupported_rows() {
    let snapshot = StatusCache::new().snapshot_at(Instant::now());
    for &signal in Signal::ALL {
        assert_eq!(
            snapshot.get(signal),
            if supported(signal) {
                Readout::NotPublished
            } else {
                Readout::Unsupported
            },
            "{signal:?}"
        );
    }
    assert_ne!(
        Readout::NotPublished.to_string(),
        Value::Missing.to_string()
    );
    assert_ne!(Readout::Unsupported.to_string(), Value::Missing.to_string());
}

#[test]
fn busy_cache_returns_without_waiting_and_recovers() {
    let cache = StatusCache::new();
    let now = Instant::now();
    let guard = cache.publication.lock();
    let snapshot = cache.snapshot_at(now);
    for &signal in Signal::ALL {
        assert_eq!(
            snapshot.get(signal),
            if supported(signal) {
                Readout::Busy
            } else {
                Readout::Unsupported
            },
            "{signal:?}"
        );
    }
    drop(guard);
    assert_eq!(
        cache.snapshot_at(now).get(Signal::ReplayDriving),
        Readout::NotPublished
    );
}

#[test]
fn publication_retains_current_missing_and_stale_values_separately() {
    let cache = StatusCache::new();
    let now = Instant::now();
    let mut publication = publication(now, false, 7.0);
    publication.values[0] = Readout::Current(Value::Missing);
    cache.publish(publication);
    let current = cache.snapshot_at(now + MAX_AGE - Duration::from_nanos(1));
    assert_eq!(
        current.get(Signal::ReplayDriving),
        Readout::Current(Value::Missing)
    );
    assert_eq!(
        current.get(Signal::Recenters),
        Readout::Current(Value::Number(7.0))
    );
    assert_eq!(current.get(Signal::VrRendering), Readout::Unsupported);
    assert_eq!(current.get(Signal::ReplayDriving).to_string(), "missing");
    let stale = cache.snapshot_at(now + MAX_AGE);
    assert_eq!(
        stale.get(Signal::ReplayDriving),
        Readout::Stale {
            age: MAX_AGE,
            value: Value::Missing,
        }
    );
    assert_eq!(
        stale.get(Signal::Recenters),
        Readout::Stale {
            age: MAX_AGE,
            value: Value::Number(7.0),
        }
    );
    assert_eq!(stale.get(Signal::VrRendering), Readout::Unsupported);
    assert!(
        stale
            .get(Signal::ReplayDriving)
            .to_string()
            .contains("stale")
    );
    assert!(
        stale
            .get(Signal::ReplayDriving)
            .to_string()
            .contains("missing")
    );
    assert_eq!(
        cache
            .snapshot_at(now - Duration::from_millis(1))
            .get(Signal::Recenters),
        Readout::Current(Value::Number(7.0))
    );
}

#[test]
fn contended_publisher_keeps_the_complete_previous_publication() {
    let cache = StatusCache::new();
    let now = Instant::now();
    cache.publish(publication(now, false, 2.0));
    let guard = cache.publication.lock();
    cache.publish(publication(now + MAX_AGE, true, 3.0));
    drop(guard);
    let snapshot = cache.snapshot_at(now);
    assert_eq!(
        snapshot.get(Signal::ReplayDriving),
        Readout::Current(Value::Bool(false))
    );
    assert_eq!(
        snapshot.get(Signal::Recenters),
        Readout::Current(Value::Number(2.0))
    );
    cache.publish(publication(now + MAX_AGE, true, 3.0));
    let snapshot = cache.snapshot_at(now + MAX_AGE);
    assert_eq!(
        snapshot.get(Signal::ReplayDriving),
        Readout::Current(Value::Bool(true))
    );
    assert_eq!(
        snapshot.get(Signal::Recenters),
        Readout::Current(Value::Number(3.0))
    );
}

#[test]
fn production_capture_uses_the_atomic_sources_and_keeps_unreviewed_rows_unsupported() {
    let cache = StatusCache::new();
    let driving = replay::is_driving();
    let recenters = crate::vr::recenter_count();
    cache.publish(Publication::capture());
    let snapshot = cache.snapshot_at(Instant::now());
    assert_eq!(
        snapshot.get(Signal::ReplayDriving),
        Readout::Current(Value::Bool(driving))
    );
    assert_eq!(
        snapshot.get(Signal::Recenters),
        Readout::Current(Value::Number(f64::from(recenters)))
    );
    assert_eq!(replay::is_driving(), driving);
    assert_eq!(crate::vr::recenter_count(), recenters);
    assert_eq!(
        Signal::ALL
            .iter()
            .filter(|&&signal| supported(signal))
            .count(),
        8
    );
    for &signal in Signal::ALL {
        if supported(signal) {
            assert_ne!(snapshot.get(signal), Readout::Unsupported, "{signal:?}");
        } else {
            assert_eq!(snapshot.get(signal), Readout::Unsupported, "{signal:?}");
        }
    }
}

#[test]
fn root_projection_retains_measurement_values_and_distinguishes_busy_from_missing() {
    let now = Instant::now();
    let frame = root_frame(now);
    assert_eq!(
        frame_values(Some(frame), now, Duration::ZERO),
        [
            Readout::Current(Value::Number(10.0)),
            Readout::Current(Value::Number(8.0)),
            Readout::Current(Value::Number(0.25)),
            Readout::Current(Value::Bool(true)),
            Readout::Current(Value::Number(7.0)),
        ]
    );
    assert_eq!(
        frame_values(None, now, Duration::ZERO),
        std::array::from_fn(|_| Readout::Busy)
    );
    let missing = frame_values(
        Some(FrameReadout { at: None, ..frame }),
        now,
        Duration::ZERO,
    );
    assert_eq!(missing[0], Readout::Current(Value::Missing));
    assert_eq!(missing[1], Readout::Current(Value::Missing));
    assert_eq!(missing[2], Readout::Current(Value::Missing));
    assert_eq!(missing[3], Readout::Current(Value::Bool(false)));
    assert_eq!(missing[4], Readout::Current(Value::Number(7.0)));
}

#[test]
fn root_simulation_projection_reads_the_projected_count_and_distinguishes_busy() {
    assert_eq!(sim_value(None), Readout::Busy);
    assert_eq!(
        sim_value(Some(SimStatus {
            ledger: LedgerCounts {
                exact: 11,
                projected: 13,
            },
            ..SimStatus::default()
        })),
        Readout::Current(Value::Number(13.0))
    );
}

#[test]
fn stale_source_age_accumulates_without_discarding_the_retained_value() {
    let cache = StatusCache::new();
    let now = Instant::now();
    let mut publication = publication(now, false, 0.0);
    publication.frame = Some(root_frame(now - Duration::from_secs(2)));
    cache.publish(publication);
    let snapshot = cache.snapshot_at(now + Duration::from_millis(250));
    assert_eq!(
        snapshot.get(Signal::RigGapM),
        Readout::Stale {
            age: Duration::from_millis(2250),
            value: Value::Number(10.0),
        }
    );
    assert_eq!(
        snapshot.get(Signal::RigObstructed),
        Readout::Stale {
            age: Duration::from_millis(2250),
            value: Value::Bool(true),
        }
    );
    assert_eq!(
        snapshot.get(Signal::RigPushbacks),
        Readout::Current(Value::Number(7.0))
    );
    assert_eq!(snapshot.get(Signal::RigLedgerProjected), Readout::Busy);
    assert_eq!(snapshot.get(Signal::VrRendering), Readout::Unsupported);
}

#[test]
fn missing_root_measurements_keep_publication_age_and_do_not_become_busy_or_unsupported() {
    let cache = StatusCache::new();
    let now = Instant::now();
    let mut publication = publication(now, false, 0.0);
    publication.frame = Some(FrameReadout {
        at: None,
        ..root_frame(now)
    });
    cache.publish(publication);
    for elapsed in [Duration::ZERO, MAX_AGE] {
        let snapshot = cache.snapshot_at(now + elapsed);
        for signal in [Signal::RigGapM, Signal::RigResidualM, Signal::RigPushbackM] {
            assert_eq!(
                snapshot.get(signal),
                if elapsed < MAX_AGE {
                    Readout::Current(Value::Missing)
                } else {
                    Readout::Stale {
                        age: elapsed,
                        value: Value::Missing,
                    }
                }
            );
        }
        assert_eq!(snapshot.get(Signal::RigLedgerProjected), Readout::Busy);
        assert_eq!(snapshot.get(Signal::VrRendering), Readout::Unsupported);
    }
}

#[test]
fn fresh_root_measurements_expire_at_the_source_boundary_before_the_publication() {
    let cache = StatusCache::new();
    let now = Instant::now();
    let mut publication = publication(now, false, 3.0);
    publication.frame = Some(root_frame(now - Duration::from_millis(490)));
    publication.values[2] = Readout::Current(Value::Number(13.0));
    cache.publish(publication);
    let measurements = [
        (Signal::RigGapM, Value::Number(10.0)),
        (Signal::RigResidualM, Value::Number(8.0)),
        (Signal::RigPushbackM, Value::Number(0.25)),
        (Signal::RigObstructed, Value::Bool(true)),
    ];
    for elapsed in [
        Duration::ZERO,
        Duration::from_millis(10) - Duration::from_nanos(1),
    ] {
        let snapshot = cache.snapshot_at(now + elapsed);
        for (signal, value) in &measurements {
            assert_eq!(snapshot.get(*signal), Readout::Current(value.clone()));
        }
    }
    for elapsed in [Duration::from_millis(10), Duration::from_millis(20)] {
        let snapshot = cache.snapshot_at(now + elapsed);
        for (signal, value) in &measurements {
            assert_eq!(
                snapshot.get(*signal),
                Readout::Stale {
                    age: Duration::from_millis(490) + elapsed,
                    value: value.clone(),
                }
            );
        }
        for (signal, value) in [
            (Signal::Recenters, Value::Number(3.0)),
            (Signal::RigPushbacks, Value::Number(7.0)),
            (Signal::RigLedgerProjected, Value::Number(13.0)),
        ] {
            assert_eq!(snapshot.get(signal), Readout::Current(value));
        }
        assert_eq!(snapshot.get(Signal::VrRendering), Readout::Unsupported);
    }
}

#[test]
fn fresh_at_capture_root_measurements_keep_their_full_age_after_publication_expiry() {
    let cache = StatusCache::new();
    let now = Instant::now();
    let mut publication = publication(now, false, 3.0);
    publication.frame = Some(root_frame(now - Duration::from_millis(490)));
    publication.values[2] = Readout::Current(Value::Number(13.0));
    cache.publish(publication);
    let snapshot = cache.snapshot_at(now + MAX_AGE);
    for (signal, value) in [
        (Signal::RigGapM, Value::Number(10.0)),
        (Signal::RigResidualM, Value::Number(8.0)),
        (Signal::RigPushbackM, Value::Number(0.25)),
        (Signal::RigObstructed, Value::Bool(true)),
    ] {
        assert_eq!(
            snapshot.get(signal),
            Readout::Stale {
                age: Duration::from_millis(1490),
                value,
            }
        );
    }
    for (signal, value) in [
        (Signal::Recenters, Value::Number(3.0)),
        (Signal::RigPushbacks, Value::Number(7.0)),
        (Signal::RigLedgerProjected, Value::Number(13.0)),
    ] {
        assert_eq!(
            snapshot.get(signal),
            Readout::Stale {
                age: MAX_AGE,
                value,
            }
        );
    }
    assert_eq!(snapshot.get(Signal::VrRendering), Readout::Unsupported);
}

fn supported(signal: Signal) -> bool {
    matches!(
        signal,
        Signal::ReplayDriving
            | Signal::Recenters
            | Signal::RigGapM
            | Signal::RigResidualM
            | Signal::RigPushbackM
            | Signal::RigObstructed
            | Signal::RigPushbacks
            | Signal::RigLedgerProjected
    )
}

fn publication(at: Instant, driving: bool, recenters: f64) -> Publication {
    let mut values = std::array::from_fn(|_| Readout::Busy);
    values[0] = Readout::Current(Value::Bool(driving));
    values[1] = Readout::Current(Value::Number(recenters));
    Publication {
        at,
        values,
        frame: None,
    }
}

fn root_frame(at: Instant) -> FrameReadout {
    FrameReadout {
        at: Some(at),
        measurement: Measurement {
            target: Vec2::new(3.0, 4.0),
            lean: Lean {
                vector: Vec2::new(3.0, 0.0),
                ..Lean::default()
            },
            ..Measurement::default()
        },
        world_scale: 2.0,
        pushback: PushbackOutcome {
            obstructed: true,
            ..PushbackOutcome::default()
        },
        debt_m: 0.25,
        pushbacks: 7,
        ..FrameReadout::default()
    }
}
