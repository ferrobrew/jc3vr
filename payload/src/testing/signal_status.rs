//! Bounded, UI-only signal snapshots. Scenario expectations keep using `signals::Signal::read`.
//!
//! The game thread publishes only reviewed atomics and root try-read copies after the scenario
//! update returns. Publications retain the root source timestamp so snapshot reads apply the
//! root's freshness rule without losing the source age. Rendering copies the leaf cache once
//! and never samples a signal source.

use std::{
    fmt,
    time::{Duration, Instant},
};

use parking_lot::Mutex;

use crate::{
    controllers::body::rig::root::{self, FrameReadout, SimStatus},
    testing::{
        replay,
        signals::{Signal, Value},
    },
};

/// The availability of one signal in the UI snapshot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Readout {
    /// No reviewed source is registered for this signal's UI row.
    Unsupported,
    NotPublished,
    Busy,
    Stale {
        age: Duration,
        value: Value,
    },
    Current(Value),
}

impl fmt::Display for Readout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported: no reviewed UI snapshot source"),
            Self::NotPublished => f.write_str("not published"),
            Self::Busy => f.write_str("busy"),
            Self::Stale { age, value } => {
                write!(f, "stale ({:.1} s old): {value}", age.as_secs_f64())
            }
            Self::Current(value) => value.fmt(f),
        }
    }
}

/// One owned, fixed-size copy for the whole Signals table.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Snapshot {
    values: [Readout; 8],
}

impl Snapshot {
    pub(crate) fn get(&self, signal: Signal) -> Readout {
        let index = match signal {
            Signal::ReplayDriving => 0,
            Signal::Recenters => 1,
            Signal::RigGapM => 2,
            Signal::RigResidualM => 3,
            Signal::RigPushbackM => 4,
            Signal::RigObstructed => 5,
            Signal::RigPushbacks => 6,
            Signal::RigLedgerProjected => 7,
            _ => return Readout::Unsupported,
        };
        self.values[index].clone()
    }
}

/// Copy the UI publication without waiting for the publisher or calling any signal readers.
pub(crate) fn snapshot() -> Snapshot {
    CACHE.snapshot_at(Instant::now())
}

/// Publish the complete reviewed subset after `scenario::update` releases its state lock.
pub(crate) fn publish() {
    CACHE.publish(Publication::capture());
}

const MAX_AGE: Duration = Duration::from_secs(1);
static CACHE: StatusCache = StatusCache::new();

struct StatusCache {
    publication: Mutex<Option<Publication>>,
}

impl StatusCache {
    const fn new() -> Self {
        Self {
            publication: Mutex::new(None),
        }
    }

    fn snapshot_at(&self, now: Instant) -> Snapshot {
        let publication = {
            let Some(slot) = self.publication.try_lock() else {
                return Snapshot {
                    values: std::array::from_fn(|_| Readout::Busy),
                };
            };
            slot.clone()
        };
        match publication {
            None => Snapshot {
                values: std::array::from_fn(|_| Readout::NotPublished),
            },
            Some(publication) => publication.snapshot_at(now),
        }
    }

    fn publish(&self, publication: Publication) {
        if let Some(mut slot) = self.publication.try_lock() {
            *slot = Some(publication);
        }
    }
}

#[derive(Clone)]
struct Publication {
    at: Instant,
    values: [Readout; 3],
    frame: Option<FrameReadout>,
}

impl Publication {
    fn capture() -> Self {
        // Each root owner is copied once, without waiting or updating its stored status.
        let frame = root::try_frame_readout();
        let sim = root::try_sim_readout();
        Self {
            at: Instant::now(),
            values: [
                Readout::Current(Value::Bool(replay::is_driving())),
                Readout::Current(Value::Number(f64::from(crate::vr::recenter_count()))),
                sim_value(sim),
            ],
            frame,
        }
    }

    fn snapshot_at(self, now: Instant) -> Snapshot {
        let age = now.saturating_duration_since(self.at);
        let [driving, recenters, projected] = self.values;
        let [gap, residual, debt, obstructed, pushbacks] = frame_values(self.frame, now, age);
        Snapshot {
            values: [
                driving, recenters, gap, residual, debt, obstructed, pushbacks, projected,
            ]
            .map(|value| match value {
                Readout::Current(value) if age >= MAX_AGE => Readout::Stale { age, value },
                value => value,
            }),
        }
    }
}

fn frame_values(
    frame: Option<FrameReadout>,
    now: Instant,
    publication_age: Duration,
) -> [Readout; 5] {
    let Some(frame) = frame else {
        return std::array::from_fn(|_| Readout::Busy);
    };
    let measurement = |value| match frame.at {
        None => Readout::Current(Value::Missing),
        Some(_) if frame.fresh(now) && publication_age < MAX_AGE => Readout::Current(value),
        Some(at) => Readout::Stale {
            age: now.saturating_duration_since(at),
            value,
        },
    };
    [
        measurement(Value::Number(f64::from(frame.gap_m()))),
        measurement(Value::Number(f64::from(frame.residual_m()))),
        measurement(Value::Number(f64::from(frame.debt_m))),
        if frame.at.is_none() {
            Readout::Current(Value::Bool(false))
        } else {
            measurement(Value::Bool(frame.pushback.obstructed))
        },
        Readout::Current(Value::Number(frame.pushbacks as f64)),
    ]
}

fn sim_value(sim: Option<SimStatus>) -> Readout {
    sim.map_or(Readout::Busy, |sim| {
        Readout::Current(Value::Number(sim.ledger.projected as f64))
    })
}

#[cfg(test)]
#[path = "signal_status_tests.rs"]
mod tests;
