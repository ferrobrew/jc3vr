use std::time::Instant;

use glam::{Quat, Vec3};

use super::*;
use crate::{
    ControllerSample, DecodeLimits, Frame, Header, SessionSample, StartMarker, decode,
    decode_with_limits, encode_chunked_preamble, encode_frame, encode_input, encode_input_preamble,
    encode_preamble,
    input::{
        CapturedInput, ClockDomain, DiscontinuityReason, GroupOrder, InputGroup, InputRecord,
        InputRoute, InputStamp, RouteSample, SourceQuery, ValueBits,
    },
    model::{ActionSetKind, HAND_JOINT_COUNT, Hand, HandFrame, HandJoint, Pose, Stage},
};

fn header() -> Header {
    Header {
        build: "synthetic chunk fixture".into(),
        recorded_at: "fixture".into(),
        action_schema: 4,
        start: StartMarker {
            transform: [0.0; 16],
            bookmark: Some("origin".into()),
        },
        dominant_hand: Hand::Left,
        world_scale: 1.0,
        time_scale: 1.0,
    }
}

fn frame(sequence: u64) -> Frame {
    let bits = [
        0,
        0x8000_0000,
        1,
        0x7f80_0000,
        0xff80_0000,
        0x7fc1_2345,
        0x7fa0_0001,
    ];
    let joints = std::array::from_fn(|index| {
        let value = f32::from_bits(bits[index % bits.len()]);
        HandJoint {
            pose: Pose {
                position: Vec3::new(value, -0.0, value),
                orientation: Quat::from_xyzw(value, 0.0, -0.0, 1.0),
            },
            radius: value,
            valid: index % 2 == 0,
            tracked: index % 3 == 0,
        }
    });
    let hand = HandFrame {
        joints: Some(joints),
        ..HandFrame::default()
    };
    Frame {
        time_ns: sequence,
        display_time_ns: -10,
        session: SessionSample {
            state: None,
            should_render: true,
            head_tracked: false,
        },
        head: None,
        stage: Stage::default(),
        controllers: Some(ControllerSample {
            focused: true,
            action_set: ActionSetKind::OnFoot,
            hands: [hand.clone(), hand],
        }),
        semantic: None,
        recenters: Vec::new(),
    }
}

fn input(sequence: u64) -> CapturedInput {
    CapturedInput {
        capture_time_ns: sequence,
        frame_sequence: Some(sequence),
        record: InputRecord::Group(InputGroup {
            stamp: InputStamp {
                epoch: 9,
                clock: ClockDomain::Synthetic,
                time_ns: sequence,
                sync: sequence,
                sequence,
            },
            order: GroupOrder::ObservedGroup,
            source_generation: 11,
            binding_generation: 13,
            action_schema: 4,
            context: 7,
            focused: true,
            samples: vec![RouteSample {
                route: InputRoute::Raw(0),
                value: ValueBits::Vec2([0x8000_0000, 0x7fc1_2345]),
                active: true,
                down: false,
                change: None,
                sources: SourceQuery::Paths(vec!["/user/hand/left/input/trigger".into()].into()),
            }],
        }),
    }
}

fn batch(sequence: u64) -> Vec<u8> {
    let mut bytes = encode_input(&input(sequence)).unwrap();
    bytes.extend(encode_frame(&frame(sequence)));
    bytes
}

fn file(chunk: &[u8]) -> Vec<u8> {
    let mut bytes = encode_chunked_preamble(&header());
    bytes.extend_from_slice(chunk);
    bytes
}

fn raw_chunk(records: &[u8], count: u32) -> Vec<u8> {
    stored_chunk(0, records.len() as u32, count, records)
}

fn stored_chunk(storage: u8, expanded: u32, count: u32, stored: &[u8]) -> Vec<u8> {
    let mut metadata = Encoder::default();
    metadata.u8(storage);
    metadata.u8(0);
    metadata.u16(0);
    metadata.u32(expanded);
    metadata.u32(stored.len() as u32);
    metadata.u32(count);
    let mut out = Encoder::default();
    out.u8(TAG_CHUNK);
    out.u32((20 + stored.len()) as u32);
    out.bytes.extend_from_slice(&metadata.bytes);
    out.u32(checksum(&metadata.bytes, stored));
    out.bytes.extend_from_slice(stored);
    out.bytes
}

#[test]
fn chunk_preserves_inner_wire_ieee_bits_joints_and_input_metadata() {
    assert_eq!(HAND_JOINT_COUNT, 26);
    let original = batch(42);
    let encoded = encode_chunk(&original).unwrap();
    let mut reader = Decoder::new(&encoded[5..]);
    let chunk = Chunk::read(&mut reader).unwrap();
    assert_eq!(chunk.expand().unwrap(), original);
    let decoded = decode(&file(&encoded)).unwrap();
    assert_eq!(decoded.version, 3);
    assert_eq!(
        encode_input(&decoded.input[0]).unwrap(),
        encode_input(&input(42)).unwrap()
    );
    assert_eq!(encode_frame(&decoded.frames[0]), encode_frame(&frame(42)));
    assert!(!decoded.truncated);
}

#[test]
fn chunk_recovers_prior_atomic_batches_at_every_cut_position() {
    let preamble = encode_chunked_preamble(&header());
    let first = encode_chunk(&batch(1)).unwrap();
    let second = encode_chunk(&batch(2)).unwrap();
    let mut bytes = preamble.clone();
    bytes.extend(&first);
    bytes.extend(&second);
    for end in 0..=bytes.len() {
        if end < preamble.len() {
            assert!(decode(&bytes[..end]).is_err(), "cut {end}");
            continue;
        }
        let decoded = decode(&bytes[..end]).unwrap();
        let count =
            usize::from(end >= preamble.len() + first.len()) + usize::from(end == bytes.len());
        assert_eq!(decoded.frames.len(), count, "cut {end}");
        assert_eq!(decoded.input.len(), count, "cut {end}");
        assert_eq!(
            decoded.truncated,
            end != preamble.len() && end != preamble.len() + first.len() && end != bytes.len(),
            "cut {end}"
        );
    }
}

#[test]
fn legacy_snapshot_and_input_wires_remain_readable() {
    let mut v1 = encode_preamble(&header());
    v1.extend(encode_frame(&frame(1)));
    let mut v2 = encode_input_preamble(&header());
    v2.extend(batch(2));
    for (bytes, version, inputs) in [(v1, 1, 0), (v2, 2, 1)] {
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.version, version);
        assert_eq!(decoded.frames.len(), 1);
        assert_eq!(decoded.input.len(), inputs);
        assert_eq!(
            encode_frame(&decoded.frames[0]),
            encode_frame(&frame(u64::from(version)))
        );
    }
}

#[test]
fn raw_fallback_and_lz4_storage_both_preserve_bytes() {
    let mut state = 0x1234_5678u32;
    let mut random = vec![OPTIONAL_TAG_BIT, 0, 16, 0, 0];
    for _ in 0..4096 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        random.push(state as u8);
    }
    let raw = encode_chunk(&random).unwrap();
    assert_eq!(raw[5], 0);
    let compressed = encode_chunk(&batch(1)).unwrap();
    assert_eq!(compressed[5], 1);
    for (encoded, expected) in [(raw, random), (compressed, batch(1))] {
        assert_eq!(
            Chunk::read(&mut Decoder::new(&encoded[5..]))
                .unwrap()
                .expand()
                .unwrap(),
            expected
        );
    }
}

#[test]
fn complete_corrupt_chunks_reject_lengths_flags_checksum_counts_and_trailing_bytes() {
    let valid = encode_chunk(&batch(1)).unwrap();
    for (offset, replacement) in [
        (5, 2u32),
        (6, 1),
        (7, 1),
        (9, u32::MAX),
        (13, u32::MAX),
        (17, 0),
        (17, 4097),
    ] {
        let mut corrupt = valid.clone();
        if offset < 9 {
            corrupt[offset] = replacement as u8;
        } else {
            corrupt[offset..offset + 4].copy_from_slice(&replacement.to_le_bytes());
        }
        assert!(decode(&file(&corrupt)).is_err(), "offset {offset}");
    }
    let mut corrupt = valid.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(matches!(
        decode(&file(&corrupt)).unwrap_err().kind,
        DecodeErrorKind::ChunkChecksum
    ));
    let mut corrupt_metadata = valid.clone();
    corrupt_metadata[17..21].copy_from_slice(&3u32.to_le_bytes());
    assert!(matches!(
        decode(&file(&corrupt_metadata)).unwrap_err().kind,
        DecodeErrorKind::ChunkChecksum
    ));
    let mut trailing = valid.clone();
    trailing.push(0);
    let payload_len = (trailing.len() - 5) as u32;
    trailing[1..5].copy_from_slice(&payload_len.to_le_bytes());
    assert!(decode(&file(&trailing)).is_err());
    let records = batch(1);
    for count in [1, 3] {
        assert!(decode(&file(&raw_chunk(&records, count))).is_err());
    }
    let mut trailing_inner = records.clone();
    trailing_inner.push(0);
    assert!(decode(&file(&raw_chunk(&trailing_inner, 2))).is_err());
    assert!(decode(&file(&raw_chunk(&valid, 1))).is_err());
    assert!(encode_chunk(&valid).is_err());
    assert!(encode_chunk(&[]).is_err());
    assert!(encode_chunk(&vec![0; MAX_CHUNK_BYTES + 1]).is_err());
    assert!(
        decode(&file(&stored_chunk(
            0,
            (records.len() + 1) as u32,
            2,
            &records
        )))
        .is_err()
    );
    let mut hostile_header = encode_chunked_preamble(&header());
    hostile_header[10..14].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode(&hostile_header).is_err());
}

#[test]
fn invalid_lz4_short_output_overflow_and_extra_input_are_errors() {
    let malformed = stored_chunk(1, 100, 1, &[0x00, 0x00, 0x00]);
    assert!(matches!(
        decode(&file(&malformed)).unwrap_err().kind,
        DecodeErrorKind::ChunkDecompression
    ));
    let original = batch(1);
    let valid = encode_chunk(&original).unwrap();
    let stored = &valid[25..];
    for expanded in [original.len() - 1, original.len() + 1, MAX_CHUNK_BYTES + 1] {
        assert!(decode(&file(&stored_chunk(1, expanded as u32, 2, stored))).is_err());
    }
    let mut extra = stored.to_vec();
    extra.push(0);
    assert!(decode(&file(&stored_chunk(1, original.len() as u32, 2, &extra))).is_err());
}

#[test]
fn independent_lz4_terminal_unused_nibble_preserves_exact_output() {
    let stored = [0x56, 0x80, 0x0f, 0, 0, 0, 1, 0, 0x51, 0, 0, 0, 0, 0];
    let mut zero_nibble = stored;
    zero_nibble[8] = 0x50;
    let mut expected = vec![OPTIONAL_TAG_BIT, 15, 0, 0, 0];
    expected.resize(20, 0);
    for block in [stored.as_slice(), zero_nibble.as_slice()] {
        assert_eq!(validate_lz4(block, 20), Ok(()));
        let encoded = stored_chunk(1, 20, 1, block);
        assert_eq!(
            Chunk::read(&mut Decoder::new(&encoded[5..]))
                .unwrap()
                .expand()
                .unwrap(),
            expected
        );
        let decoded = decode(&file(&encoded)).unwrap();
        assert_eq!(decoded.skipped_records, 1);
        assert!(!decoded.truncated);
        let mut trailing = block.to_vec();
        trailing.push(0);
        assert!(validate_lz4(&trailing, 20).is_err());
        assert!(decode(&file(&stored_chunk(1, 20, 1, &trailing))).is_err());
    }
}

#[test]
fn independent_lz4_overlapping_matches_and_permissive_end_conditions_are_bounded() {
    let short_final_literals = [0x5a, 0x80, 0x0f, 0, 0, 0, 1, 0, 0x10, 0];
    let encoded = stored_chunk(1, 20, 1, &short_final_literals);
    let mut expected = vec![OPTIONAL_TAG_BIT, 15, 0, 0, 0];
    expected.resize(20, 0);
    assert_eq!(
        Chunk::read(&mut Decoder::new(&encoded[5..]))
            .unwrap()
            .expand()
            .unwrap(),
        expected
    );
    assert_eq!(decode(&file(&encoded)).unwrap().skipped_records, 1);
    // A distance-one match copies from bytes produced by the same overlapping match.
    let overlap = [0x1a, b'z', 1, 0, 0x50, b'z', b'z', b'z', b'z', b'z'];
    assert_eq!(validate_lz4(&overlap, 20), Ok(()));
    let mut expanded = [0; 20];
    assert_eq!(decompress_into(&overlap, &mut expanded).unwrap(), 20);
    assert_eq!(expanded, [b'z'; 20]);
    for distance in [0, 2] {
        let mut invalid = overlap;
        invalid[2] = distance;
        assert!(validate_lz4(&invalid, 20).is_err());
    }
}

#[test]
fn independent_lz4_literal_and_match_extension_boundaries() {
    for (length, extension) in [(15, vec![0]), (270, vec![255, 0]), (271, vec![255, 1])] {
        let mut block = vec![0xf7];
        block.extend(extension);
        block.resize(block.len() + length, b'q');
        assert_eq!(validate_lz4(&block, length), Ok(()));
        let mut output = vec![0; length];
        assert_eq!(decompress_into(&block, &mut output).unwrap(), length);
        assert_eq!(output, vec![b'q'; length]);
    }
    for (matched, extension) in [(19, vec![0]), (274, vec![255, 0]), (275, vec![255, 1])] {
        let mut block = vec![0x1f, b'r', 1, 0];
        block.extend(extension);
        block.extend([0x50, b'r', b'r', b'r', b'r', b'r']);
        let expected = 1 + matched + 5;
        assert_eq!(validate_lz4(&block, expected), Ok(()));
        let mut output = vec![0; expected];
        assert_eq!(decompress_into(&block, &mut output).unwrap(), expected);
        assert_eq!(output, vec![b'r'; expected]);
    }
}

#[test]
fn independent_lz4_unterminated_extensions_and_truncated_sequences_fail() {
    for block in [
        vec![0xf0],
        vec![0xf0, 255],
        vec![0xf0, 255, 255],
        vec![0x1f, b'a', 1, 0],
        vec![0x1f, b'a', 1, 0, 255],
        vec![0x10],
        vec![0x10, b'a', 1],
        vec![0x10, b'a', 1, 0],
    ] {
        assert!(validate_lz4(&block, 1024).is_err(), "{block:?}");
    }
}

#[test]
fn chunk_and_legacy_aggregate_budgets_reject_before_record_parsing() {
    let mut bytes = file(&encode_chunk(&batch(1)).unwrap());
    bytes.extend(encode_chunk(&batch(2)).unwrap());
    for limits in [
        DecodeLimits {
            max_file_bytes: 1,
            ..DecodeLimits::default()
        },
        DecodeLimits {
            max_expanded_bytes: 1,
            ..DecodeLimits::default()
        },
        DecodeLimits {
            max_projected_bytes: 1,
            ..DecodeLimits::default()
        },
        DecodeLimits {
            max_records: 3,
            ..DecodeLimits::default()
        },
        DecodeLimits {
            max_chunks: 1,
            ..DecodeLimits::default()
        },
    ] {
        assert!(decode_with_limits(&bytes, limits).is_err());
    }
    let mut legacy = encode_input_preamble(&header());
    legacy.extend(batch(1));
    assert!(
        decode_with_limits(
            &legacy,
            DecodeLimits {
                max_records: 1,
                ..DecodeLimits::default()
            }
        )
        .is_err()
    );
    let mut records = Vec::new();
    for _ in 0..=MAX_CHUNK_RECORDS {
        records.extend([OPTIONAL_TAG_BIT, 0, 0, 0, 0]);
    }
    assert!(encode_chunk(&records).is_err());
}

#[test]
fn v3_unknown_required_tags_fail_and_optional_tags_are_counted() {
    assert!(decode(&file(&[4, 0, 0, 0, 0])).is_err());
    let optional = [OPTIONAL_TAG_BIT, 0, 0, 0, 0];
    assert_eq!(decode(&file(&optional)).unwrap().skipped_records, 1);
    assert_eq!(
        decode(&file(&raw_chunk(&optional, 1)))
            .unwrap()
            .skipped_records,
        1
    );
    let mut v2 = encode_input_preamble(&header());
    v2.extend([4, 0, 0, 0, 0]);
    assert_eq!(decode(&v2).unwrap().skipped_records, 1);
}

#[test]
fn chunk_crc32_matches_the_standard_check_value() {
    assert_eq!(checksum(b"1234", b"56789"), 0xcbf4_3926);
}

#[test]
#[ignore = "deterministic synthetic timing harness; not representative real-capture evidence"]
fn synthetic_full_joint_chunk_timing_harness() {
    let started = Instant::now();
    let mut stored = 0usize;
    let mut logical = 0usize;
    for sequence in 0..10_000 {
        let original = batch(sequence);
        let encoded = encode_chunk(&original).unwrap();
        let decoded = decode(&file(&encoded)).unwrap();
        assert_eq!(
            encode_frame(&decoded.frames[0]),
            encode_frame(&frame(sequence))
        );
        stored += encoded.len();
        logical += original.len();
    }
    eprintln!(
        "synthetic deterministic full-joint corpus only: elapsed={:?} stored={stored} logical={logical}",
        started.elapsed()
    );
}

#[test]
fn final_loss_discard_count_is_not_a_chunk_count() {
    let captured = CapturedInput {
        capture_time_ns: 2,
        frame_sequence: None,
        record: InputRecord::Discontinuity {
            reason: DiscontinuityReason::Overflow,
            discarded: 987,
        },
    };
    let decoded = decode(&file(
        &encode_chunk(&encode_input(&captured).unwrap()).unwrap(),
    ))
    .unwrap();
    assert_eq!(decoded.input, [captured]);
}
