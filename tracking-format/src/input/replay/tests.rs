use super::*;
use crate::input::{
    GroupOrder, InputGroup,
    policy::{AcceptedCommand, AcceptedSingle, POLICY_A_VERSION, PolicyEvaluation, PolicyOutcome},
};

fn group(sequence: u64) -> InputRecord {
    InputRecord::Group(InputGroup {
        stamp: InputStamp {
            epoch: 1,
            clock: ClockDomain::Synthetic,
            time_ns: sequence,
            sync: sequence,
            sequence,
        },
        order: GroupOrder::UnorderedSync,
        binding_generation: 1,
        source_generation: 1,
        action_schema: 1,
        context: 0,
        focused: true,
        samples: Vec::new(),
    })
}

fn policy(sequence: u64) -> InputRecord {
    let InputRecord::Group(group) = group(sequence) else {
        unreachable!()
    };
    InputRecord::Policy(PolicyEvaluation {
        version: POLICY_A_VERSION,
        stamp: group.stamp,
        action_schema: 1,
        timing: None,
        context: 0,
        gameplay: false,
        outcome: PolicyOutcome::Cancelled,
        neutral: false,
        singles: [AcceptedSingle::None; 3],
        single_down: [false; 3],
        withheld: [true; 3],
        command: AcceptedCommand::None,
        raw: [0; 3],
        direct: Default::default(),
        claims: 0,
        commands: 0,
    })
}

fn captured(records: Vec<InputRecord>) -> Vec<CapturedInput> {
    records
        .into_iter()
        .enumerate()
        .map(|(index, record)| CapturedInput {
            capture_time_ns: index as u64 + 1,
            frame_sequence: None,
            record,
        })
        .collect()
}

fn reference(records: &[CapturedInput], start: usize, deadline: u64) -> Range<usize> {
    let mut end = start
        + records[start..]
            .iter()
            .take_while(|record| record.capture_time_ns <= deadline)
            .count();
    loop {
        let incomplete = records[start..end].iter().enumerate().find_map(|(offset, record)| {
            let InputRecord::Group(group) = &record.record else { return None; };
            let index = start + offset;
            records[index + 1..].iter().position(|candidate| matches!(&candidate.record, InputRecord::Policy(policy) if policy.stamp == group.stamp))
                .map(|offset| index + 1 + offset).filter(|&matching| matching >= end).map(|_| index)
        });
        let Some(incomplete) = incomplete else {
            break;
        };
        end = incomplete;
    }
    start..end
}

#[test]
fn prepared_units_preserve_first_later_match_duplicate_and_gap_semantics() {
    for records in [
        vec![group(1), group(2), policy(1), policy(2)],
        vec![policy(1), group(1), policy(1), policy(1)],
        vec![group(1), group(1), policy(1), group(2)],
        vec![group(1), group(2), policy(2), policy(1), group(3)],
    ] {
        let records = captured(records);
        let prepared = PreparedInput::new(&records);
        for split in 0..=records.len() {
            let mut cursor = InputReplay::default();
            let mut next = 0;
            for deadline in [split as u64, u64::MAX] {
                let expected = reference(&records, next, deadline);
                next = expected.end;
                assert_eq!(
                    cursor.advance(&prepared, deadline),
                    expected,
                    "split {split}"
                );
            }
            assert!(cursor.finished(records.len()));
        }
    }
}

#[test]
fn large_missing_and_interleaved_recordings_have_bounded_preparation_and_frame_work() {
    const GROUPS: usize = 100_000;
    for interleaved in [false, true] {
        let mut source = (1..=GROUPS as u64).map(group).collect::<Vec<_>>();
        if interleaved {
            source.extend((1..=GROUPS as u64).map(policy));
        }
        let records = captured(source);
        let prepared = PreparedInput::new(&records);
        assert_eq!(prepared.work.record_visits, records.len() * 2);
        assert_eq!(prepared.work.index_operations, records.len());
        let mut cursor = InputReplay::default();
        let mut consumed = 0;
        for deadline in 0..=records.len() as u64 {
            let range = cursor.advance(&prepared, deadline);
            assert_eq!(range.start, consumed);
            consumed = range.end;
            if interleaved && deadline < records.len() as u64 {
                assert_eq!(consumed, 0);
            }
        }
        assert_eq!(consumed, records.len());
        assert!(cursor.finished(records.len()));
        assert!(cursor.unit_checks <= records.len() * 2 + 1);
    }
}
