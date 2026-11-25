//! The version 7 additions: the event kinds noted as moments, the controller state's character
//! motion, and the follow bank's recenter cause. Each round-trips with every optional field present
//! and absent, and an earlier container reads without them or refuses them.

use glam::{Quat, Vec3};

use super::{V8_STATE_GROUPS, controller_state, frame, header, strip_absent_flags};
use crate::{
    DecodeErrorKind, RecenterEvent, TAG_EVENT, VERSION, decode, encode_chunk, encode_event,
    encode_latest_frame, encode_latest_preamble,
    event::{
        BodyState, ControllerState, Event, EventKind, GripRelease, Label, Snatch, Throw,
        WingsuitGesture, WorldMotion,
    },
    model::{Hand, RecenterCause, RecenterKind},
};

#[test]
fn every_moment_round_trips_direct_and_chunked() {
    let events: Vec<Event> = [true, false]
        .into_iter()
        .flat_map(moments)
        .enumerate()
        .map(|(index, kind)| Event {
            time_ns: index as u64,
            kind,
        })
        .collect();
    assert_round_trip(&events);
}

#[test]
fn the_character_motion_round_trips_with_and_without_its_height() {
    let full = controller_state(true);
    let mut no_height = controller_state(true);
    no_height.character.as_mut().unwrap().height = None;
    let none = ControllerState {
        character: None,
        ..controller_state(true)
    };
    let events: Vec<Event> = [full, no_height, none]
        .into_iter()
        .map(|state| Event {
            time_ns: 3,
            kind: EventKind::ControllerState(Box::new(state)),
        })
        .collect();
    assert_round_trip(&events);
}

#[test]
fn a_version_6_controller_state_reads_without_the_character_motion() {
    let event = Event {
        time_ns: 4,
        kind: EventKind::ControllerState(Box::new(ControllerState {
            character: None,
            ..controller_state(true)
        })),
    };
    let mut record = encode_event(&event).unwrap();
    // Version 6 ends the controller state at the root tick: drop the character motion's and the
    // version 8 groups' absent flags.
    strip_absent_flags(&mut record, 1 + V8_STATE_GROUPS);
    let decoded = decode(&container(6, &record)).unwrap();
    assert_eq!(decoded.version, 6);
    assert_eq!(decoded.events, vec![event]);
}

#[test]
fn earlier_containers_refuse_the_moment_kinds() {
    for kind in moments(true) {
        let name = kind.name();
        let record = encode_event(&Event { time_ns: 1, kind }).unwrap();
        let code = record[1 + 4 + 8];
        for version in [5, 6] {
            let error = decode(&container(version, &record)).unwrap_err();
            assert!(
                matches!(
                    error.kind,
                    DecodeErrorKind::InvalidValue {
                        field: "the event kind",
                        value,
                    } if value == u64::from(code)
                ),
                "{name} in version {version}: {error}"
            );
        }
    }
}

#[test]
fn the_follow_bank_round_trips_as_a_recenter_cause() {
    let mut frame = frame(10);
    frame.recenters = vec![
        RecenterEvent {
            kind: RecenterKind::YawAndHeight,
            cause: RecenterCause::FollowBank,
            turn: Quat::from_rotation_y(-0.25),
        },
        RecenterEvent {
            kind: RecenterKind::Full,
            cause: RecenterCause::Manual,
            turn: Quat::IDENTITY,
        },
    ];
    let mut bytes = encode_latest_preamble(&header());
    bytes.extend(encode_latest_frame(&frame));
    let decoded = decode(&bytes).unwrap();
    assert_eq!(decoded.frames, vec![frame]);
}

/// Every moment kind, with each optional field present (`full`) or absent.
pub(super) fn moments(full: bool) -> Vec<EventKind> {
    let label = |text: &'static str| full.then_some(Label::Borrowed(text));
    let number = |value: f32| full.then_some(value);
    vec![
        EventKind::BodyState(Box::new(BodyState {
            rule_state: full.then_some(0x1234_5678),
            rule_state_name: label("S_JC_WINGSUIT_IDLE"),
            case: Label::Borrowed("wingsuit"),
            repeat: !full,
        })),
        EventKind::Throw(Box::new(Throw {
            hand: if full { Hand::Left } else { Hand::Right },
            item: Label::Borrowed("grenade"),
            motion: WorldMotion {
                linear: Vec3::new(4.0, 2.5, -9.0),
                angular: Vec3::new(0.0, 6.0, 1.0),
            },
            time_scale: if full { 0.25 } else { 1.0 },
            source: label("history"),
            armed: full.then_some(true),
            release: None,
        })),
        EventKind::Throw(Box::new(Throw {
            hand: Hand::Right,
            item: Label::Borrowed("c4"),
            motion: WorldMotion {
                linear: Vec3::ZERO,
                angular: Vec3::ZERO,
            },
            time_scale: 1.0,
            source: None,
            armed: full.then_some(false),
            release: None,
        })),
        EventKind::GripRelease(Box::new(GripRelease {
            hand: Hand::Left,
            held: Label::Borrowed("prop"),
            object: full.then_some(0xdead_beef),
            body: full.then_some(42),
            cause: Label::Borrowed("grip opened"),
            thrown: full,
        })),
        EventKind::WingsuitGesture(Box::new(WingsuitGesture {
            phase: label("armed"),
            why: label("there is no valid arm pose"),
        })),
        EventKind::Snatch(Box::new(Snatch {
            hand: full.then_some(Hand::Right),
            from: Label::Borrowed("ARMED"),
            to: Label::Borrowed("IDLE"),
            outcome: label("cancelled"),
            reason: label("the grip let go"),
            target: label("weapon"),
            distance_m: number(14.5),
            yank_m: number(0.12),
            yank_mps: number(1.8),
        })),
    ]
}

/// Encode `events` beside a frame, direct and chunked, and check that each reads back as written
/// and encodes again to the same bytes.
pub(super) fn assert_round_trip(events: &[Event]) {
    let records: Vec<Vec<u8>> = events.iter().map(|e| encode_event(e).unwrap()).collect();
    let frame = frame(10);
    for chunked in [false, true] {
        let mut batch: Vec<u8> = records.concat();
        batch.extend(encode_latest_frame(&frame));
        let mut bytes = encode_latest_preamble(&header());
        if chunked {
            bytes.extend(encode_chunk(&batch).unwrap());
        } else {
            bytes.extend_from_slice(&batch);
        }
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.version, VERSION);
        assert_eq!(decoded.events, events);
        let again: Vec<Vec<u8>> = decoded
            .events
            .iter()
            .map(|e| encode_event(e).unwrap())
            .collect();
        assert_eq!(again, records);
    }
}

/// A container of `version` holding `record`.
pub(super) fn container(version: u16, record: &[u8]) -> Vec<u8> {
    assert_eq!(record[0], TAG_EVENT);
    let mut bytes = encode_latest_preamble(&header());
    // The version follows the eight-byte magic.
    bytes[8..10].copy_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(record);
    bytes
}
