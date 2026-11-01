//! Event records: their round trip, direct and chunked, and their version gates.

use glam::{Vec2, Vec3};

use crate::{
    DecodeErrorKind, Frame, Header, SessionSample, StartMarker, TAG_EVENT, VERSION, decode,
    encode_chunk, encode_event, encode_latest_frame, encode_latest_preamble,
    event::{
        AimState, CharacterMotion, ControllerState, Event, EventKind, GrappleAim, GripAcquisition,
        GripCandidate, GripDrive, GripScan, GripScanEntry, GripStage, GroundHeight, GunRelease,
        HandAim, HandLate, HandTier, IkState, Label, LateState, ReleaseController, ReleaseDrop,
        RoomscaleState, RootTick, TakeMarker, TierState, WeaponAim, WorldMotion,
    },
    model::{Hand, Stage},
    wire::Encoder,
};

#[test]
fn events_round_trip_direct_and_chunked_beside_frames() {
    let events = [
        Event {
            time_ns: 5,
            kind: EventKind::ControllerState(Box::new(controller_state(true))),
        },
        Event {
            time_ns: 6,
            kind: EventKind::ControllerState(Box::new(controller_state(false))),
        },
        Event {
            time_ns: 7,
            kind: EventKind::GripAcquisition(Box::new(grip(true))),
        },
        Event {
            time_ns: 8,
            kind: EventKind::GripAcquisition(Box::new(grip(false))),
        },
        Event {
            time_ns: 9,
            kind: EventKind::GunRelease(Box::new(release(true))),
        },
        Event {
            time_ns: 9,
            kind: EventKind::GunRelease(Box::new(release(false))),
        },
        Event {
            time_ns: 10,
            kind: EventKind::TakeMarker(Box::new(take_marker(true))),
        },
        Event {
            time_ns: 10,
            kind: EventKind::TakeMarker(Box::new(take_marker(false))),
        },
    ];
    let records: Vec<Vec<u8>> = events.iter().map(|e| encode_event(e).unwrap()).collect();
    let frame = frame(10);
    for chunked in [false, true] {
        let mut batch = Vec::new();
        for record in &records {
            batch.extend_from_slice(record);
        }
        batch.extend(encode_latest_frame(&frame));
        let mut bytes = encode_latest_preamble(&header());
        if chunked {
            bytes.extend(encode_chunk(&batch).unwrap());
        } else {
            bytes.extend_from_slice(&batch);
        }
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.version, VERSION);
        assert_eq!(decoded.version, 7);
        assert!(!decoded.truncated);
        assert_eq!(decoded.events, events);
        assert_eq!(decoded.frames, vec![frame.clone()]);
        let again: Vec<Vec<u8>> = decoded
            .events
            .iter()
            .map(|e| encode_event(e).unwrap())
            .collect();
        assert_eq!(again, records);
    }
}

#[test]
fn a_version_5_controller_state_reads_without_the_root_tick() {
    let event = Event {
        time_ns: 4,
        kind: EventKind::ControllerState(Box::new(ControllerState {
            root: None,
            character: None,
            ..controller_state(true)
        })),
    };
    let mut record = encode_event(&event).unwrap();
    // Version 5 ends the controller state at the roomscale state: drop the root tick's and the
    // character motion's absent flags and shorten the record's length to match. The length follows
    // the one-byte tag.
    assert_eq!(record.pop(), Some(0));
    assert_eq!(record.pop(), Some(0));
    let len = u32::from_le_bytes(record[1..5].try_into().unwrap()) - 2;
    record[1..5].copy_from_slice(&len.to_le_bytes());
    let mut bytes = encode_latest_preamble(&header());
    // The version follows the eight-byte magic.
    bytes[8..10].copy_from_slice(&5u16.to_le_bytes());
    bytes.extend_from_slice(&record);
    let decoded = decode(&bytes).unwrap();
    assert_eq!(decoded.version, 5);
    assert_eq!(decoded.events, vec![event]);
}

#[test]
fn a_cut_final_event_is_left_out_and_earlier_events_stay_readable() {
    let first = encode_event(&Event {
        time_ns: 1,
        kind: EventKind::GunRelease(Box::new(release(false))),
    })
    .unwrap();
    let second = encode_event(&Event {
        time_ns: 2,
        kind: EventKind::GripAcquisition(Box::new(grip(true))),
    })
    .unwrap();
    let mut bytes = encode_latest_preamble(&header());
    bytes.extend_from_slice(&first);
    bytes.extend_from_slice(&second[..second.len() - 3]);
    let decoded = decode(&bytes).unwrap();
    assert!(decoded.truncated);
    assert_eq!(decoded.events.len(), 1);
    assert_eq!(decoded.events[0].time_ns, 1);
}

#[test]
fn version_4_containers_reject_event_records_direct_and_chunked() {
    let record = encode_event(&Event {
        time_ns: 1,
        kind: EventKind::GunRelease(Box::new(release(true))),
    })
    .unwrap();
    for chunked in [false, true] {
        let mut bytes = encode_latest_preamble(&header());
        // The version follows the eight-byte magic.
        bytes[8..10].copy_from_slice(&4u16.to_le_bytes());
        if chunked {
            bytes.extend(encode_chunk(&record).unwrap());
        } else {
            bytes.extend_from_slice(&record);
        }
        let error = decode(&bytes).unwrap_err();
        assert!(
            matches!(
                error.kind,
                DecodeErrorKind::InvalidValue {
                    field: "required outer tag" | "chunk inner tag",
                    value
                } if value == u64::from(TAG_EVENT)
            ),
            "{error}"
        );
    }
}

#[test]
fn an_unknown_event_kind_and_unread_event_bytes_are_errors() {
    let mut payload = Encoder::default();
    payload.u64(3);
    payload.u8(0x7f);
    let mut bytes = encode_latest_preamble(&header());
    bytes.push(TAG_EVENT);
    bytes.extend((payload.bytes.len() as u32).to_le_bytes());
    bytes.extend(&payload.bytes);
    let error = decode(&bytes).unwrap_err();
    assert!(matches!(
        error.kind,
        DecodeErrorKind::InvalidValue {
            field: "the event kind",
            value: 0x7f
        }
    ));

    let mut record = encode_event(&Event {
        time_ns: 1,
        kind: EventKind::GunRelease(Box::new(release(false))),
    })
    .unwrap();
    let len = u32::from_le_bytes(record[1..5].try_into().unwrap()) + 1;
    record[1..5].copy_from_slice(&len.to_le_bytes());
    record.push(0);
    let mut bytes = encode_latest_preamble(&header());
    bytes.extend(record);
    let error = decode(&bytes).unwrap_err();
    assert!(matches!(
        error.kind,
        DecodeErrorKind::TrailingBytes {
            record: "event",
            ..
        }
    ));
}

#[test]
fn an_event_over_the_list_bound_is_refused_not_truncated() {
    let mut grip = grip(false);
    grip.candidates = vec![candidate(None); 257];
    let event = Event {
        time_ns: 1,
        kind: EventKind::GripAcquisition(Box::new(grip)),
    };
    assert!(encode_event(&event).is_err());
}

fn header() -> Header {
    Header {
        build: "fixture".into(),
        recorded_at: "fixture".into(),
        action_schema: 1,
        start: StartMarker {
            transform: [0.0; 16],
            bookmark: None,
        },
        dominant_hand: Hand::Right,
        world_scale: 1.0,
        time_scale: 1.0,
    }
}

fn frame(time_ns: u64) -> Frame {
    Frame {
        time_ns,
        display_time_ns: 90,
        session: SessionSample {
            state: None,
            should_render: true,
            head_tracked: false,
        },
        head: None,
        stage: Stage::default(),
        controllers: None,
        semantic: None,
        recenters: Vec::new(),
    }
}

fn some_label(full: bool, text: &'static str) -> Option<Label> {
    full.then_some(Label::Borrowed(text))
}

fn controller_state(full: bool) -> ControllerState {
    let vec = |x: f32| full.then_some(Vec3::new(x, -x, 0.5));
    let ik = |weight| IkState {
        queued: full,
        skip: some_label(!full, "no_target"),
        weight,
        reach_distance_m: 0.4,
        rotation_deg: 12.0,
        secondary_wrist_off: !full,
        secondary_grip_off: full,
        overrode_live_target: false,
    };
    let late = |reason| HandLate {
        reason: Label::Borrowed(reason),
        physical_displacement_m: full.then_some(0.01),
        relative_rotation_deg: full.then_some(3.0),
        requested_wrist: vec(1.0),
        reference_wrist: vec(2.0),
        latest_wrist: vec(3.0),
        achieved_wrist_model: vec(4.0),
    };
    ControllerState {
        frame_sequence: full.then_some(41),
        publication_age_us: full.then_some(900),
        scheme: "controllers".into(),
        mode: "on_foot".into(),
        traversal: some_label(full, "reeling_in"),
        held: some_label(full, "debouncing"),
        input_error: full.then(|| Label::Owned("input: an owned error".into())),
        ik: [ik(0.25), ik(1.0)],
        tiers: TierState {
            state: some_label(full, "blending"),
            hands: [
                HandTier {
                    assigned: "free".into(),
                    effective: "anchored".into(),
                    controller_weight: 0.5,
                    anchor_weight: 0.5,
                },
                HandTier {
                    assigned: "weapon".into(),
                    effective: "weapon".into(),
                    controller_weight: 1.0,
                    anchor_weight: 0.0,
                },
            ],
        },
        late: LateState {
            hands: [late("applied"), late("disabled")],
            weapons_updated: 7,
            palettes_edited: 8,
        },
        aim: AimState {
            idle: some_label(!full, "no_hands"),
            hands: [
                full.then(|| HandAim {
                    source: "gun_sight".into(),
                    gun_index: Some(1),
                    hit_distance_m: Some(30.0),
                    hit_object: true,
                    target: Vec3::new(1.0, 2.0, 3.0),
                }),
                full.then(|| HandAim {
                    source: "aim_pose".into(),
                    gun_index: None,
                    hit_distance_m: None,
                    hit_object: false,
                    target: Vec3::Z,
                }),
            ],
            grapple: full.then_some(GrappleAim {
                hit: true,
                valid: false,
                tetherable: true,
                point: Vec3::ONE,
            }),
            weapon: full.then(|| WeaponAim {
                name_hash: 0xdead_beef,
                gun_hands: [Some(Hand::Left), None, Some(Hand::Right), None],
                sight_source: "fire_point".into(),
                sight_offset_m: Some(Vec3::Y),
                sight_angles_deg: Vec2::new(1.0, -2.0),
            }),
            raycast_error: some_label(full, "invalid_distance"),
        },
        roomscale: RoomscaleState {
            active: full,
            posture: "standing".into(),
            eye_drop_m: 0.1,
            root_owner: some_label(full, "chase"),
            gap_m: 0.2,
            residual_m: 0.3,
            pushback_m: 0.4,
            cockpit_shift_m: Vec3::new(0.1, 0.2, 0.3),
            chase_velocity_mps: Vec2::new(0.5, 0.6),
            task_velocity_mps: Vec2::new(-0.5, -0.6),
            crouched: !full,
            seated_crouch: full,
            seat: "out".into(),
            probe: some_label(full, "obstructed"),
            fade_level: 0.75,
        },
        root: full.then(|| RootTick {
            tick: 1234,
            dt_s: 1.0 / 30.0,
            position: Vec3::new(1741.6, 1082.5, 4596.4),
            moved_m: Vec3::new(0.0, 0.0, -0.11),
            unexplained_m: 0.11,
            task_mps: Vec2::new(0.1, 0.2),
            stick_mps: Vec2::new(0.3, 0.4),
            palms_mps: Vec2::new(0.5, 0.6),
            chase_mps: Vec2::new(0.7, 0.8),
            wanted_mps: Vec3::new(0.7, -0.1, 0.8),
            proxy_mps: Vec3::new(0.6, 0.0, 0.9),
            surface_mps: Vec3::new(0.0, 0.0, 0.25),
            support: "supported".into(),
            surface_dynamic: true,
            ground_body: Some(77),
            proxy_state: 1,
            pending_proxy_state: 2,
            default_proxy_state: -1,
            authored: true,
            capsule_owned: false,
            capsule_blocked: true,
            jumps: 9,
        }),
        character: full.then(|| CharacterMotion {
            velocity_mps: Vec3::new(12.0, -3.5, 0.25),
            height: Some(GroundHeight {
                nearest_m: 4.5,
                ground_m: 6.0,
            }),
        }),
    }
}

fn candidate(lost: Option<&'static str>) -> GripCandidate {
    GripCandidate {
        resolver_index: 3,
        distance_m: 0.12,
        tier: "Grip".into(),
        volume: "Pickup(..)".into(),
        observed: lost.map(|_| Label::Borrowed("Reach { .. }")),
        lost: lost.map(Label::Borrowed),
    }
}

fn grip(full: bool) -> GripAcquisition {
    GripAcquisition {
        hand: if full { Hand::Left } else { Hand::Right },
        stage: if full {
            GripStage::Drive
        } else {
            GripStage::Input
        },
        sequence: 100,
        observation: 101,
        episode: 3,
        request: full.then_some(2),
        grip_down: true,
        grip_pressed: full,
        grip_released: false,
        latch: "Open".into(),
        grab: some_label(full, "Pickup(..)"),
        chosen: full.then(|| candidate(None)),
        candidates: if full {
            vec![candidate(None), candidate(Some("Farther(0.2)"))]
        } else {
            Vec::new()
        },
        candidates_omitted: u32::from(full),
        scan: full.then(|| GripScan {
            entries: vec![
                GripScanEntry {
                    branch: "Ray".into(),
                    object: Some(0x1234_5678),
                    body: Some(9),
                    door: None,
                    point: Some(Vec3::X),
                    distance_m: Some(0.3),
                    mass_kg: Some(12.0),
                    result: "Offered".into(),
                },
                GripScanEntry {
                    branch: "DoorHandle".into(),
                    object: None,
                    body: None,
                    door: Some("(.., FrontLeft, 2)".into()),
                    point: None,
                    distance_m: None,
                    mass_kg: None,
                    result: "OutOfReach".into(),
                },
            ],
            omitted: 1,
            ray_ran: true,
            handles_ran: false,
            ray_selected: some_label(full, "Reach { .. }"),
            handle_selected: None,
            selected: some_label(full, "Reach { .. }"),
        }),
        consumer: "Queued".into(),
        target: some_label(full, "Reach { .. }"),
        result: some_label(full, "BodyAccepted { joined: false }"),
        drive: full.then(|| GripDrive {
            object: 0x1234_5678,
            body: 9,
            before: "BodyState { .. }".into(),
            commanded: Some("Velocities { .. }".into()),
            result: Some("Ok(())".into()),
            report: None,
        }),
    }
}

fn release(full: bool) -> GunRelease {
    let motion = WorldMotion {
        linear: Vec3::new(1.0, 2.0, 3.0),
        angular: Vec3::new(-1.0, 0.0, f32::INFINITY),
    };
    GunRelease {
        hand: Hand::Right,
        attempt: 4,
        weapon: full.then_some(0xabc0),
        route: "Wielded".into(),
        max_speed_mps: 12.0,
        refusal: some_label(!full, "NotHeld"),
        before_disarm: full,
        disarm_returned: full,
        slot_clear_returned: !full,
        reconcile_returned: true,
        controller: full.then_some(ReleaseController {
            frame_sequence: 77,
            display_time_ns: -5,
            publication_age_ns: 1_000_000,
        }),
        drop: full.then(|| ReleaseDrop {
            weapon: 0xabc0,
            slot: Some("Primary".into()),
            sample_stage: "Wielded".into(),
            sample_source: "TimestampedHandHistory".into(),
            cached_motion: Some(motion),
            native_body: Some(5),
            requested_motion: None,
            release_source: "RigTransportOnly".into(),
            history_age_s: Some(0.02),
            write: "SetterReturned".into(),
            on_drop_returned: true,
            slot_clear_returned: false,
        }),
    }
}

fn take_marker(full: bool) -> TakeMarker {
    TakeMarker {
        index: if full { 7 } else { 1 },
        stamp: Label::Borrowed("2026-01-02_03-04-05_006"),
        unix_ms: -5,
        game_run_time_us: full.then_some(123_456),
        label: Label::Borrowed(if full { "door handle" } else { "" }),
    }
}

#[path = "event_tests/moments.rs"]
mod moments;
