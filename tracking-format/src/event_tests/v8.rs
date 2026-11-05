//! The version 8 additions: the controller state's view, grapple, vehicle, and frame groups, the
//! grip acquisition's target point and the throw's release pose, and the kinds for the body, the
//! world around it, and the diagnostic notes. Each round-trips with its optional fields present and
//! absent, a version 7 container reads the earlier kinds without the appended fields and refuses
//! the new kinds, and the quantised samples keep the precision the format promises.

use glam::{Quat, Vec2, Vec3};

use super::{
    V8_STATE_GROUPS, controller_state, grip, header,
    moments::{assert_round_trip, container, moments},
    strip_absent_flags,
};
use crate::{
    DecodeErrorKind, decode, encode_event, encode_latest_preamble,
    event::{
        BODY_BONE_PARENTS, BODY_BONES, BodiesSample, BodyIntro, BodySample, BoneSet, CameraState,
        ControllerState, Diagnostic, Event, EventKind, Force, ForceKind, FrameState, FrameTiming,
        GrappleState, Haptic, HeldMotion, HeldSample, HoldReport, IkGoal, IkTarget, Injected,
        InjectedWrite, Label, LockState, PoseSample, PoseStage, ReleasePose, RigConfig, RigInput,
        RigResult, RigSample, Settings, SlingState, SlotTiming, Throw, VehicleState, ViewState,
        WireEndSample, WorldMotion,
    },
    model::{Hand, Pose},
    quant::rotation_error_deg,
};

#[test]
fn every_version_8_kind_round_trips_direct_and_chunked() {
    let events: Vec<Event> = [true, false]
        .into_iter()
        .flat_map(world_kinds)
        .map(on_grid)
        .enumerate()
        .map(|(index, kind)| Event {
            time_ns: index as u64,
            kind,
        })
        .collect();
    assert_round_trip(&events);
}

#[test]
fn the_controller_state_groups_round_trip_present_and_absent() {
    let states = [
        with_groups(true),
        with_groups(false),
        controller_state(true),
        ControllerState {
            view: Some(ViewState {
                camera: None,
                ..view()
            }),
            ..controller_state(false)
        },
    ];
    let events: Vec<Event> = states
        .into_iter()
        .map(|state| Event {
            time_ns: 2,
            kind: EventKind::ControllerState(Box::new(state)),
        })
        .collect();
    assert_round_trip(&events);
}

#[test]
fn a_grip_target_point_and_a_throw_release_round_trip() {
    let mut pointed = grip(true);
    pointed.target_point = Some(Vec3::new(1.5, 0.25, -3.0));
    let events: Vec<Event> = [
        EventKind::GripAcquisition(Box::new(pointed)),
        EventKind::Throw(Box::new(throw(Some(ReleasePose {
            position: Vec3::new(4.0, 1.25, -2.0),
            orientation: Some(Quat::from_rotation_y(0.5)),
        })))),
        EventKind::Throw(Box::new(throw(Some(ReleasePose {
            position: Vec3::ONE,
            orientation: None,
        })))),
    ]
    .into_iter()
    .map(|kind| Event { time_ns: 3, kind })
    .collect();
    assert_round_trip(&events);
}

#[test]
fn a_version_7_file_reads_its_kinds_without_the_appended_fields() {
    let cases: Vec<(EventKind, usize)> = vec![
        (
            EventKind::ControllerState(Box::new(controller_state(true))),
            V8_STATE_GROUPS,
        ),
        (EventKind::GripAcquisition(Box::new(grip(true))), 1),
        (EventKind::Throw(Box::new(throw(None))), 1),
    ];
    for (kind, appended) in cases {
        let name = kind.name();
        let event = Event { time_ns: 9, kind };
        let mut record = encode_event(&event).unwrap();
        strip_absent_flags(&mut record, appended);
        let decoded = decode(&container(7, &record)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(decoded.version, 7);
        assert_eq!(decoded.events, vec![event], "{name}");
    }
    // The moments version 7 added carry no appended field but the throw's.
    for kind in moments(true) {
        if matches!(kind, EventKind::Throw(_)) {
            continue;
        }
        let event = Event { time_ns: 1, kind };
        let record = encode_event(&event).unwrap();
        assert_eq!(decode(&container(7, &record)).unwrap().events, vec![event]);
    }
}

#[test]
fn a_version_7_file_carrying_a_version_8_field_is_an_error() {
    let event = Event {
        time_ns: 9,
        kind: EventKind::ControllerState(Box::new(with_groups(true))),
    };
    let record = encode_event(&event).unwrap();
    let error = decode(&container(7, &record)).unwrap_err();
    assert!(
        matches!(error.kind, DecodeErrorKind::TrailingBytes { .. }),
        "{error}"
    );
}

#[test]
fn a_version_7_container_refuses_the_version_8_kinds() {
    for kind in world_kinds(true) {
        let name = kind.name();
        let record = encode_event(&Event { time_ns: 1, kind }).unwrap();
        let code = record[1 + 4 + 8];
        let error = decode(&container(7, &record)).unwrap_err();
        assert!(
            matches!(
                error.kind,
                DecodeErrorKind::InvalidValue {
                    field: "the event kind",
                    value,
                } if value == u64::from(code)
            ),
            "{name}: {error}"
        );
    }
}

#[test]
fn quantised_samples_keep_their_promised_precision() {
    let pose = pose_sample(true);
    let bodies = bodies_sample(true);
    let EventKind::Pose(decoded_pose) = on_grid(EventKind::Pose(Box::new(pose.clone()))) else {
        unreachable!()
    };
    for (written, read) in pose.bones.iter().zip(&decoded_pose.bones) {
        let (written, read) = (written.unwrap(), read.unwrap());
        assert!(written.position.distance(read.position) <= 1.0e-4);
        assert!(rotation_error_deg(written.orientation, read.orientation) < 0.01);
    }
    let EventKind::Bodies(decoded) = on_grid(EventKind::Bodies(Box::new(bodies.clone()))) else {
        unreachable!()
    };
    for (written, read) in bodies.bodies.iter().zip(&decoded.bodies) {
        assert!(written.offset.distance(read.offset) <= 1.0e-3);
        assert!(written.linear.distance(read.linear) <= 1.0e-2);
        assert!(written.angular.distance(read.angular) <= 1.0e-2);
        assert!(rotation_error_deg(written.orientation, read.orientation) < 0.01);
    }
}

#[test]
fn a_bone_beyond_the_encodings_range_reads_as_absent() {
    let mut pose = pose_sample(true);
    pose.bones[3] = Some(Pose {
        position: Vec3::new(0.0, 4.0, 0.0),
        orientation: Quat::IDENTITY,
    });
    let EventKind::Pose(decoded) = on_grid(EventKind::Pose(Box::new(pose))) else {
        unreachable!()
    };
    assert_eq!(decoded.bones[3], None);
    assert!(decoded.bones[4].is_some());
}

#[test]
fn a_pose_with_the_wrong_bone_count_is_refused() {
    let mut pose = pose_sample(true);
    pose.bones.pop();
    let event = Event {
        time_ns: 1,
        kind: EventKind::Pose(Box::new(pose)),
    };
    assert!(encode_event(&event).is_err());
}

#[test]
fn the_body_bone_list_names_each_bone_once_and_parents_each_before_it() {
    for (index, name) in BODY_BONES.iter().enumerate() {
        assert_eq!(
            BODY_BONES.iter().position(|other| other == name),
            Some(index)
        );
        if let Some(parent) = BODY_BONE_PARENTS[index] {
            assert!(parent < index, "{name}");
        }
    }
    assert_eq!(BODY_BONE_PARENTS[0], None);
}

#[test]
fn settings_longer_than_a_label_round_trip() {
    let toml: String = "[section]\nkey = 1\n".repeat(8000);
    assert!(toml.len() > usize::from(u16::MAX));
    let event = Event {
        time_ns: 1,
        kind: EventKind::Settings(Box::new(Settings {
            toml: Label::Owned(toml),
        })),
    };
    let mut bytes = encode_latest_preamble(&header());
    bytes.extend(encode_event(&event).unwrap());
    assert_eq!(decode(&bytes).unwrap().events, vec![event]);
}

/// Encode and decode one event, so a quantised sample sits on its grid.
fn on_grid(kind: EventKind) -> EventKind {
    let record = encode_event(&Event { time_ns: 0, kind }).unwrap();
    let mut bytes = encode_latest_preamble(&header());
    bytes.extend(record);
    decode(&bytes).unwrap().events.remove(0).kind
}

fn with_groups(full: bool) -> ControllerState {
    ControllerState {
        view: Some(view()),
        grapple: Some(grapple(full)),
        vehicle: Some(vehicle(full)),
        frame: Some(FrameState {
            render_frame: full.then_some(12_345),
            mirror_frame: full.then_some(678),
            interval_ms: full.then_some(11.1),
            ticks: u32::from(full),
            haptics: [full.then_some(0.5), None],
        }),
        ..controller_state(full)
    }
}

fn pose(x: f32) -> Pose {
    Pose {
        position: Vec3::new(x, 1.5, -x),
        orientation: Quat::from_rotation_y(x),
    }
}

fn view() -> ViewState {
    ViewState {
        head_world: pose(100.0),
        head_cockpit: Vec3::new(0.1, 1.7, 0.2),
        view: Quat::from_rotation_y(1.25),
        world_scale: 1.1,
        camera: Some(CameraState {
            previous: pose(99.0),
            current: pose(100.5),
            fraction: 0.4,
            rendered: pose(99.6),
        }),
    }
}

fn grapple(full: bool) -> GrappleState {
    GrappleState {
        hook_state: "GHS_CUSTOM_ACTIVE_WIRE".into(),
        wire_state: "GHWS_ATTACHED".into(),
        max_length_m: 80.0,
        hook: WireEndSample {
            state: "GWES_ATTACHED".into(),
            position: Vec3::new(10.0, 2.0, -30.0),
            attached: true,
        },
        device: WireEndSample {
            state: "GWES_ATTACHED".into(),
            position: Vec3::new(0.0, 1.2, 0.0),
            attached: full,
        },
        target_object: full.then_some(0xfeed),
        sling: full.then_some(SlingState {
            length_m: 6.0,
            tension_n: 900.0,
            max_tension_n: 2500.0,
            mass_kg: 80.0,
            braced: true,
            tethered: false,
            anchor: Vec3::new(0.2, 1.4, -0.3),
            centre: Vec3::new(4.0, 1.0, -5.0),
        }),
        lock: full.then_some(LockState {
            braking: false,
            length_m: 9.5,
            stretch_m: -0.25,
            tension_n: 1200.0,
            max_tension_n: 3924.0,
            target_mass_kg: Some(1400.0),
            hook: Vec3::new(10.0, 2.0, -30.0),
            rider: Vec3::new(4.0, -4.0, -25.0),
        }),
    }
}

fn vehicle(full: bool) -> VehicleState {
    VehicleState {
        class: "Car".into(),
        driver: full,
        object: full.then_some(0xcafe),
        transform: full.then(|| pose(3.0)),
        rendered: full.then(|| pose(2.9)),
        step_m: full.then_some(Vec3::new(0.0, 0.0, 0.5)),
        steering: full.then_some(Vec3::new(1.0, 0.0, -0.25)),
        handling: full,
        holds: [full.then_some("Wheel".into()), None],
        mounted: !full,
        mounted_holds: [!full, false],
        mounted_aim: (!full).then_some(Vec3::new(5.0, 2.0, -40.0)),
    }
}

fn throw(release: Option<ReleasePose>) -> Throw {
    Throw {
        hand: Hand::Right,
        item: "prop".into(),
        motion: WorldMotion {
            linear: Vec3::new(2.0, 3.0, -6.0),
            angular: Vec3::ZERO,
        },
        time_scale: 1.0,
        source: None,
        armed: None,
        release,
    }
}

fn pose_sample(full: bool) -> PoseSample {
    PoseSample {
        pass: 77,
        stage: if full {
            PoseStage::PreIk
        } else {
            PoseStage::Final
        },
        character: pose(50.0),
        set: BoneSet::Body,
        bones: (0..BODY_BONES.len())
            .map(|index| {
                (full || index % 3 != 0).then(|| Pose {
                    position: Vec3::new(0.1 * index as f32 - 1.0, 0.07 * index as f32, -0.2),
                    orientation: Quat::from_euler(
                        glam::EulerRot::YXZ,
                        0.1 * index as f32,
                        -0.05 * index as f32,
                        0.3,
                    ),
                })
            })
            .collect(),
        targets: if full {
            vec![
                IkTarget {
                    effector: 15,
                    goal: IkGoal::Position(Vec3::new(0.0, 1.6, 0.1)),
                    reach: 1.0,
                    pull: 0.25,
                },
                IkTarget {
                    effector: 1,
                    goal: IkGoal::Rotation {
                        axis: Vec3::Y,
                        angle_rad: 0.0,
                    },
                    reach: 0.5,
                    pull: 0.0,
                },
            ]
        } else {
            Vec::new()
        },
    }
}

fn bodies_sample(full: bool) -> BodiesSample {
    BodiesSample {
        step: 9001,
        origin: Vec3::new(1000.0, 200.0, -3000.0),
        walked: 512,
        peak: 4096,
        cost_us: 37,
        bodies: if full {
            vec![
                BodySample {
                    id: 1234,
                    offset: Vec3::new(1.2345, 0.5, -7.25),
                    orientation: Quat::from_rotation_x(0.7),
                    linear: Vec3::new(3.21, -9.81, 0.0),
                    angular: Vec3::new(0.0, 12.34, -1.0),
                    resting: false,
                    held: [true, false],
                },
                BodySample {
                    id: 99,
                    offset: Vec3::new(-2.0, 0.0, 2.0),
                    orientation: Quat::IDENTITY,
                    linear: Vec3::ZERO,
                    angular: Vec3::ZERO,
                    resting: true,
                    held: [false, false],
                },
            ]
        } else {
            Vec::new()
        },
    }
}

/// Every version 8 kind, with each optional field present (`full`) or absent.
fn world_kinds(full: bool) -> Vec<EventKind> {
    let label = |text: &'static str| full.then_some(Label::Borrowed(text));
    let number = |value: f32| full.then_some(value);
    vec![
        EventKind::Pose(Box::new(pose_sample(full))),
        EventKind::Rig(Box::new(RigSample {
            input: RigInput {
                eyes: Vec3::new(0.0, 1.68, 0.0),
                head: Quat::from_rotation_y(0.3),
                hands: [full.then_some(Vec3::new(-0.3, 1.1, -0.3)), None],
                pelvis: Vec2::new(0.0, 0.1),
                moving: full,
                sample_height: !full,
                dt_s: 0.011,
            },
            output: RigResult {
                neck: Vec3::new(0.0, 1.55, 0.05),
                waist: Vec3::new(0.0, 1.0, 0.08),
                spine_m: 0.55,
                offset: Vec2::new(0.01, -0.02),
                bend: Vec2::new(0.1, -0.05),
                lean_reach_m: 0.2,
                step_demand: Vec2::new(0.0, 0.03),
                crouch_m: 0.0,
                hands_reaching: full,
                phase: "stepping".into(),
                gap: Vec2::new(0.02, 0.0),
                body_yaw: 1.0,
                chest_yaw: 1.1,
                yaw_target: 1.2,
                turning: !full,
                turn_cause: label("sustained"),
                standing_neck_m: 1.56,
                steps: 4,
            },
            floor_m: -1.7,
            snapped_to: full.then_some(Vec2::new(0.5, -0.5)),
        })),
        EventKind::RigConfig(Box::new(RigConfig {
            height_m: 1.8,
            eye_height_m: 1.68,
            body_eye_height_m: 1.68,
            body_hip_height_m: 0.95,
            tuning_json: "{\"pelvis\":{}}".into(),
            repeat: !full,
        })),
        EventKind::Bodies(Box::new(bodies_sample(full))),
        EventKind::BodyIntro(Box::new(BodyIntro {
            id: 1234,
            object: full.then_some(0xabcd),
            kind: "prop".into(),
            mass_kg: number(25.0),
            shape: "convex polytope".into(),
            half_extents: Vec3::new(0.3, 0.4, 0.2),
            filter: 0x0102_0304,
            repeat: full,
        })),
        EventKind::Held(Box::new(HeldSample {
            hand: Hand::Left,
            kind: "prop".into(),
            object: 0xabcd,
            body: 1234,
            motion: full.then(|| HeldMotion {
                pose: pose(4.0),
                linear: Vec3::new(1.0, 2.0, 3.0),
                angular: Vec3::new(-1.0, 0.0, 0.5),
            }),
            report: full.then_some(HoldReport {
                mass_kg: 25.0,
                centre: Vec3::new(1.0, 1.0, 1.0),
                anchor: Vec3::new(1.1, 1.0, 1.0),
                target: Vec3::new(1.15, 1.02, 1.0),
                lag_m: 0.05,
                load: 0.6,
                twist: 0.2,
                joined: false,
                shared: true,
                liftable: true,
            }),
        })),
        EventKind::Force(Box::new(Force {
            source: "payload/src/controllers/physical_hands/touch.rs".into(),
            line: 152,
            kind: if full {
                ForceKind::Impulse {
                    impulse: Vec3::new(10.0, 0.0, -4.0),
                    at: Vec3::new(1.0, 1.0, 1.0),
                }
            } else {
                ForceKind::Velocity {
                    linear: Vec3::new(0.0, 5.0, 0.0),
                    angular: Vec3::ZERO,
                }
            },
            body: 1234,
            object: full.then_some(0xabcd),
            mass_kg: number(25.0),
            before: full.then_some(WorldMotion {
                linear: Vec3::new(0.5, 0.0, 0.0),
                angular: Vec3::ZERO,
            }),
        })),
        EventKind::Injected(Box::new(Injected {
            writes: if full {
                vec![InjectedWrite {
                    map: "player".into(),
                    action: "FIRE_GRAPPLE".into(),
                    value: 1.0,
                    phase: "held".into(),
                    held_s: Some(0.4),
                    outcome: "applied".into(),
                }]
            } else {
                Vec::new()
            },
        })),
        EventKind::Diagnostic(Box::new(Diagnostic {
            source: "trigger".into(),
            hand: full.then_some(Hand::Right),
            what: "the press fired nothing".into(),
            detail: label("the gun is empty"),
        })),
        EventKind::Haptic(Box::new(Haptic {
            hand: Hand::Left,
            event: "grab".into(),
            primitive: "click".into(),
            amplitude: 0.7,
            duration_s: 0.02,
            frequency_hz: number(160.0),
        })),
        EventKind::Settings(Box::new(Settings {
            toml: "[vr]\nworld_scale = 1.0\n".into(),
        })),
        EventKind::FrameTiming(Box::new(FrameTiming {
            window_s: 1.0,
            frames: 90,
            sim_ticks: 30,
            frame_ms: 11.1,
            frame_max_ms: 14.0,
            slots: if full {
                vec![SlotTiming {
                    name: "character_pose".into(),
                    self_ms: 0.3,
                    max_ms: 0.9,
                    calls: 60,
                }]
            } else {
                Vec::new()
            },
        })),
    ]
}
