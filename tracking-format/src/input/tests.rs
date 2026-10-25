use super::*;
use crate::{
    Header, Recording, StartMarker, decode, encode_input, encode_input_preamble, encode_preamble,
    model::Hand,
};

fn group(time: u64, sequence: u64) -> InputGroup {
    InputGroup {
        stamp: InputStamp {
            epoch: 8,
            clock: ClockDomain::Synthetic,
            time_ns: time,
            sync: 4,
            sequence,
        },
        order: GroupOrder::ObservedGroup,
        source_generation: 3,
        binding_generation: 5,
        action_schema: 4,
        context: 0,
        focused: true,
        samples: vec![RouteSample {
            route: InputRoute::Raw(0),
            value: ValueBits::Bool(true),
            active: true,
            down: true,
            change: Some(RuntimeChange {
                changed_since_sync: true,
                last_change_time_ns: -27,
            }),
            sources: SourceQuery::Paths(
                vec!["/user/hand/left/input/thumbstick/click".to_owned()].into(),
            ),
        }],
    }
}

fn header() -> Header {
    Header {
        build: "synthetic".to_owned(),
        recorded_at: "fixture".to_owned(),
        action_schema: 4,
        start: StartMarker {
            transform: [0.0; 16],
            bookmark: None,
        },
        dominant_hand: Hand::Right,
        world_scale: 1.0,
        time_scale: 1.0,
    }
}

fn captured(record: InputRecord) -> CapturedInput {
    CapturedInput {
        capture_time_ns: 42,
        frame_sequence: Some(7),
        record,
    }
}

fn wire(records: &[InputRecord]) -> Vec<u8> {
    let mut bytes = encode_input_preamble(&header());
    for record in records {
        bytes.extend(encode_input(&captured(record.clone())).expect("bounded fixture"));
    }
    bytes
}

#[test]
fn multiple_observed_groups_before_one_tick_are_consumed_once() {
    let mut queue = InputHistory::default();
    let first = group(10, 1);
    let mut second = group(20, 2);
    second.samples[0].value = ValueBits::Bool(false);
    second.samples[0].down = false;
    queue.push(first.clone());
    queue.push(second.clone());
    assert_eq!(
        queue.take(),
        vec![InputRecord::Group(first), InputRecord::Group(second)]
    );
    assert!(queue.take().is_empty());
}

#[test]
fn reset_and_rejected_history_do_not_rewrite_accepted_time() {
    let mut queue = InputHistory::default();
    queue.push(group(100, 10));
    queue.take();
    queue.interrupt(DiscontinuityReason::Unavailable);
    queue.take();
    queue.push(group(50, 11));
    assert!(matches!(
        queue.take().as_slice(),
        [InputRecord::Discontinuity {
            reason: DiscontinuityReason::InvalidOrder,
            ..
        }]
    ));
    queue.push(group(101, 11));
    assert_eq!(queue.take(), vec![InputRecord::Group(group(101, 11))]);
}

#[test]
fn equal_time_cannot_be_split_into_an_invented_order() {
    let mut queue = InputHistory::default();
    queue.push(group(10, 1));
    queue.push(group(10, 2));
    assert!(matches!(
        queue.take().as_slice(),
        [InputRecord::Discontinuity {
            reason: DiscontinuityReason::InvalidOrder,
            discarded: 1
        }]
    ));
}

#[test]
fn overflow_is_bounded_and_explicit_not_a_merged_tap() {
    let mut queue = InputHistory::default();
    for i in 1..=HISTORY_CAPACITY as u64 + 1 {
        queue.push(group(i, i));
    }
    let records = queue.take();
    assert_eq!(records.len(), 2);
    assert_eq!(queue.discarded, HISTORY_CAPACITY as u64);
    assert_eq!(
        records[0],
        InputRecord::Discontinuity {
            reason: DiscontinuityReason::Overflow,
            discarded: HISTORY_CAPACITY as u64
        }
    );
    assert!(
        matches!(&records[1], InputRecord::Group(last) if last.stamp.sequence == HISTORY_CAPACITY as u64 + 1)
    );
}

#[test]
fn generation_and_epoch_changes_reset_without_restamping_groups() {
    let mut queue = InputHistory::default();
    queue.push(group(10, 1));
    queue.take();
    let mut changed = group(20, 2);
    changed.binding_generation += 1;
    queue.push(changed.clone());
    assert_eq!(
        queue.take(),
        vec![
            InputRecord::Discontinuity {
                reason: DiscontinuityReason::GenerationChanged,
                discarded: 0
            },
            InputRecord::Group(changed)
        ]
    );
    let mut new_epoch = group(1, 1);
    new_epoch.stamp.epoch += 1;
    queue.push(new_epoch.clone());
    assert_eq!(
        queue.take(),
        vec![
            InputRecord::Discontinuity {
                reason: DiscontinuityReason::ClockChanged,
                discarded: 0
            },
            InputRecord::Group(new_epoch)
        ]
    );
    queue.push(group(1000, 1000));
    assert!(matches!(
        queue.take().as_slice(),
        [InputRecord::Discontinuity {
            reason: DiscontinuityReason::InvalidOrder,
            ..
        }]
    ));
}

#[test]
fn actual_wire_preserves_bits_sources_timing_and_discontinuities() {
    let mut observed = group(123, 77);
    observed.order = GroupOrder::UnorderedSync;
    observed.samples.extend([
        RouteSample {
            route: InputRoute::Semantic {
                set: 1,
                action: 0,
                hand: 0,
            },
            value: ValueBits::Float(0x8000_0000),
            active: true,
            down: false,
            change: None,
            sources: SourceQuery::Paths(Vec::new().into()),
        },
        RouteSample {
            route: InputRoute::Semantic {
                set: 1,
                action: 0,
                hand: 1,
            },
            value: ValueBits::Vec2([0x7fc0_1234, 0xff80_0000]),
            active: false,
            down: false,
            change: Some(RuntimeChange {
                changed_since_sync: false,
                last_change_time_ns: i64::MIN,
            }),
            sources: SourceQuery::Failed,
        },
    ]);
    let records = vec![
        InputRecord::Group(observed),
        InputRecord::Discontinuity {
            reason: DiscontinuityReason::Unavailable,
            discarded: 9,
        },
    ];
    let decoded = decode(&wire(&records)).unwrap();
    assert_eq!(decoded.version, 2);
    assert_eq!(
        decoded.input,
        records.into_iter().map(captured).collect::<Vec<_>>()
    );
    assert!(!decoded.truncated);
}

#[test]
fn v1_decode_has_no_fabricated_input_order() {
    let mut bytes = encode_preamble(&header());
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    let Recording { version, input, .. } = decode(&bytes).unwrap();
    assert_eq!(version, 1);
    assert!(input.is_empty());
    let decoded = decode(&bytes).unwrap();
    let mut replay = replay::InputReplay::default();
    assert_eq!(replay.advance(&decoded.input, 100), 0..0);
    assert!(replay.finished(0));
}

#[test]
fn interrupted_input_tail_recovers_only_complete_records() {
    let first = InputRecord::Group(group(10, 1));
    let second = InputRecord::Group(group(20, 2));
    let complete = wire(std::slice::from_ref(&first));
    let bytes = wire(&[first.clone(), second]);
    for end in complete.len() + 1..bytes.len() {
        let decoded = decode(&bytes[..end]).unwrap();
        assert!(decoded.truncated);
        assert_eq!(decoded.input, vec![captured(first.clone())]);
    }
}

#[test]
fn producer_bounds_and_hostile_reserved_bits_are_rejected() {
    let mut oversized = group(10, 1);
    oversized.samples[0].sources = SourceQuery::Paths(vec!["x".repeat(MAX_SOURCE_PATH + 1)].into());
    assert!(encode_input(&captured(InputRecord::Group(oversized))).is_err());
    let observed = group(10, 1);
    let mut encoded = encode_input(&captured(InputRecord::Group(observed))).unwrap();
    // Framing, schema/kind, group header, raw route, and boolean value precede the flags.
    let flags_offset =
        5 + 2 + 8 + 1 + 8 + 1 + 8 + 1 + 8 + 8 + 8 + 1 + 8 + 8 + 4 + 8 + 1 + 2 + 2 + 2;
    encoded[flags_offset] |= 0x80;
    let mut bytes = encode_input_preamble(&header());
    bytes.extend(encoded);
    assert!(decode(&bytes).is_err());
    let mut hostile = encode_input_preamble(&header());
    hostile.push(crate::TAG_INPUT);
    hostile.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        decode(&hostile).unwrap_err().kind,
        crate::DecodeErrorKind::InvalidValue {
            field: "input payload length",
            ..
        }
    ));
}

#[test]
fn recorded_boundaries_generations_and_epochs_survive_every_batch_size() {
    let mut changed = group(20, 2);
    changed.binding_generation += 1;
    changed.source_generation += 1;
    let mut epoch = group(1, 1);
    epoch.stamp.epoch += 1;
    let records = vec![
        InputRecord::Group(group(10, 1)),
        InputRecord::Discontinuity {
            reason: DiscontinuityReason::Unavailable,
            discarded: 0,
        },
        InputRecord::Group(changed),
        InputRecord::Discontinuity {
            reason: DiscontinuityReason::ClockChanged,
            discarded: 0,
        },
        InputRecord::Group(epoch),
    ];
    let decoded = decode(&wire(&records)).unwrap();
    for batch_size in [1, 2, 32] {
        let mut queue = InputHistory::default();
        let mut actual = Vec::new();
        for batch in decoded.input.chunks(batch_size) {
            for entry in batch {
                queue.publish(entry.record.clone());
            }
            actual.extend(queue.take());
        }
        assert_eq!(actual, records);
        assert_eq!(queue.discarded, 0);
    }
}

#[test]
fn one_sync_cannot_be_split_or_mixed_when_any_group_is_unordered() {
    for (first_order, second_order, accepted) in [
        (GroupOrder::UnorderedSync, GroupOrder::UnorderedSync, false),
        (GroupOrder::UnorderedSync, GroupOrder::ObservedGroup, false),
        (GroupOrder::ObservedGroup, GroupOrder::UnorderedSync, false),
        (GroupOrder::ObservedGroup, GroupOrder::ObservedGroup, true),
    ] {
        let mut first = group(10, 1);
        first.order = first_order;
        let mut second = group(20, 2);
        second.order = second_order;
        let mut queue = InputHistory::default();
        queue.push(first.clone());
        queue.push(second.clone());
        let actual = queue.take();
        if accepted {
            assert_eq!(
                actual,
                vec![InputRecord::Group(first), InputRecord::Group(second)]
            );
        } else {
            assert_eq!(
                actual,
                vec![InputRecord::Discontinuity {
                    reason: DiscontinuityReason::InvalidOrder,
                    discarded: 1
                }]
            );
        }
    }
}

#[test]
fn bounds_rejection_counts_the_incoming_group_exactly_once_on_wire() {
    for prefix in [false, true] {
        let mut queue = InputHistory::default();
        if prefix {
            queue.push(group(10, 1));
        }
        let mut invalid = group(20, 2);
        invalid.samples[0].sources =
            SourceQuery::Paths(vec!["x".repeat(MAX_SOURCE_PATH + 1)].into());
        queue.push(invalid);
        let expected = u64::from(prefix) + 1;
        let records = queue.take();
        assert_eq!(queue.discarded, expected);
        assert_eq!(
            records,
            vec![InputRecord::Discontinuity {
                reason: DiscontinuityReason::BoundsExceeded,
                discarded: expected
            }]
        );
        assert_eq!(decode(&wire(&records)).unwrap().input[0].record, records[0]);
    }
}

#[test]
fn replay_capture_schedule_does_not_restamp_logical_time() {
    let records = [
        InputRecord::Group(group(100, 1)),
        InputRecord::Group(group(200, 2)),
    ];
    let mut bytes = encode_input_preamble(&header());
    for (index, record) in records.iter().enumerate() {
        bytes.extend(
            encode_input(&CapturedInput {
                capture_time_ns: 10 + index as u64 * 20,
                frame_sequence: None,
                record: record.clone(),
            })
            .unwrap(),
        );
    }
    let decoded = decode(&bytes).unwrap();
    for deadlines in [vec![0, 10, 29, 30], vec![30]] {
        let mut cursor = replay::InputReplay::default();
        let mut actual = Vec::new();
        for deadline in deadlines {
            let range = cursor.advance(&decoded.input, deadline);
            actual.extend(
                decoded.input[range]
                    .iter()
                    .map(|entry| entry.record.clone()),
            );
        }
        assert_eq!(actual, records);
        assert!(cursor.finished(2));
        assert_eq!(cursor.advance(&decoded.input, u64::MAX), 2..2);
    }
}

#[test]
fn replay_batching_preserves_recorded_clock_and_loss_count() {
    let records = vec![
        InputRecord::Group(group(10, 1)),
        InputRecord::Group(group(20, 2)),
    ];
    let decoded = decode(&wire(&records)).unwrap();
    for batch_size in [1, 2, 8] {
        let mut queue = InputHistory::default();
        let mut received = Vec::new();
        for batch in decoded.input.chunks(batch_size) {
            for entry in batch {
                queue.publish(entry.record.clone());
            }
            received.extend(queue.take());
        }
        assert_eq!(received, records);
        queue.publish(InputRecord::Discontinuity {
            reason: DiscontinuityReason::Overflow,
            discarded: 12,
        });
        assert_eq!(
            queue.take(),
            vec![InputRecord::Discontinuity {
                reason: DiscontinuityReason::Overflow,
                discarded: 12
            }]
        );
    }
}
