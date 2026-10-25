//! Capture-time scheduling of prepared group/decision units without logical restamping.

use std::{collections::BTreeMap, ops::Range};

use crate::input::{CapturedInput, ClockDomain, InputRecord, InputStamp};

/// Dependency-complete units prepared once from the complete loaded recording, not a streaming
/// prefix. Build this on the loader thread. Missing decisions remain actual gaps for the consumer.
/// Each group's dependency is the first later Policy with the same full stamp, including when
/// duplicate groups or decisions exist. Preparation is O(N log N); storage is O(N).
#[derive(Debug, Default)]
pub struct PreparedInput {
    units: Vec<Unit>,
    #[cfg(test)]
    work: PreparationWork,
}

impl PreparedInput {
    pub fn new(records: &[CapturedInput]) -> Self {
        let mut next_policy = BTreeMap::new();
        let mut dependencies = Vec::with_capacity(records.len());
        dependencies.extend(0..records.len());
        #[cfg(test)]
        let mut work = PreparationWork::default();
        for (index, record) in records.iter().enumerate().rev() {
            #[cfg(test)]
            {
                work.record_visits += 1;
            }
            match &record.record {
                InputRecord::Policy(policy) => {
                    next_policy.insert(stamp_key(policy.stamp), index);
                    #[cfg(test)]
                    {
                        work.index_operations += 1;
                    }
                }
                InputRecord::Group(group) => {
                    dependencies[index] = next_policy
                        .get(&stamp_key(group.stamp))
                        .copied()
                        .unwrap_or(index);
                    #[cfg(test)]
                    {
                        work.index_operations += 1;
                    }
                }
                InputRecord::Discontinuity { .. } => {}
            }
        }
        let mut units = Vec::new();
        let mut next = 0;
        while next < records.len() {
            let mut end = next + 1;
            let mut capture_time_ns = 0;
            while next < end {
                end = end.max(dependencies[next] + 1);
                capture_time_ns = capture_time_ns.max(records[next].capture_time_ns);
                next += 1;
                #[cfg(test)]
                {
                    work.record_visits += 1;
                }
            }
            units.push(Unit {
                end,
                capture_time_ns,
            });
        }
        Self {
            units,
            #[cfg(test)]
            work,
        }
    }
}

/// Independent from the pose cursor. Frame advancement visits each prepared unit at most once,
/// plus one check of the next blocked unit per call. It performs no record or stamp matching.
#[derive(Debug, Default)]
pub struct InputReplay {
    next: usize,
    unit: usize,
    #[cfg(test)]
    unit_checks: usize,
}

impl InputReplay {
    pub fn advance(&mut self, prepared: &PreparedInput, capture_time_ns: u64) -> Range<usize> {
        let start = self.next;
        while let Some(unit) = prepared.units.get(self.unit) {
            #[cfg(test)]
            {
                self.unit_checks += 1;
            }
            if unit.capture_time_ns > capture_time_ns {
                break;
            }
            self.next = unit.end;
            self.unit += 1;
        }
        start..self.next
    }

    pub fn finished(&self, len: usize) -> bool {
        self.next >= len
    }
}

#[derive(Debug)]
struct Unit {
    end: usize,
    capture_time_ns: u64,
}

fn stamp_key(stamp: InputStamp) -> (u64, u8, u64, u64, u64) {
    let clock = match stamp.clock {
        ClockDomain::HostMonotonic => 0,
        ClockDomain::Recorded => 1,
        ClockDomain::Synthetic => 2,
    };
    (
        stamp.epoch,
        clock,
        stamp.time_ns,
        stamp.sync,
        stamp.sequence,
    )
}

#[cfg(test)]
#[derive(Debug, Default)]
struct PreparationWork {
    record_visits: usize,
    index_operations: usize,
}

#[cfg(test)]
mod tests;
