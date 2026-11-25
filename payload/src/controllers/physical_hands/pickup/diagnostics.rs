//! Bounded evidence from the grip resolver, pickup request queue, and pickup consumer.
//!
//! Records contain copied observations only. Missing provider identity is distinct from a provider
//! reporting no target. The leaf slot never logs and never calls an engine reader.
//!
//! While a tracking recording or the lookback collects, each acquisition stage is also noted as a
//! [`GripAcquisition`] event: a grip press or a queued request with the resolver's candidates and
//! the consumer's branch, the simulation consumer's result for a request, and the first drive of
//! what a request took. The event is built after the slot's lock is released.

pub(crate) mod scan;
#[cfg(test)]
mod tests;

use parking_lot::Mutex;
use tracking_format::event::{
    GripAcquisition, GripCandidate, GripDrive, GripScan, GripScanEntry, GripStage, Label,
};

use crate::{
    controllers::{
        bindings::controls::ButtonControl,
        frame::Hand,
        grab::{
            GrabVolume,
            latch::GripLatch,
            resolve::{self, Lost},
            tick::Tick,
        },
        physical_hands::{
            bodies::{BodyError, BodyState},
            grasp::Reach,
            pickup::HeldReport,
            rigid::Velocities,
        },
    },
    recorder::tracking::{
        events::{self, EventKind},
        format::{debug_label, hand_to_file},
    },
};

/// The number of resolver candidates retained in each input snapshot.
pub(crate) const CANDIDATE_CAPACITY: usize = 8;
/// The number of queued requests retained for each hand.
pub(crate) const REQUEST_CAPACITY: usize = 4;

/// The input caller's observations, before the pickup consumer runs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct InputState {
    pub grips: [ButtonControl; 2],
    pub latches: [GripLatch; 2],
    pub grabs: [Option<GrabVolume>; 2],
}

/// The exact controller publication used by an input tick, with its bounded resolver evidence.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct InputContext {
    pub hands: [InputRecord; 2],
}

/// A reached target retained from the real resolver output.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Candidate {
    pub resolved: resolve::Reach,
    /// The provider's copied reach, when that provider supplies identity.
    pub observed: Option<Reach>,
    pub lost: Option<Lost>,
}

/// One hand's input observation. A frame sequence is a publication identity, not a clock.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct InputRecord {
    /// The scan copied by the provider while it reads the corresponding physical-hand state.
    pub scan: Option<scan::Scan>,
    pub sequence: u64,
    /// The per-hand input observation, including repeated reads of one controller publication.
    pub observation: u64,
    /// The hand's grip episode, incremented on a press or a newly observed held grip.
    pub episode: u64,
    pub grip: ButtonControl,
    pub latch: GripLatch,
    pub grab: Option<GrabVolume>,
    pub chosen: Option<Candidate>,
    pub candidates: [Option<Candidate>; CANDIDATE_CAPACITY],
    pub candidates_omitted: usize,
    pub consumer: InputConsumer,
}

/// What the pickup input consumer actually did with an input observation.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum InputConsumer {
    NotRun,
    GripOpen,
    NotPickup,
    StaleReach,
    NoReach,
    WeaponPath,
    Queued,
}

/// An identity carried by the actual queued request to its simulation consumer.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RequestKey {
    pub sequence: u64,
    pub episode: u64,
    pub request: u64,
    /// The reach read by the pickup finisher, not an inferred provider identity.
    pub target: Reach,
}

/// The simulation consumer's observed result, or the queue operation that removed a request.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ConsumerResult {
    Queued,
    Replaced,
    Cleared,
    NoPalm,
    LatchAccepted,
    LatchRefused,
    CharacterAccepted,
    CharacterRefused,
    BodyAccepted { joined: bool },
    FixedOrTooHeavy,
    BodyRefused(BodyError),
}

/// The last observed carried-body write or joined-body report for an accepted request.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct DriveRecord {
    pub object: usize,
    pub body: u32,
    pub before: BodyState,
    /// None for a joint-driven hold. This is the requested value, not a post-step observation.
    pub commanded: Option<Velocities>,
    pub result: Option<Result<(), BodyError>>,
    pub report: Option<HeldReport>,
}

/// One queued request and its observed continuation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RequestRecord {
    pub key: RequestKey,
    pub input: InputRecord,
    pub consumer: ConsumerResult,
    pub drive: Option<DriveRecord>,
}

/// A fixed-size readout for a hand. Array order is storage order; request keys identify entries.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct HandEvidence {
    pub latest: Option<InputRecord>,
    pub last_press: Option<InputRecord>,
    pub requests: [Option<RequestRecord>; REQUEST_CAPACITY],
    pub evicted: u64,
    observation: u64,
    episode: u64,
    held: bool,
    next_request: u64,
    next_slot: usize,
}

/// Both hands' copied evidence. Reading this slot acquires only its leaf lock.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Status {
    pub hands: [HandEvidence; 2],
}

/// Capture the exact resolver result and the input caller's captured publication sequence.
pub(crate) fn input(
    sequence: u64,
    tick: &Tick,
    state: InputState,
    scans: [Option<scan::Scan>; 2],
) -> InputContext {
    let mut status = STATUS.lock();
    InputContext {
        hands: Hand::BOTH.map(|hand| {
            let mut record = status.hands[hand.index()].input(sequence, tick, state, hand);
            record.scan = scans[hand.index()];
            status.hands[hand.index()].latest = Some(record);
            if record.grip.pressed {
                status.hands[hand.index()].last_press = Some(record);
            }
            record
        }),
    }
}

/// Copy a bounded snapshot for a guard-free consumer to format after this call returns.
pub(crate) fn status() -> Status {
    *STATUS.lock()
}

/// Record a branch in the actual pickup input consumer. A press's branch is noted as an event.
pub(crate) fn input_consumer(hand: Hand, input: InputRecord, result: InputConsumer) {
    STATUS.lock().hands[hand.index()].input_consumer(input, result);
    if input.grip.pressed {
        note(|| {
            let input = InputRecord {
                consumer: result,
                ..input
            };
            event(hand, GripStage::Input, &input, None)
        });
    }
}

/// Record the reach that the actual pickup input consumer places in its queue, and note it.
pub(crate) fn queued(hand: Hand, input: InputRecord, target: Reach) -> RequestKey {
    let key = STATUS.lock().hands[hand.index()].queued(input, target);
    note(|| {
        let input = InputRecord {
            consumer: InputConsumer::Queued,
            ..input
        };
        event(
            hand,
            GripStage::Input,
            &input,
            Some(&request_record(key, input)),
        )
    });
    key
}

/// Attach the actual consumer result only to the request carrying this key, and note it.
pub(crate) fn consumed(hand: Hand, key: RequestKey, result: ConsumerResult) {
    let record = {
        let mut status = STATUS.lock();
        status.hands[hand.index()].consumed(key, result);
        status.hands[hand.index()].request_mut(key).copied()
    };
    if let Some(record) = record {
        note(|| event(hand, GripStage::Request, &record.input, Some(&record)));
    }
}

/// Retain already-read body values and the outcome of the existing drive call. A request's first
/// drive is noted as an event.
pub(crate) fn driven(hand: Hand, key: RequestKey, drive: DriveRecord) {
    let first = {
        let mut status = STATUS.lock();
        status.hands[hand.index()]
            .request_mut(key)
            .and_then(|record| {
                let first = record.drive.is_none();
                record.drive = Some(drive);
                first.then_some(*record)
            })
    };
    if let Some(record) = first {
        note(|| event(hand, GripStage::Drive, &record.input, Some(&record)));
    }
}

fn request_record(key: RequestKey, input: InputRecord) -> RequestRecord {
    RequestRecord {
        key,
        input,
        consumer: ConsumerResult::Queued,
        drive: None,
    }
}

fn note(event: impl FnOnce() -> GripAcquisition) {
    events::note_with(|| EventKind::GripAcquisition(Box::new(event())));
}

/// The event for one acquisition stage. Nested runtime values are recorded as their Debug text.
fn event(
    hand: Hand,
    stage: GripStage,
    input: &InputRecord,
    request: Option<&RequestRecord>,
) -> GripAcquisition {
    GripAcquisition {
        hand: hand_to_file(hand),
        stage,
        sequence: input.sequence,
        observation: input.observation,
        episode: input.episode,
        request: request.map(|record| record.key.request),
        grip_down: input.grip.down,
        grip_pressed: input.grip.pressed,
        grip_released: input.grip.released,
        latch: debug_label(&input.latch),
        grab: input.grab.map(|grab| debug_label(&grab)),
        chosen: input.chosen.as_ref().map(candidate),
        candidates: input.candidates.iter().flatten().map(candidate).collect(),
        candidates_omitted: u32::try_from(input.candidates_omitted).unwrap_or(u32::MAX),
        scan: input.scan.as_ref().map(|scan| GripScan {
            entries: scan
                .entries
                .iter()
                .flatten()
                .map(|entry| GripScanEntry {
                    branch: debug_label(&entry.branch),
                    object: entry.object.map(|object| object.get() as u64),
                    body: entry.body,
                    door: entry.door.map(|door| debug_label(&door)),
                    point: entry.point,
                    distance_m: entry.distance,
                    mass_kg: entry.mass_kg,
                    result: debug_label(&entry.result),
                })
                .collect(),
            omitted: u32::try_from(scan.omitted).unwrap_or(u32::MAX),
            ray_ran: scan.ray_ran,
            handles_ran: scan.handles_ran,
            ray_selected: scan.ray_selected.map(|reach| debug_label(&reach)),
            handle_selected: scan.handle_selected.map(|reach| debug_label(&reach)),
            selected: scan.selected.map(|reach| debug_label(&reach)),
        }),
        consumer: debug_label(&input.consumer),
        target: request.map(|record| debug_label(&record.key.target)),
        result: request
            .filter(|_| stage != GripStage::Input)
            .map(|record| debug_label(&record.consumer)),
        drive: request
            .and_then(|record| record.drive)
            .map(|drive| GripDrive {
                object: drive.object as u64,
                body: drive.body,
                before: debug_label(&drive.before),
                commanded: drive.commanded.map(|commanded| debug_label(&commanded)),
                result: drive.result.map(|result| debug_label(&result)),
                report: drive.report.map(|report| debug_label(&report)),
            }),
        target_point: request
            .map(|record| record.key.target.point)
            .or_else(|| input.scan.as_ref()?.selected.map(|reach| reach.point)),
    }
}

fn candidate(candidate: &Candidate) -> GripCandidate {
    GripCandidate {
        resolver_index: u32::try_from(candidate.resolved.index).unwrap_or(u32::MAX),
        distance_m: candidate.resolved.distance,
        tier: debug_label(&candidate.resolved.tier),
        volume: debug_label(&candidate.resolved.volume),
        observed: candidate.observed.map(|reach| debug_label(&reach)),
        lost: candidate.lost.map(|lost| Label::Owned(lost.describe())),
    }
}

impl HandEvidence {
    const fn new() -> Self {
        Self {
            latest: None,
            last_press: None,
            requests: [None; REQUEST_CAPACITY],
            evicted: 0,
            observation: 0,
            episode: 0,
            held: false,
            next_request: 0,
            next_slot: 0,
        }
    }

    fn input(&mut self, sequence: u64, tick: &Tick, state: InputState, hand: Hand) -> InputRecord {
        let i = hand.index();
        let grip = state.grips[i];
        self.observation = self.observation.saturating_add(1);
        if grip.pressed || (grip.held() && !self.held) {
            self.episode = self.episode.saturating_add(1);
        }
        self.held = grip.held();
        let resolved = &tick.resolved[i];
        let candidate = |reach: resolve::Reach| Candidate {
            resolved: reach,
            observed: tick.targets[reach.index].observed,
            lost: resolved
                .chosen
                .filter(|chosen| chosen.index != reach.index)
                .map(|chosen| resolve::lost_to(reach, chosen)),
        };
        let mut candidates = [None; CANDIDATE_CAPACITY];
        for (slot, reach) in candidates.iter_mut().zip(&resolved.reaches) {
            *slot = Some(candidate(*reach));
        }
        let record = InputRecord {
            scan: None,
            sequence,
            observation: self.observation,
            episode: self.episode,
            grip,
            latch: state.latches[i],
            grab: state.grabs[i],
            chosen: resolved.chosen.map(candidate),
            candidates,
            candidates_omitted: resolved.reaches.len().saturating_sub(CANDIDATE_CAPACITY),
            consumer: InputConsumer::NotRun,
        };
        self.latest = Some(record);
        if grip.pressed {
            self.last_press = Some(record);
        }
        record
    }

    fn input_consumer(&mut self, input: InputRecord, result: InputConsumer) {
        for record in [&mut self.latest, &mut self.last_press]
            .into_iter()
            .flatten()
        {
            if record.sequence == input.sequence
                && record.observation == input.observation
                && record.episode == input.episode
                && record.grip == input.grip
                && record.latch == input.latch
                && record.grab == input.grab
            {
                record.consumer = result;
            }
        }
    }

    fn queued(&mut self, mut input: InputRecord, target: Reach) -> RequestKey {
        self.next_request = self.next_request.saturating_add(1);
        let key = RequestKey {
            sequence: input.sequence,
            episode: input.episode,
            request: self.next_request,
            target,
        };
        self.input_consumer(input, InputConsumer::Queued);
        input.consumer = InputConsumer::Queued;
        self.evicted = self
            .evicted
            .saturating_add(u64::from(self.requests[self.next_slot].is_some()));
        self.requests[self.next_slot] = Some(RequestRecord {
            key,
            input,
            consumer: ConsumerResult::Queued,
            drive: None,
        });
        self.next_slot = (self.next_slot + 1) % REQUEST_CAPACITY;
        key
    }

    fn consumed(&mut self, key: RequestKey, result: ConsumerResult) {
        if let Some(record) = self.request_mut(key) {
            record.consumer = result;
        }
    }

    fn request_mut(&mut self, key: RequestKey) -> Option<&mut RequestRecord> {
        self.requests.iter_mut().flatten().find(|record| {
            record.key.sequence == key.sequence
                && record.key.episode == key.episode
                && record.key.request == key.request
        })
    }
}

static STATUS: Mutex<Status> = Mutex::new(Status {
    hands: [HandEvidence::new(); 2],
});
