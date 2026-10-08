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
                valid: true,
                tracked: i % 2 == 0,
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
    let mut bytes = encode_preamble(&header());
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
fn an_unknown_record_is_skipped() {
    let mut bytes = encode_preamble(&header());
    bytes.extend([0x7F, 3, 0, 0, 0, 1, 2, 3]);
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
fn a_long_string_is_cut_at_a_character_boundary() {
    let mut header = header();
    header.build = "é".repeat(40_000);
    let recording = decode(&encode_preamble(&header)).expect("the file decodes");
    assert_eq!(recording.header.build.len(), 65_534);
    assert!(recording.header.build.chars().all(|c| c == 'é'));
}
