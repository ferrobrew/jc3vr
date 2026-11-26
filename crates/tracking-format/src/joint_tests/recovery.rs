use super::*;
use crate::{
    DecodeLimits, MAX_CHUNK_BYTES, MAX_CHUNK_RECORDS, OPTIONAL_TAG_BIT, TAG_CHUNK, TAG_INPUT,
    chunk::Chunk, decode_with_limits, wire::Decoder,
};

#[test]
fn v4_recovers_complete_records_and_chunks_at_every_cut_position() {
    let preamble = encode_latest_preamble(&header());
    for chunked in [false, true] {
        let record = |sequence| {
            let bytes = encode_latest_frame(&frame(sequence));
            if chunked {
                encode_chunk(&bytes).unwrap()
            } else {
                bytes
            }
        };
        let first = record(1);
        let second = record(2);
        let mut bytes = preamble.clone();
        bytes.extend_from_slice(&first);
        bytes.extend_from_slice(&second);
        for end in 0..=bytes.len() {
            if end < preamble.len() {
                assert!(
                    decode(&bytes[..end]).is_err(),
                    "cut {end}, chunked {chunked}"
                );
                continue;
            }
            let decoded = decode(&bytes[..end]).unwrap();
            let count =
                usize::from(end >= preamble.len() + first.len()) + usize::from(end == bytes.len());
            assert_eq!(decoded.frames.len(), count, "cut {end}, chunked {chunked}");
            assert_eq!(
                decoded.truncated,
                end != preamble.len() && end != preamble.len() + first.len() && end != bytes.len(),
                "cut {end}, chunked {chunked}"
            );
            if let Some(first_frame) = decoded.frames.first() {
                assert_eq!(
                    encode_latest_frame(first_frame),
                    encode_latest_frame(&frame(1))
                );
            }
        }
    }
}

#[test]
fn complete_malformed_v4_flags_are_errors_in_direct_and_chunked_records() {
    for (flags, field) in [
        (0x40, "joint reserved flags"),
        (0x80, "joint reserved flags"),
        (1, "joint aggregate flags"),
        (2, "joint aggregate flags"),
        (0x0c, "joint aggregate flags"),
        (0x30, "joint aggregate flags"),
    ] {
        for offset in joint_flag_offsets() {
            let mut record = encode_latest_frame(&frame(1));
            record[offset] = flags;
            for stored in [record.clone(), encode_chunk(&record).unwrap()] {
                let mut bytes = encode_latest_preamble(&header());
                bytes.extend(encode_latest_frame(&frame(0)));
                bytes.extend(stored);
                assert_eq!(
                    decode(&bytes).unwrap_err().kind,
                    DecodeErrorKind::InvalidValue {
                        field,
                        value: u64::from(flags)
                    }
                );
            }
        }
    }
}

#[test]
fn incomplete_standalone_frames_and_complete_short_payloads_are_errors() {
    let record = encode_latest_frame(&frame(1));
    for end in 0..record.len() {
        assert!(decode_frame_with_schema(&record[..end], FrameSchema::V4).is_err());
    }
    for offset in joint_flag_offsets() {
        let mut short = record[..offset].to_vec();
        short[1..5].copy_from_slice(&((offset - 5) as u32).to_le_bytes());
        for stored in [short.clone(), encode_chunk(&short).unwrap()] {
            let mut bytes = encode_latest_preamble(&header());
            bytes.extend(stored);
            assert!(matches!(
                decode(&bytes).unwrap_err().kind,
                DecodeErrorKind::Truncated { .. }
            ));
        }
    }
    let mut trailing = record.clone();
    trailing.push(0);
    assert!(matches!(
        decode_frame_with_schema(&trailing, FrameSchema::V4)
            .unwrap_err()
            .kind,
        DecodeErrorKind::TrailingBytes {
            record: "frame record",
            ..
        }
    ));
    trailing[1..5].copy_from_slice(&((record.len() - 4) as u32).to_le_bytes());
    assert!(matches!(
        decode_frame_with_schema(&trailing, FrameSchema::V4)
            .unwrap_err()
            .kind,
        DecodeErrorKind::TrailingBytes {
            record: "frame",
            ..
        }
    ));
    let mut wrong_tag = record;
    wrong_tag[0] = TAG_INPUT;
    assert!(decode_frame_with_schema(&wrong_tag, FrameSchema::V4).is_err());
}

#[test]
fn v4_uses_the_existing_crc_chunk_layout_and_rejects_complete_corruption() {
    let record = expected_record(&frame(3), FrameSchema::V4);
    let encoded = encode_chunk(&record).unwrap();
    assert_eq!(
        Chunk::read(&mut Decoder::new(&encoded[5..]))
            .unwrap()
            .expand()
            .unwrap(),
        record
    );
    let raw = raw_chunk(&record, 1);
    let mut bytes = encode_latest_chunked_preamble(&header());
    bytes.extend_from_slice(&raw);
    let decoded = decode(&bytes).unwrap();
    assert_eq!(encode_latest_frame(&decoded.frames[0]), record);
    assert_eq!(raw_chunk(&encode_latest_frame(&decoded.frames[0]), 1), raw);
    for chunk in [raw, encoded] {
        for offset in [5, 6, 7, 9, 13, 17, 21, chunk.len() - 1] {
            let mut corrupt = chunk.clone();
            corrupt[offset] ^= 1;
            let mut bytes = encode_latest_preamble(&header());
            bytes.extend(corrupt);
            assert!(decode(&bytes).is_err(), "corrupt byte {offset}");
        }
    }
    let record = encode_latest_frame(&frame(1));
    for count in [0, 2, MAX_CHUNK_RECORDS as u32 + 1] {
        let mut bytes = encode_latest_preamble(&header());
        bytes.extend(raw_chunk(&record, count));
        assert!(decode(&bytes).is_err(), "announced count {count}");
    }
    let mut incomplete_inner = record.clone();
    incomplete_inner.pop();
    let mut bytes = encode_latest_preamble(&header());
    bytes.extend(raw_chunk(&incomplete_inner, 1));
    assert!(decode(&bytes).is_err());
    let mut trailing_inner = record;
    trailing_inner.push(0);
    let mut bytes = encode_latest_preamble(&header());
    bytes.extend(raw_chunk(&trailing_inner, 1));
    assert!(decode(&bytes).is_err());
}

#[test]
fn v4_bounds_and_unknown_required_tags_are_checked_before_tail_recovery() {
    for (tag, length) in [
        (TAG_FRAME, 2 * 1024 * 1024 + 1),
        (TAG_INPUT, crate::input::MAX_INPUT_PAYLOAD + 1),
        (TAG_CHUNK, MAX_CHUNK_BYTES + 21),
        (OPTIONAL_TAG_BIT, MAX_CHUNK_BYTES + 21),
        (99, 0),
    ] {
        let mut bytes = encode_latest_preamble(&header());
        bytes.push(tag);
        bytes.extend_from_slice(&(length as u32).to_le_bytes());
        assert!(matches!(
            decode(&bytes).unwrap_err().kind,
            DecodeErrorKind::InvalidValue { .. }
        ));
    }
    let mut optional = encode_latest_preamble(&header());
    optional.extend_from_slice(&[OPTIONAL_TAG_BIT, 0, 0, 0, 0]);
    assert_eq!(decode(&optional).unwrap().skipped_records, 1);
    for tag in [TAG_CHUNK, 99] {
        assert!(encode_chunk(&[tag, 0, 0, 0, 0]).is_err());
    }
    let mut nested = encode_latest_preamble(&header());
    nested.extend(raw_chunk(&[TAG_CHUNK, 0, 0, 0, 0], 1));
    assert!(decode(&nested).is_err());

    let record = encode_latest_frame(&frame(1));
    let mut raw = encode_latest_preamble(&header());
    raw.extend_from_slice(&record);
    let mut chunked = encode_latest_preamble(&header());
    chunked.extend(encode_chunk(&record).unwrap());
    for bytes in [&raw, &chunked] {
        for limits in [
            DecodeLimits {
                max_file_bytes: bytes.len() as u64 - 1,
                ..DecodeLimits::default()
            },
            DecodeLimits {
                max_expanded_bytes: 0,
                ..DecodeLimits::default()
            },
            DecodeLimits {
                max_projected_bytes: 0,
                ..DecodeLimits::default()
            },
            DecodeLimits {
                max_records: 0,
                ..DecodeLimits::default()
            },
        ] {
            assert!(decode_with_limits(bytes, limits).is_err());
        }
    }
    assert!(
        decode_with_limits(
            &chunked,
            DecodeLimits {
                max_chunks: 0,
                ..DecodeLimits::default()
            }
        )
        .is_err()
    );
    assert!(
        decode_with_limits(
            &raw,
            DecodeLimits {
                max_chunks: 0,
                ..DecodeLimits::default()
            }
        )
        .is_ok()
    );
}

fn raw_chunk(records: &[u8], count: u32) -> Vec<u8> {
    let mut metadata = vec![0, 0, 0, 0];
    metadata.extend_from_slice(&(records.len() as u32).to_le_bytes());
    metadata.extend_from_slice(&(records.len() as u32).to_le_bytes());
    metadata.extend_from_slice(&count.to_le_bytes());
    let mut crc = !0u32;
    for byte in metadata.iter().chain(records) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    let mut bytes = vec![TAG_CHUNK];
    bytes.extend_from_slice(&((20 + records.len()) as u32).to_le_bytes());
    bytes.extend(metadata);
    bytes.extend_from_slice(&(!crc).to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}
