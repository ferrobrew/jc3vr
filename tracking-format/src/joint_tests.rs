//! Versioned joint flags and exact frame bytes, including non-finite IEEE-754 payloads.

use glam::{Quat, Vec3};

use crate::{
    ControllerSample, DecodeErrorKind, Frame, FrameSchema, Header, SessionSample, StartMarker,
    TAG_FRAME, VERSION, decode, decode_frame_with_schema, encode_chunk, encode_chunked_preamble,
    encode_frame, encode_frame_with_schema, encode_input_preamble, encode_latest_chunked_preamble,
    encode_latest_frame, encode_latest_preamble, encode_legacy_frame, encode_preamble,
    model::{ActionSetKind, HAND_JOINT_COUNT, Hand, HandFrame, HandJoint, Pose, Stage},
};

#[test]
fn all_component_combinations_have_exact_v4_frame_and_chunk_wire() {
    let source = frame(41);
    let expected = expected_record(&source, FrameSchema::V4);
    let record = encode_latest_frame(&source);
    assert_eq!(record, expected);
    assert_eq!(record, encode_frame_with_schema(&source, FrameSchema::V4));
    let decoded = decode_frame_with_schema(&record, FrameSchema::V4).unwrap();
    assert_components(&decoded, &source, false);
    assert_eq!(encode_latest_frame(&decoded), record);

    for chunked in [false, true] {
        let stored = if chunked {
            encode_chunk(&record).unwrap()
        } else {
            record.clone()
        };
        let mut bytes = encode_latest_preamble(&header());
        bytes.extend_from_slice(&stored);
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.version, 4);
        assert!(!decoded.truncated);
        assert_eq!(decoded.frames.len(), 1);
        assert_components(&decoded.frames[0], &source, false);
        assert_eq!(encode_latest_frame(&decoded.frames[0]), expected);
        if chunked {
            assert_eq!(
                encode_chunk(&encode_latest_frame(&decoded.frames[0])).unwrap(),
                stored
            );
        }
    }
}

#[test]
fn all_legacy_versions_down_convert_by_conjunction_and_ignore_upper_joint_bits() {
    let source = frame(7);
    let canonical = expected_record(&source, FrameSchema::Legacy);
    assert_eq!(encode_legacy_frame(&source), canonical);
    assert_eq!(encode_frame(&source), canonical);
    assert_eq!(
        encode_frame_with_schema(&source, FrameSchema::Legacy),
        canonical
    );
    let decoded = decode_frame_with_schema(&canonical, FrameSchema::Legacy).unwrap();
    assert_components(&decoded, &source, true);
    assert_eq!(encode_legacy_frame(&decoded), canonical);

    for version in 1..=3 {
        for upper in 0..64u8 {
            let mut record = canonical.clone();
            for offset in joint_flag_offsets() {
                record[offset] |= upper << 2;
            }
            let mut bytes = match version {
                1 => encode_preamble(&header()),
                2 => encode_input_preamble(&header()),
                3 => encode_chunked_preamble(&header()),
                _ => unreachable!(),
            };
            if version == 3 {
                bytes.extend(encode_chunk(&record).unwrap());
            } else {
                bytes.extend(record);
            }
            let decoded = decode(&bytes).unwrap();
            assert_eq!(decoded.version, version);
            assert_components(&decoded.frames[0], &source, true);
            assert_eq!(encode_legacy_frame(&decoded.frames[0]), canonical);
        }
    }
}

#[test]
fn schema_selection_and_legacy_entry_points_stay_pinned() {
    assert_eq!(VERSION, 4);
    for version in 1..=4 {
        assert_eq!(
            FrameSchema::for_version(version).unwrap(),
            if version == 4 {
                FrameSchema::V4
            } else {
                FrameSchema::Legacy
            }
        );
    }
    for version in [0, 5, u16::MAX] {
        assert_eq!(
            FrameSchema::for_version(version).unwrap_err().kind,
            DecodeErrorKind::UnsupportedVersion(version)
        );
    }
    for (preamble, version) in [
        (encode_preamble(&header()), 1u16),
        (encode_input_preamble(&header()), 2),
        (encode_chunked_preamble(&header()), 3),
        (encode_latest_preamble(&header()), 4),
        (encode_latest_chunked_preamble(&header()), 4),
    ] {
        assert_eq!(&preamble[8..10], &version.to_le_bytes());
    }
    assert_eq!(
        encode_latest_preamble(&header()),
        encode_latest_chunked_preamble(&header())
    );
    assert_ne!(
        encode_legacy_frame(&frame(1)),
        encode_latest_frame(&frame(1))
    );
    let mut v3 = encode_chunked_preamble(&header());
    v3.extend(encode_legacy_frame(&frame(1)));
    assert!(decode(&v3).is_err(), "v3 must still require chunks");
}

#[test]
fn v4_accepts_exactly_the_sixteen_consistent_flag_bytes() {
    let record = encode_latest_frame(&frame(1));
    let offset = joint_flag_offsets()[0];
    let mut accepted = 0;
    for flags in 0..=u8::MAX {
        let mut bytes = record.clone();
        bytes[offset] = flags;
        let result = decode_frame_with_schema(&bytes, FrameSchema::V4);
        let valid = flags & 0xc0 == 0
            && (flags & 1 != 0) == (flags & 0x0c == 0x0c)
            && (flags & 2 != 0) == (flags & 0x30 == 0x30);
        if valid {
            accepted += 1;
            let decoded = result.unwrap();
            let joint = decoded.controllers.as_ref().unwrap().hands[0]
                .joints
                .unwrap()[0];
            assert_eq!(joint.position_valid, flags & 4 != 0);
            assert_eq!(joint.orientation_valid, flags & 8 != 0);
            assert_eq!(joint.position_tracked, flags & 16 != 0);
            assert_eq!(joint.orientation_tracked, flags & 32 != 0);
            assert_eq!(encode_latest_frame(&decoded), bytes);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.offset, offset);
            assert_eq!(
                error.kind,
                DecodeErrorKind::InvalidValue {
                    field: if flags & 0xc0 != 0 {
                        "joint reserved flags"
                    } else {
                        "joint aggregate flags"
                    },
                    value: u64::from(flags),
                }
            );
        }
    }
    assert_eq!(accepted, 16);
}

fn header() -> Header {
    Header {
        build: "joint wire fixture".into(),
        recorded_at: "synthetic".into(),
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

fn frame(sequence: u64) -> Frame {
    let hands = std::array::from_fn(|hand| HandFrame {
        joints: Some(std::array::from_fn(|index| {
            let mask = (index + hand * 7) as u8 % 16;
            let values: [f32; 8] = std::array::from_fn(|component| {
                f32::from_bits(FLOAT_BITS[(index + hand + component) % FLOAT_BITS.len()])
            });
            HandJoint {
                pose: Pose {
                    position: Vec3::new(values[0], values[1], values[2]),
                    orientation: Quat::from_xyzw(values[3], values[4], values[5], values[6]),
                },
                radius: values[7],
                position_valid: mask & 1 != 0,
                orientation_valid: mask & 2 != 0,
                position_tracked: mask & 4 != 0,
                orientation_tracked: mask & 8 != 0,
            }
        })),
        ..HandFrame::default()
    });
    Frame {
        time_ns: sequence,
        display_time_ns: -73,
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
            hands,
        }),
        semantic: None,
        recenters: Vec::new(),
    }
}

const FLOAT_BITS: [u32; 10] = [
    0,
    0x8000_0000,
    1,
    0x007f_ffff,
    0x3eaa_aaab,
    0xbf80_0001,
    0x7f80_0000,
    0xff80_0000,
    0x7fc1_2345,
    0x7fa0_0001,
];

fn assert_components(actual: &Frame, source: &Frame, legacy: bool) {
    let actual = &actual.controllers.as_ref().unwrap().hands;
    let source = &source.controllers.as_ref().unwrap().hands;
    let mut seen = [false; 16];
    for hand in 0..2 {
        for (actual, source) in actual[hand]
            .joints
            .as_ref()
            .unwrap()
            .iter()
            .zip(source[hand].joints.as_ref().unwrap())
        {
            let mask = u8::from(source.position_valid)
                | (u8::from(source.orientation_valid) << 1)
                | (u8::from(source.position_tracked) << 2)
                | (u8::from(source.orientation_tracked) << 3);
            seen[usize::from(mask)] = true;
            assert_eq!(
                actual.position_valid,
                if legacy {
                    source.valid()
                } else {
                    source.position_valid
                }
            );
            assert_eq!(
                actual.orientation_valid,
                if legacy {
                    source.valid()
                } else {
                    source.orientation_valid
                }
            );
            assert_eq!(
                actual.position_tracked,
                if legacy {
                    source.tracked()
                } else {
                    source.position_tracked
                }
            );
            assert_eq!(
                actual.orientation_tracked,
                if legacy {
                    source.tracked()
                } else {
                    source.orientation_tracked
                }
            );
            assert_eq!(actual.valid(), source.valid());
            assert_eq!(actual.tracked(), source.tracked());
            assert_eq!(
                actual.pose.position.to_array().map(f32::to_bits),
                source.pose.position.to_array().map(f32::to_bits)
            );
            assert_eq!(
                actual.pose.orientation.to_array().map(f32::to_bits),
                source.pose.orientation.to_array().map(f32::to_bits)
            );
            assert_eq!(actual.radius.to_bits(), source.radius.to_bits());
        }
    }
    assert!(seen.into_iter().all(|present| present));
}

fn expected_record(frame: &Frame, schema: FrameSchema) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&frame.time_ns.to_le_bytes());
    payload.extend_from_slice(&frame.display_time_ns.to_le_bytes());
    payload.extend_from_slice(&[0x0a, 0, 1, 0]);
    for hand in &frame.controllers.as_ref().unwrap().hands {
        payload.extend_from_slice(&[2, 0]);
        for _ in 0..2 {
            payload.extend_from_slice(&[0]);
            for value in [0.0f32, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
                payload.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        payload.extend_from_slice(&[0; 36]);
        for joint in hand.joints.as_ref().unwrap() {
            for value in joint
                .pose
                .position
                .to_array()
                .into_iter()
                .chain(joint.pose.orientation.to_array())
                .chain([joint.radius])
            {
                payload.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            let mut flags = u8::from(joint.position_valid && joint.orientation_valid)
                | (u8::from(joint.position_tracked && joint.orientation_tracked) << 1);
            if schema == FrameSchema::V4 {
                flags |= (u8::from(joint.position_valid) << 2)
                    | (u8::from(joint.orientation_valid) << 3)
                    | (u8::from(joint.position_tracked) << 4)
                    | (u8::from(joint.orientation_tracked) << 5);
            }
            payload.push(flags);
        }
    }
    payload.push(0);
    let mut record = vec![TAG_FRAME];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.extend(payload);
    record
}

fn joint_flag_offsets() -> Vec<usize> {
    let first_joint = 5 + 16 + 4 + 96;
    let hand_bytes = 96 + HAND_JOINT_COUNT * 33;
    (0..2)
        .flat_map(|hand| {
            (0..HAND_JOINT_COUNT)
                .map(move |joint| first_joint + hand * hand_bytes + joint * 33 + 32)
        })
        .collect()
}

#[path = "joint_tests/input.rs"]
mod input;
#[path = "joint_tests/recovery.rs"]
mod recovery;
