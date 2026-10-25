//! Bounded evidence from the grip resolver, pickup request queue, and pickup consumer.
//!
//! Records contain copied observations only. Missing provider identity is distinct from a provider
//! reporting no target. The leaf slot never logs and never calls an engine reader.

pub(crate) mod scan;
#[cfg(test)]
mod tests;

use parking_lot::Mutex;

use crate::controllers::{
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

/// Record a branch in the actual pickup input consumer.
pub(crate) fn input_consumer(hand: Hand, input: InputRecord, result: InputConsumer) {
    STATUS.lock().hands[hand.index()].input_consumer(input, result);
}

/// Record the reach that the actual pickup input consumer places in its queue.
pub(crate) fn queued(hand: Hand, input: InputRecord, target: Reach) -> RequestKey {
    STATUS.lock().hands[hand.index()].queued(input, target)
}

/// Attach the actual consumer result only to the request carrying this key.
pub(crate) fn consumed(hand: Hand, key: RequestKey, result: ConsumerResult) {
    STATUS.lock().hands[hand.index()].consumed(key, result);
}

/// Retain already-read body values and the outcome of the existing drive call.
pub(crate) fn driven(hand: Hand, key: RequestKey, drive: DriveRecord) {
    let mut status = STATUS.lock();
    if let Some(record) = status.hands[hand.index()].request_mut(key) {
        record.drive = Some(drive);
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
