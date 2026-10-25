use glam::{Quat, Vec2, Vec3};
use pretty_assertions::assert_eq;

use super::*;
use crate::{
    controllers::frame::{
        Button, ButtonSet, HandInputs, HandJoint, InteractionProfile, TrackedPose,
    },
    vr::input::semantic::{
        SemanticSet,
        frame::{ActionState, ActionValue},
    },
};

fn header() -> Header {
    let mut transform = [0.0; 16];
    transform[0] = 1.0;
    transform[5] = 1.0;
    transform[10] = 1.0;
    transform[15] = 1.0;
    transform[12] = 1024.5;
    transform[13] = 210.25;
    transform[14] = -3071.0;
    Header {
        build: "2026-10-08 12:00 test".to_owned(),
        recorded_at: "2026-10-08_12-00-00_000".to_owned(),
        action_schema: 2,
        start: StartMarker {
            transform,
            bookmark: Some("the tower ledge".to_owned()),
        },
        dominant_hand: Hand::Left,
        world_scale: 1.25,
        time_scale: 0.5,
    }
}

fn tracked(position: Vec3, velocities: bool) -> TrackedPose {
    TrackedPose {
        pose: Pose {
            position,
            orientation: Quat::from_rotation_y(0.3),
        },
        position_valid: true,
        orientation_valid: true,
        position_tracked: velocities,
        orientation_tracked: true,
        linear_velocity: velocities.then_some(Vec3::new(0.5, -1.0, 2.0)),
        angular_velocity: velocities.then_some(Vec3::new(0.0, 3.0, 0.0)),
    }
}

fn hand(with_joints: bool) -> HandFrame {
    let mut buttons = ButtonSet::EMPTY;
    buttons.set(Button::TriggerPress, true);
    buttons.set(Button::TrackpadPressRight, true);
    buttons.set(Button::AClick, true);
    HandFrame {
        active: true,
        profile: InteractionProfile::ValveIndex,
        grip: tracked(Vec3::new(0.2, 1.1, -0.3), true),
        aim: tracked(Vec3::new(0.21, 1.12, -0.35), false),
        inputs: HandInputs {
            trigger: 0.75,
            trigger_click: false,
            trigger_touch: true,
            squeeze: 0.25,
            squeeze_force: 0.1,
            thumbstick: Vec2::new(-0.5, 1.0),
            thumbstick_click: true,
            thumbstick_touch: true,
            trackpad: Vec2::new(0.4, -0.2),
            trackpad_force: 0.6,
            trackpad_touch: true,
            a_click: true,
            a_touch: true,
            b_click: false,
            b_touch: true,
            system_click: true,
        },
        buttons,
        joints: with_joints.then(|| {
            std::array::from_fn(|i| HandJoint {
                pose: Pose {
                    position: Vec3::splat(i as f32 * 0.01),
                    orientation: Quat::from_rotation_x(i as f32 * 0.05),
                },
                radius: 0.008,
                position_valid: i & 1 != 0,
                orientation_valid: i & 2 != 0,
                position_tracked: i & 4 != 0,
                orientation_tracked: i & 8 != 0,
            })
        }),
    }
}

fn set_state(set: SemanticSet) -> SetState {
    let actions = set
        .actions()
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let value = match i % 3 {
                0 => ActionValue::Bool(i % 2 == 0),
                1 => ActionValue::Float(i as f32 * 0.1),
                _ => ActionValue::Vec2(Vec2::new(0.1, -0.2)),
            };
            [
                ActionState {
                    value,
                    active: true,
                    down: i % 2 == 1,
                },
                ActionState {
                    value,
                    active: false,
                    down: false,
                },
            ]
        })
        .collect();
    SetState { set, actions }
}

fn full_frame() -> Frame {
    Frame {
        time_ns: 11_111_111,
        display_time_ns: 9_876_543_210,
        session: SessionSample {
            state: Some(xr::SessionState::FOCUSED),
            should_render: true,
            head_tracked: true,
        },
        head: Some(HeadSample {
            eyes: [
                EyeSample {
                    pose: Pose {
                        position: Vec3::new(-0.032, 0.05, 0.01),
                        orientation: Quat::from_rotation_y(0.04),
                    },
                    fov: [-0.9, 0.8, 0.85, -0.95],
                },
                EyeSample {
                    pose: Pose {
                        position: Vec3::new(0.032, 0.05, 0.01),
                        orientation: Quat::from_rotation_y(-0.04),
                    },
                    fov: [-0.8, 0.9, 0.85, -0.95],
                },
            ],
        }),
        stage: StageInfo {
            available: true,
            bounds: Some(Vec2::new(3.0, 2.5)),
            head_height: Some(1.62),
            ..StageInfo::default()
        },
        controllers: Some(ControllerSample {
            focused: true,
            action_set: ActionSetKind::Helicopter,
            hands: [hand(true), hand(false)],
        }),
        semantic: Some(SemanticSample {
            action_set: ActionSetKind::Helicopter,
            global: set_state(SemanticSet::Global),
            mode: set_state(SemanticSet::Helicopter),
        }),
        recenters: vec![RecenterEvent {
            kind: RecenterKind::YawAndHeight,
            cause: RecenterCause::SeatTransition,
            turn: Quat::from_rotation_y(-1.2),
        }],
    }
}

fn empty_frame(time_ns: u64) -> Frame {
    Frame {
        time_ns,
        display_time_ns: 0,
        session: SessionSample {
            state: None,
            should_render: false,
            head_tracked: false,
        },
        head: None,
        stage: StageInfo::default(),
        controllers: None,
        semantic: None,
        recenters: Vec::new(),
    }
}

fn file(frames: &[Frame]) -> Vec<u8> {
    let mut bytes = encode_input_preamble(&header());
    for frame in frames {
        bytes.extend(encode_frame(frame));
    }
    bytes
}

#[test]
fn a_recording_reads_back_as_written() {
    let frames = [full_frame(), empty_frame(22_222_222)];
    let recording = decode(&file(&frames)).expect("the file decodes");
    assert_eq!(
        recording,
        Recording {
            header: header(),
            frames: frames.to_vec(),
            input: Vec::new(),
            version: VERSION,
            truncated: false,
            skipped_records: 0,
        }
    );
}

#[test]
fn a_header_without_a_bookmark_reads_back() {
    let mut header = header();
    header.start.bookmark = None;
    let recording = decode(&encode_preamble(&header)).expect("the file decodes");
    assert_eq!(recording.header, header);
    assert!(recording.frames.is_empty());
}

#[test]
fn a_cut_short_last_record_is_left_out_and_marked() {
    let bytes = file(&[full_frame(), full_frame()]);
    for cut in [1, 4, 5, 40] {
        let recording = decode(&bytes[..bytes.len() - cut]).expect("the file decodes");
        assert_eq!(recording.frames.len(), 1, "cut {cut}");
        assert!(recording.truncated, "cut {cut}");
    }
}

#[test]
fn an_unknown_optional_record_is_skipped() {
    let mut bytes = encode_input_preamble(&header());
    bytes.extend([0xFF, 3, 0, 0, 0, 1, 2, 3]);
    bytes.extend(encode_frame(&full_frame()));
    let recording = decode(&bytes).expect("the file decodes");
    assert_eq!(recording.skipped_records, 1);
    assert_eq!(recording.frames, vec![full_frame()]);
}

#[test]
fn another_file_is_refused() {
    let error = decode(b"PNG\r\n\x1a\n\0\0\0\0").expect_err("not a recording");
    assert_eq!(error.kind, DecodeErrorKind::BadMagic);
    let error = decode(b"JC3").expect_err("too short");
    assert_eq!(error.kind, DecodeErrorKind::BadMagic);
}

#[test]
fn a_newer_version_is_refused() {
    let mut bytes = file(&[]);
    bytes[8..10].copy_from_slice(&(VERSION + 1).to_le_bytes());
    let error = decode(&bytes).expect_err("a newer version");
    assert_eq!(error.kind, DecodeErrorKind::UnsupportedVersion(VERSION + 1));
    assert_eq!(error.offset, 8);
}

#[test]
fn an_out_of_range_field_is_an_error_naming_it() {
    let mut frame = empty_frame(5);
    frame.controllers = Some(ControllerSample {
        focused: false,
        action_set: ActionSetKind::OnFoot,
        hands: [hand(false), hand(false)],
    });
    let mut bytes = file(&[frame]);
    // The action set byte follows the record framing (5), the two clocks (16), the frame flags
    // (1), the stage flags (1), and the focus flag (1).
    let at = file(&[]).len() + 5 + 16 + 1 + 1 + 1;
    assert_eq!(bytes[at], ActionSetKind::OnFoot.index() as u8);
    bytes[at] = 200;
    let error = decode(&bytes).expect_err("an invalid action set");
    assert_eq!(
        error.kind,
        DecodeErrorKind::InvalidValue {
            field: "the action set",
            value: 200
        }
    );
    assert_eq!(error.offset, at);
    assert!(error.to_string().starts_with("tracking file: at byte "));
}

#[test]
fn a_record_with_unread_bytes_is_an_error() {
    let mut record = encode_frame(&empty_frame(1));
    let len = u32::from_le_bytes(record[1..5].try_into().unwrap()) + 2;
    record[1..5].copy_from_slice(&len.to_le_bytes());
    record.extend([0, 0]);
    let mut bytes = file(&[]);
    bytes.extend(record);
    let error = decode(&bytes).expect_err("trailing bytes");
    assert_eq!(
        error.kind,
        DecodeErrorKind::TrailingBytes {
            record: "frame",
            extra: 2
        }
    );
}

#[test]
fn versioned_input_wire_survives_payload_conversion() {
    let mut bytes = tracking_format::encode_input_preamble(&convert::header_to_file(&header()));
    let record = tracking_format::input::CapturedInput {
        capture_time_ns: 12,
        frame_sequence: None,
        record: tracking_format::input::InputRecord::Discontinuity {
            reason: tracking_format::input::DiscontinuityReason::LegacyOrderAbsent,
            discarded: 7,
        },
    };
    bytes.extend(tracking_format::encode_input(&record).unwrap());
    let recording = decode(&bytes).unwrap();
    assert_eq!(recording.version, 2);
    assert_eq!(recording.input, vec![record]);
}

#[test]
fn version4_chunk_wire_survives_payload_conversion() {
    let record = tracking_format::input::CapturedInput {
        capture_time_ns: 12,
        frame_sequence: None,
        record: tracking_format::input::InputRecord::Discontinuity {
            reason: tracking_format::input::DiscontinuityReason::Unavailable,
            discarded: 7,
        },
    };
    let mut inner = tracking_format::encode_input(&record).unwrap();
    let encoded_frame = encode_frame(&full_frame());
    inner.extend(&encoded_frame);
    let mut bytes = encode_chunked_preamble(&header());
    bytes.extend(tracking_format::encode_chunk(&inner).unwrap());
    let recording = decode(&bytes).unwrap();
    assert_eq!(recording.version, 4);
    assert_eq!(recording.input, vec![record]);
    assert_eq!(encode_frame(&recording.frames[0]), encoded_frame);
}

#[test]
fn version4_runtime_wire_preserves_all_joint_float_bits_and_input_records() {
    let mut frame = full_frame();
    let controllers = frame.controllers.as_mut().unwrap();
    controllers.hands[1] = hand(true);
    let float_bits = [
        0x8000_0000,
        0x0000_0001,
        0x3f80_0000,
        0x7f80_0000,
        0xff80_0000,
        0x7fc0_0042,
        0xffc0_1234,
        0x7f7f_ffff,
    ];
    for (side, hand) in controllers.hands.iter_mut().enumerate() {
        for (i, joint) in hand.joints.as_mut().unwrap().iter_mut().enumerate() {
            let value = |component: usize| {
                f32::from_bits(float_bits[(i + component + side) % float_bits.len()])
            };
            joint.pose.position = Vec3::new(value(0), value(1), value(2));
            joint.pose.orientation = Quat::from_xyzw(value(3), value(4), value(5), value(6));
            joint.radius = value(7);
        }
    }
    let input = tracking_format::input::CapturedInput {
        capture_time_ns: frame.time_ns,
        frame_sequence: Some(17),
        record: tracking_format::input::InputRecord::Discontinuity {
            reason: tracking_format::input::DiscontinuityReason::Unavailable,
            discarded: 3,
        },
    };
    let mut inner = tracking_format::encode_input(&input).unwrap();
    let encoded_frame = encode_frame(&frame);
    inner.extend(&encoded_frame);
    for chunked in [false, true] {
        let mut bytes = if chunked {
            encode_chunked_preamble(&header())
        } else {
            encode_input_preamble(&header())
        };
        if chunked {
            bytes.extend(tracking_format::encode_chunk(&inner).unwrap());
        } else {
            bytes.extend(&inner);
        }
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.version, 4);
        assert_eq!(decoded.input, vec![input.clone()]);
        assert_eq!(encode_frame(&decoded.frames[0]), encoded_frame);
        let actual = decoded.frames[0].controllers.as_ref().unwrap();
        let expected = frame.controllers.as_ref().unwrap();
        for (actual, expected) in actual.hands.iter().zip(&expected.hands) {
            for (actual, expected) in actual
                .joints
                .as_ref()
                .unwrap()
                .iter()
                .zip(expected.joints.as_ref().unwrap())
            {
                assert_eq!(actual.position_valid, expected.position_valid);
                assert_eq!(actual.orientation_valid, expected.orientation_valid);
                assert_eq!(actual.position_tracked, expected.position_tracked);
                assert_eq!(actual.orientation_tracked, expected.orientation_tracked);
                assert_eq!(
                    actual.pose.position.to_array().map(f32::to_bits),
                    expected.pose.position.to_array().map(f32::to_bits)
                );
                assert_eq!(
                    actual.pose.orientation.to_array().map(f32::to_bits),
                    expected.pose.orientation.to_array().map(f32::to_bits)
                );
                assert_eq!(actual.radius.to_bits(), expected.radius.to_bits());
            }
        }
    }
}

#[test]
fn legacy_runtime_wire_decodes_only_conservative_component_flags() {
    let frame = full_frame();
    let legacy = encode_legacy_frame(&frame);
    let file_header = convert::header_to_file(&header());
    for (version, mut bytes) in [
        (1, tracking_format::encode_preamble(&file_header)),
        (2, tracking_format::encode_input_preamble(&file_header)),
        (3, tracking_format::encode_chunked_preamble(&file_header)),
    ] {
        if version == 3 {
            bytes.extend(tracking_format::encode_chunk(&legacy).unwrap());
        } else {
            bytes.extend(&legacy);
        }
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.version, version);
        assert_eq!(encode_legacy_frame(&decoded.frames[0]), legacy);
        let original = frame.controllers.as_ref().unwrap().hands[0]
            .joints
            .as_ref()
            .unwrap();
        let restored = decoded.frames[0].controllers.as_ref().unwrap().hands[0]
            .joints
            .as_ref()
            .unwrap();
        for (original, restored) in original.iter().zip(restored) {
            assert_eq!(
                restored.position_valid,
                original.position_valid && original.orientation_valid
            );
            assert_eq!(
                restored.orientation_valid,
                original.position_valid && original.orientation_valid
            );
            assert_eq!(restored.position_tracked, original.is_tracked());
            assert_eq!(restored.orientation_tracked, original.is_tracked());
        }
    }
}

#[test]
fn a_long_string_is_cut_at_a_character_boundary() {
    let mut header = header();
    header.build = "é".repeat(40_000);
    let recording = decode(&encode_preamble(&header)).expect("the file decodes");
    assert_eq!(recording.header.build.len(), 65_534);
    assert!(recording.header.build.chars().all(|c| c == 'é'));
}
