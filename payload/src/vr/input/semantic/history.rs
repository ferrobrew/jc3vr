//! Coherent input publications for capture and a separate recorded-input replay lifecycle.
//!
//! Live groups retain host observation time and actual controller publication identity. Replay
//! never enters the live clock's history. Gameplay continues to use the compatibility snapshots
//! until the ordered input consumer is integrated.

use std::{collections::VecDeque, sync::LazyLock, time::Instant};

use parking_lot::Mutex;
use tracking_format::input::{
    CapturedInput, ClockDomain, DiscontinuityReason, GroupOrder, HISTORY_CAPACITY, InputGroup,
    InputRecord, InputStamp, RouteSample,
};

use crate::vr::input::semantic::SCHEMA_VERSION;

/// One producer publication. No display-time comparison or later snapshot read establishes its
/// controller association; only the publishing call supplies that identity.
#[derive(Debug)]
pub(crate) struct Publication {
    pub(crate) observed_at: Instant,
    pub(crate) frame_sequence: Option<u64>,
    pub(crate) record: InputRecord,
}

pub(crate) struct LiveSample {
    pub(crate) samples: Vec<RouteSample>,
    pub(crate) context: u64,
    pub(crate) focused: bool,
    pub(crate) generation: u64,
    pub(crate) observed_at: Instant,
    pub(crate) frame_sequence: u64,
}

/// Capture this immediately after a successful sync, not after the action-reading loops.
pub(crate) fn observation_time() -> Instant {
    let _ = *ORIGIN;
    Instant::now()
}

/// Failed reads discard the entire candidate group while retaining the actual snapshot identity.
pub(crate) fn publish_observation(sample: LiveSample, reads_succeeded: bool) {
    SLOT.lock().publish_observation(sample, reads_succeeded);
}

impl Published {
    fn publish_observation(&mut self, sample: LiveSample, reads_succeeded: bool) {
        if !sample.focused || !reads_succeeded {
            self.capture.push(Publication {
                observed_at: sample.observed_at,
                frame_sequence: Some(sample.frame_sequence),
                record: InputRecord::Discontinuity {
                    reason: DiscontinuityReason::Unavailable,
                    discarded: 0,
                },
            });
            return;
        }
        self.publish_live(sample);
    }

    fn publish_live(&mut self, sample: LiveSample) {
        let slot = self;
        let Some(sequence) = slot.sequence.checked_add(1) else {
            slot.interrupt(DiscontinuityReason::ClockChanged, sample.observed_at);
            return;
        };
        slot.sequence = sequence;
        let Ok(time_ns) = u64::try_from(
            sample
                .observed_at
                .saturating_duration_since(*ORIGIN)
                .as_nanos(),
        ) else {
            slot.interrupt(DiscontinuityReason::ClockChanged, sample.observed_at);
            return;
        };
        let group = InputGroup {
            stamp: InputStamp {
                epoch: 1,
                clock: ClockDomain::HostMonotonic,
                time_ns,
                sync: sequence,
                sequence,
            },
            order: GroupOrder::UnorderedSync,
            source_generation: sample.generation,
            binding_generation: sample.generation,
            action_schema: SCHEMA_VERSION,
            context: sample.context,
            focused: sample.focused,
            samples: sample.samples,
        };
        if let Some(previous) = slot.generation
            && previous != sample.generation
        {
            slot.interrupt(DiscontinuityReason::GenerationChanged, sample.observed_at);
        }
        slot.generation = Some(sample.generation);
        slot.capture.push(Publication {
            observed_at: sample.observed_at,
            frame_sequence: Some(sample.frame_sequence),
            record: InputRecord::Group(group),
        });
    }

    fn take_capture(&mut self) -> Vec<Publication> {
        self.capture.pending.drain(..).collect()
    }
}

/// Drain even while idle, so recording start cannot inherit pre-recording observations.
pub(crate) fn take_capture() -> Vec<Publication> {
    SLOT.lock().take_capture()
}

pub(crate) fn interrupt(reason: DiscontinuityReason) {
    SLOT.lock().interrupt(reason, Instant::now());
}

/// Repeated playback starts allocate fresh storage, not a replacement live clock epoch.
pub(crate) fn begin_replay(available: bool) {
    SLOT.lock().begin_replay(available);
}

/// Preserve the recorded envelope and logical stamp in bounded replay storage, never in the live
/// publication queue. No gameplay consumer is installed by this adapter.
pub(crate) fn publish_replayed(records: &[CapturedInput]) {
    SLOT.lock().publish_replayed(records);
}

pub(crate) fn end_replay() {
    SLOT.lock().end_replay(Instant::now());
}

struct Published {
    sequence: u64,
    generation: Option<u64>,
    capture: CaptureQueue,
    replayed: VecDeque<CapturedInput>,
    replay_active: bool,
}

impl Published {
    fn begin_replay(&mut self, available: bool) {
        self.replayed.clear();
        self.replay_active = true;
        if !available {
            self.replayed.push_back(CapturedInput {
                capture_time_ns: 0,
                frame_sequence: None,
                record: InputRecord::Discontinuity {
                    reason: DiscontinuityReason::LegacyOrderAbsent,
                    discarded: 0,
                },
            });
        }
    }

    fn publish_replayed(&mut self, records: &[CapturedInput]) {
        if !self.replay_active {
            return;
        }
        for record in records {
            if self.replayed.len() >= HISTORY_CAPACITY {
                let discarded = self.replayed.iter().fold(0u64, |count, captured| {
                    count.saturating_add(record_loss(&captured.record))
                });
                self.replayed.clear();
                self.replayed.push_back(CapturedInput {
                    capture_time_ns: record.capture_time_ns,
                    frame_sequence: None,
                    record: InputRecord::Discontinuity {
                        reason: DiscontinuityReason::Overflow,
                        discarded,
                    },
                });
            }
            self.replayed.push_back(record.clone());
        }
    }

    fn end_replay(&mut self, now: Instant) {
        self.replayed.clear();
        self.replay_active = false;
        self.interrupt(DiscontinuityReason::ClockChanged, now);
    }

    fn interrupt(&mut self, reason: DiscontinuityReason, at: Instant) {
        self.capture.push(Publication {
            observed_at: at,
            frame_sequence: None,
            record: InputRecord::Discontinuity {
                reason,
                discarded: 0,
            },
        });
    }
}

#[derive(Default)]
struct CaptureQueue {
    pending: VecDeque<Publication>,
}

impl CaptureQueue {
    fn push(&mut self, publication: Publication) {
        if self.pending.len() >= HISTORY_CAPACITY {
            let discarded = self.pending.iter().fold(0u64, |count, entry| {
                count.saturating_add(record_loss(&entry.record))
            });
            self.pending.clear();
            self.pending.push_back(Publication {
                observed_at: publication.observed_at,
                frame_sequence: None,
                record: InputRecord::Discontinuity {
                    reason: DiscontinuityReason::Overflow,
                    discarded,
                },
            });
        }
        self.pending.push_back(publication);
    }
}

fn record_loss(record: &InputRecord) -> u64 {
    match record {
        InputRecord::Group(_) => 1,
        InputRecord::Discontinuity { discarded, .. } => *discarded,
    }
}

static ORIGIN: LazyLock<Instant> = LazyLock::new(Instant::now);
static SLOT: LazyLock<Mutex<Published>> = LazyLock::new(|| {
    Mutex::new(Published {
        sequence: 0,
        generation: None,
        capture: CaptureQueue::default(),
        replayed: VecDeque::new(),
        replay_active: false,
    })
});

#[cfg(test)]
pub(crate) fn replayed_for_test() -> (bool, Vec<CapturedInput>) {
    let slot = SLOT.lock();
    (slot.replay_active, slot.replayed.iter().cloned().collect())
}

#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> Published {
        Published {
            sequence: 44,
            generation: Some(9),
            capture: CaptureQueue::default(),
            replayed: VecDeque::new(),
            replay_active: false,
        }
    }

    #[test]
    fn failed_raw_or_semantic_read_never_publishes_an_eligible_partial_group() {
        for (route, focused, succeeded) in [
            (tracking_format::input::InputRoute::Raw(0), true, false),
            (
                tracking_format::input::InputRoute::Semantic {
                    set: 0,
                    action: 0,
                    hand: 0,
                },
                true,
                false,
            ),
            (tracking_format::input::InputRoute::Raw(0), false, true),
        ] {
            let mut slot = local();
            let now = Instant::now();
            slot.publish_observation(
                LiveSample {
                    samples: vec![RouteSample {
                        route,
                        value: tracking_format::input::ValueBits::Bool(true),
                        active: true,
                        down: true,
                        change: None,
                        sources: tracking_format::input::SourceQuery::NotEnumerated,
                    }],
                    context: 0,
                    focused,
                    generation: 9,
                    observed_at: now,
                    frame_sequence: 77,
                },
                succeeded,
            );
            let captured = slot.take_capture();
            assert_eq!(captured.len(), 1);
            assert_eq!(captured[0].observed_at, now);
            assert_eq!(captured[0].frame_sequence, Some(77));
            assert_eq!(
                captured[0].record,
                InputRecord::Discontinuity {
                    reason: DiscontinuityReason::Unavailable,
                    discarded: 0
                }
            );
            assert_eq!(slot.sequence, 44);
        }
    }

    #[test]
    fn replay_lifecycle_never_replaces_or_splices_live_clock_history() {
        let mut slot = local();
        let now = Instant::now();
        slot.interrupt(DiscontinuityReason::Unavailable, now);
        slot.begin_replay(false);
        assert_eq!(
            slot.replayed[0].record,
            InputRecord::Discontinuity {
                reason: DiscontinuityReason::LegacyOrderAbsent,
                discarded: 0
            }
        );
        slot.begin_replay(true);
        assert!(slot.replayed.is_empty());
        let captured = CapturedInput {
            capture_time_ns: 17,
            frame_sequence: Some(99),
            record: InputRecord::Discontinuity {
                reason: DiscontinuityReason::Unavailable,
                discarded: 3,
            },
        };
        slot.publish_replayed(std::slice::from_ref(&captured));
        assert_eq!(slot.replayed[0], captured);
        assert_eq!(slot.sequence, 44);
        assert_eq!(slot.generation, Some(9));
        assert_eq!(slot.capture.pending.len(), 1);
        slot.end_replay(now);
        assert!(slot.replayed.is_empty());
        slot.publish_replayed(&[captured]);
        assert!(slot.replayed.is_empty());
        assert_eq!(slot.capture.pending.len(), 2);
        assert_eq!(slot.sequence, 44);
    }

    #[test]
    fn idle_capture_drain_is_single_consumer_and_overflow_is_explicit() {
        let mut queue = CaptureQueue::default();
        let now = Instant::now();
        for _ in 0..=HISTORY_CAPACITY {
            queue.push(Publication {
                observed_at: now,
                frame_sequence: None,
                record: InputRecord::Discontinuity {
                    reason: DiscontinuityReason::Unavailable,
                    discarded: 1,
                },
            });
        }
        let drained = queue.pending.drain(..).collect::<Vec<_>>();
        assert_eq!(drained.len(), 2);
        assert_eq!(
            drained[0].record,
            InputRecord::Discontinuity {
                reason: DiscontinuityReason::Overflow,
                discarded: HISTORY_CAPACITY as u64
            }
        );
        assert!(queue.pending.drain(..).next().is_none());
    }
}
