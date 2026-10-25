//! Coherent input publications with independent bounded capture and gameplay queues.
//!
//! Live groups retain host observation time and actual controller publication identity. Replay
//! uses separate storage and never enters the live clock's history. Each consumer drains once;
//! overflow is an explicit discontinuity, not an inferred button release.

use std::{collections::VecDeque, sync::LazyLock, time::Instant};

use parking_lot::Mutex;
use tracking_format::input::{
    CapturedInput, ClockDomain, DiscontinuityReason, GroupOrder, HISTORY_CAPACITY, InputGroup,
    InputRecord, InputStamp, RouteSample, policy::PolicyEvaluation,
};

use crate::vr::input::{
    flight_pose::{self, EpochChange},
    semantic::SCHEMA_VERSION,
};

/// One producer publication. No display-time comparison or later snapshot read establishes its
/// controller association; only the publishing call supplies that identity.
#[derive(Debug, Clone)]
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
            self.publish(Publication {
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
        slot.publish(Publication {
            observed_at: sample.observed_at,
            frame_sequence: Some(sample.frame_sequence),
            record: InputRecord::Group(group),
        });
    }

    fn publish(&mut self, publication: Publication) {
        self.latest = Some(publication.clone());
        self.gameplay.push(publication.clone());
        self.capture.push(publication);
    }

    fn take_capture(&mut self) -> Vec<Publication> {
        self.capture.pending.drain(..).collect()
    }
}

/// Drain even while idle, so recording start cannot inherit pre-recording observations.
pub(crate) fn take_capture() -> Vec<Publication> {
    SLOT.lock().take_capture()
}

/// Drain the active clock's observations independently of recording. The controller snapshot
/// identity is retained so a later snapshot cannot authorize an older group's edges.
/// Return the latest original group only when it belongs to this exact controller publication.
/// This does not drain or reinterpret the gameplay evidence.
pub(crate) fn original_group(frame_sequence: u64) -> Option<InputGroup> {
    let slot = SLOT.lock();
    if slot.replay_active {
        return None;
    }
    slot.latest
        .as_ref()
        .filter(|publication| publication.frame_sequence == Some(frame_sequence))
        .and_then(|publication| match &publication.record {
            InputRecord::Group(group) => Some(group.clone()),
            _ => None,
        })
}

pub(crate) fn take_gameplay() -> (bool, Vec<Publication>) {
    let mut slot = SLOT.lock();
    let replay = slot.replay_active;
    let publications = if replay {
        slot.gameplay.pending.clear();
        slot.replayed
            .drain(..)
            .map(|captured| Publication {
                observed_at: Instant::now(),
                frame_sequence: None,
                record: captured.record,
            })
            .collect()
    } else {
        slot.gameplay.pending.drain(..).collect()
    };
    (replay, publications)
}

/// Capture a final decision without changing live ownership. A fresh recording has an explicit
/// unavailable prefix until a recorded real-neutral boundary, rather than inheriting a live episode.
pub(crate) fn record_policy(evaluation: PolicyEvaluation) {
    #[cfg(test)]
    let _serial = TEST_LOCK.lock();
    SLOT.lock().record_policy(evaluation);
}

pub(crate) fn begin_capture() {
    let mut slot = SLOT.lock();
    slot.capture.pending.clear();
    slot.capture_boundary = false;
}

impl Published {
    fn record_policy(&mut self, mut evaluation: PolicyEvaluation) {
        if !self.capture_boundary {
            if evaluation.neutral {
                evaluation.outcome = tracking_format::input::policy::PolicyOutcome::NeutralBoundary;
                self.capture_boundary = true;
            } else {
                evaluation.outcome =
                    tracking_format::input::policy::PolicyOutcome::UnavailablePrefix;
            }
            evaluation.singles = [tracking_format::input::policy::AcceptedSingle::None; 3];
            evaluation.single_down = [false; 3];
            evaluation.withheld = [true; 3];
            evaluation.command = tracking_format::input::policy::AcceptedCommand::None;
            evaluation.raw = [0; 3];
            evaluation.direct = Default::default();
            evaluation.claims = 0;
            evaluation.commands = 0;
        }
        self.capture.push(Publication {
            observed_at: Instant::now(),
            frame_sequence: None,
            record: InputRecord::Policy(evaluation),
        });
    }
}

pub(crate) fn interrupt(reason: DiscontinuityReason) {
    SLOT.lock().interrupt(reason, Instant::now());
}

/// Repeated playback starts allocate fresh storage, not a replacement live clock epoch.
pub(crate) fn begin_replay(available: bool) {
    flight_pose::source_changed(EpochChange::ReplayStarted);
    SLOT.lock().begin_replay(available);
}

/// Preserve the recorded envelope and logical stamp in bounded replay storage, never in the live
/// publication queue. The gameplay drain selects only one clock at a time.
pub(crate) fn publish_replayed(records: &[CapturedInput]) {
    SLOT.lock().publish_replayed(records);
}

pub(crate) fn end_replay() {
    flight_pose::source_changed(EpochChange::ReplayEnded);
    SLOT.lock().end_replay(Instant::now());
}

struct Published {
    sequence: u64,
    generation: Option<u64>,
    capture: CaptureQueue,
    gameplay: CaptureQueue,
    replayed: VecDeque<CapturedInput>,
    replay_active: bool,
    capture_boundary: bool,
    latest: Option<Publication>,
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
        if matches!(reason, DiscontinuityReason::ClockChanged) {
            flight_pose::source_changed(EpochChange::SourceClockChanged);
        }
        self.publish(Publication {
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
        InputRecord::Group(_) | InputRecord::Policy(_) => 1,
        InputRecord::Discontinuity { discarded, .. } => *discarded,
    }
}

static ORIGIN: LazyLock<Instant> = LazyLock::new(Instant::now);
static SLOT: LazyLock<Mutex<Published>> = LazyLock::new(|| {
    Mutex::new(Published {
        sequence: 0,
        generation: None,
        capture: CaptureQueue::default(),
        gameplay: CaptureQueue::default(),
        replayed: VecDeque::new(),
        replay_active: false,
        capture_boundary: false,
        latest: None,
    })
});

#[cfg(test)]
pub(crate) fn replayed_for_test() -> (bool, Vec<CapturedInput>) {
    let slot = SLOT.lock();
    (slot.replay_active, slot.replayed.iter().cloned().collect())
}

#[cfg(test)]
pub(crate) static TEST_LOCK: parking_lot::ReentrantMutex<()> = parking_lot::ReentrantMutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> Published {
        Published {
            sequence: 44,
            generation: Some(9),
            capture: CaptureQueue::default(),
            gameplay: CaptureQueue::default(),
            replayed: VecDeque::new(),
            replay_active: false,
            capture_boundary: false,
            latest: None,
        }
    }

    #[test]
    fn failed_raw_or_semantic_read_never_publishes_an_eligible_partial_group() {
        let _guard = flight_pose::test_guard();
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
        let _guard = flight_pose::test_guard();
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
    fn capture_and_gameplay_have_independent_single_consumer_drains() {
        let _guard = flight_pose::test_guard();
        let mut slot = local();
        slot.interrupt(DiscontinuityReason::Unavailable, Instant::now());
        assert_eq!(slot.take_capture().len(), 1);
        assert!(slot.take_capture().is_empty());
        assert_eq!(slot.gameplay.pending.drain(..).count(), 1);
        assert!(slot.gameplay.pending.is_empty());
        slot.interrupt(DiscontinuityReason::ClockChanged, Instant::now());
        assert_eq!(slot.gameplay.pending.drain(..).count(), 1);
        assert_eq!(slot.take_capture().len(), 1);
    }

    #[test]
    fn gameplay_overflow_is_independent_of_capture_draining() {
        let _guard = flight_pose::test_guard();
        let mut slot = local();
        for _ in 0..=HISTORY_CAPACITY {
            slot.interrupt(DiscontinuityReason::Unavailable, Instant::now());
            slot.take_capture();
        }
        assert_eq!(slot.gameplay.pending.len(), 2);
        assert!(matches!(
            slot.gameplay.pending[0].record,
            InputRecord::Discontinuity {
                reason: DiscontinuityReason::Overflow,
                ..
            }
        ));
    }

    #[test]
    fn policy_only_and_mixed_capture_overflow_count_actual_records() {
        for mixed in [false, true] {
            let mut queue = CaptureQueue::default();
            let policy = PolicyEvaluation {
                version: tracking_format::input::policy::POLICY_A_VERSION,
                stamp: InputStamp {
                    epoch: 1,
                    clock: ClockDomain::Synthetic,
                    time_ns: 1,
                    sync: 1,
                    sequence: 1,
                },
                action_schema: SCHEMA_VERSION,
                timing: None,
                context: 0,
                gameplay: false,
                outcome: tracking_format::input::policy::PolicyOutcome::Cancelled,
                neutral: false,
                singles: [tracking_format::input::policy::AcceptedSingle::None; 3],
                single_down: [false; 3],
                withheld: [true; 3],
                command: tracking_format::input::policy::AcceptedCommand::None,
                raw: [0; 3],
                direct: Default::default(),
                claims: 0,
                commands: 0,
            };
            for index in 0..=HISTORY_CAPACITY {
                let record = if mixed && index % 2 == 0 {
                    InputRecord::Group(InputGroup {
                        stamp: policy.stamp,
                        order: GroupOrder::UnorderedSync,
                        source_generation: 1,
                        binding_generation: 1,
                        action_schema: SCHEMA_VERSION,
                        context: 0,
                        focused: true,
                        samples: Vec::new(),
                    })
                } else {
                    InputRecord::Policy(policy)
                };
                queue.push(Publication {
                    observed_at: Instant::now(),
                    frame_sequence: None,
                    record,
                });
            }
            assert!(
                matches!(queue.pending[0].record, InputRecord::Discontinuity { discarded, .. } if discarded == HISTORY_CAPACITY as u64)
            );
        }
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
