//! Bounded input observations shared by capture, replay, and host synthesis.
//!
//! A group is one coherent observation, not a hardware event queue. Changes within it have no
//! inferred order. Runtime change times remain in their own clock domain and never become logical
//! input time by an implicit conversion. Source paths describe possible bindings, not contributors.

pub mod policy;
pub mod replay;

use std::{collections::VecDeque, sync::Arc};

/// The current uncompressed input-record schema, independent of the tracking frame codec.
pub const INPUT_VERSION: u16 = 2;
pub const MAX_ROUTES: usize = 128;
pub const MAX_SOURCES: usize = 16;
pub const MAX_SOURCE_PATH: usize = 256;
pub const HISTORY_CAPACITY: usize = 128;
/// Maximum encoded group payload from the fixed header and the bounded route/source fields.
pub const MAX_INPUT_PAYLOAD: usize = 128 + MAX_ROUTES * (32 + MAX_SOURCES * (2 + MAX_SOURCE_PATH));

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ClockDomain {
    HostMonotonic,
    Recorded,
    Synthetic,
}

/// Immutable logical input time and ordering within an explicitly allocated epoch.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct InputStamp {
    pub epoch: u64,
    pub clock: ClockDomain,
    pub time_ns: u64,
    pub sync: u64,
    pub sequence: u64,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GroupOrder {
    /// The whole sync is observed together; reading actions in a loop proves no internal order.
    UnorderedSync,
    /// The producer actually observed a timestamp group in an ordered history.
    ObservedGroup,
}

/// Stable route identity within the recorded action schema. Raw controls are LS, RS, and right B.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum InputRoute {
    Raw(u8),
    Semantic { set: u8, action: u16, hand: u8 },
}

/// IEEE-754 bits are stored directly, including signed zero and NaN payloads.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ValueBits {
    Bool(bool),
    Float(u32),
    Vec2([u32; 2]),
}

/// OpenXR timing is not assumed to share an origin with the logical input clock.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct RuntimeChange {
    pub changed_since_sync: bool,
    pub last_change_time_ns: i64,
}

/// A successful empty query is distinct from no query, a failed query, and bounded truncation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceQuery {
    NotEnumerated,
    Failed,
    Paths(Arc<[String]>),
    LimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSample {
    pub route: InputRoute,
    pub value: ValueBits,
    pub active: bool,
    pub down: bool,
    pub change: Option<RuntimeChange>,
    pub sources: SourceQuery,
}

/// Raw and semantic samples from the same sync, before masks or accumulated edge latches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputGroup {
    pub stamp: InputStamp,
    pub order: GroupOrder,
    pub source_generation: u64,
    pub binding_generation: u64,
    pub action_schema: u32,
    pub context: u64,
    pub focused: bool,
    pub samples: Vec<RouteSample>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DiscontinuityReason {
    Overflow,
    Unavailable,
    GenerationChanged,
    ClockChanged,
    InvalidOrder,
    LegacyOrderAbsent,
    BoundsExceeded,
}

/// A discontinuity has no invented timestamp. The watermark remains the last accepted group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputRecord {
    Group(InputGroup),
    Policy(policy::PolicyEvaluation),
    Discontinuity {
        reason: DiscontinuityReason,
        discarded: u64,
    },
}

/// Capture placement is separate from immutable logical input time. A missing frame sequence means
/// no controller snapshot was published for this record; replay must not invent an association.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedInput {
    pub capture_time_ns: u64,
    pub frame_sequence: Option<u64>,
    pub record: InputRecord,
}

/// A single-consumer history. Overflow discards the incomplete prefix and precedes the retained
/// group with a discontinuity; consumers must reset ownership and require real neutral.
#[derive(Debug, Default)]
pub struct InputHistory {
    pending: VecDeque<InputRecord>,
    watermark: Option<InputGroupIdentity>,
    pub discarded: u64,
}

impl InputHistory {
    pub fn push(&mut self, group: InputGroup) {
        if !group.is_bounded() {
            self.reject_bounds();
            return;
        }
        let identity = InputGroupIdentity::from(&group);
        if let Some(previous) = self.watermark {
            if identity.stamp.epoch != previous.stamp.epoch {
                if identity.stamp.epoch <= previous.stamp.epoch {
                    self.interrupt(DiscontinuityReason::InvalidOrder);
                    return;
                }
                self.interrupt(DiscontinuityReason::ClockChanged);
            } else if identity.stamp.clock != previous.stamp.clock
                || identity.stamp.time_ns <= previous.stamp.time_ns
                || identity.stamp.sequence <= previous.stamp.sequence
                || identity.stamp.sync < previous.stamp.sync
                || (identity.stamp.sync == previous.stamp.sync
                    && (identity.order != GroupOrder::ObservedGroup
                        || previous.order != GroupOrder::ObservedGroup))
            {
                self.interrupt(DiscontinuityReason::InvalidOrder);
                return;
            } else if identity.source != previous.source || identity.binding != previous.binding {
                self.interrupt(DiscontinuityReason::GenerationChanged);
            }
        }
        if self.pending.len() >= HISTORY_CAPACITY {
            self.interrupt(DiscontinuityReason::Overflow);
        }
        self.watermark = Some(identity);
        self.pending.push_back(InputRecord::Group(group));
    }

    /// Append the recorded stream verbatim. Its boundaries belong to the producer, not to the
    /// playback batching. Only a local queue overflow can discard an accepted recorded prefix.
    pub fn publish(&mut self, record: InputRecord) {
        if matches!(&record, InputRecord::Group(group) if !group.is_bounded())
            || matches!(&record, InputRecord::Policy(policy) if !policy.is_valid())
        {
            self.reject_bounds();
            return;
        }
        if self.pending.len() >= HISTORY_CAPACITY {
            self.interrupt(DiscontinuityReason::Overflow);
        }
        if let InputRecord::Discontinuity { discarded, .. } = &record {
            self.discarded = self.discarded.saturating_add(*discarded);
        }
        self.pending.push_back(record);
    }

    fn reject_bounds(&mut self) {
        self.interrupt(DiscontinuityReason::BoundsExceeded);
        self.discarded = self.discarded.saturating_add(1);
        if let Some(InputRecord::Discontinuity { discarded, .. }) = self.pending.back_mut() {
            *discarded = discarded.saturating_add(1);
        }
    }

    pub fn interrupt(&mut self, reason: DiscontinuityReason) {
        let lost = self.pending.iter().fold(0u64, |count, record| {
            count.saturating_add(match record {
                InputRecord::Group(_) | InputRecord::Policy(_) => 1,
                InputRecord::Discontinuity { discarded, .. } => *discarded,
            })
        });
        let newly_lost = self
            .pending
            .iter()
            .filter(|r| matches!(r, InputRecord::Group(_) | InputRecord::Policy(_)))
            .count();
        self.discarded = self.discarded.saturating_add(newly_lost as u64);
        self.pending.clear();
        self.pending.push_back(InputRecord::Discontinuity {
            reason,
            discarded: lost,
        });
    }

    /// Taking twice without a publication returns an empty history, never repeated edges.
    pub fn take(&mut self) -> Vec<InputRecord> {
        self.pending.drain(..).collect()
    }
}

impl InputGroup {
    pub fn is_bounded(&self) -> bool {
        self.samples.len() <= MAX_ROUTES
            && self.samples.iter().all(|sample| {
                let route_valid = match sample.route {
                    InputRoute::Raw(control) => control < 3,
                    InputRoute::Semantic { set, hand, .. } => set < 7 && hand < 2,
                };
                route_valid
                    && match &sample.sources {
                        SourceQuery::Paths(paths) => {
                            paths.len() <= MAX_SOURCES
                                && paths.iter().all(|path| path.len() <= MAX_SOURCE_PATH)
                        }
                        _ => true,
                    }
            })
            && self.samples.iter().enumerate().all(|(index, sample)| {
                !self.samples[..index]
                    .iter()
                    .any(|before| before.route == sample.route)
            })
    }
}

#[derive(Debug, Copy, Clone)]
struct InputGroupIdentity {
    stamp: InputStamp,
    order: GroupOrder,
    source: u64,
    binding: u64,
}

impl From<&InputGroup> for InputGroupIdentity {
    fn from(group: &InputGroup) -> Self {
        Self {
            stamp: group.stamp,
            order: group.order,
            source: group.source_generation,
            binding: group.binding_generation,
        }
    }
}

#[cfg(test)]
mod tests;
