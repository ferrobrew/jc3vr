//! An export of a synthetic recording carrying every version 8 addition, run end to end into a
//! temporary file.

use std::path::PathBuf;

use glam::{Quat, Vec2, Vec3};
use tracking_format::{
    Recording,
    event::{
        AimState, BODY_BONES, BodiesSample, BodyIntro, BodySample, BoneSet, CameraState,
        ControllerState, Diagnostic, Event, EventKind, Force, ForceKind, FrameState, FrameTiming,
        GrappleState, HandLate, HandTier, Haptic, HeldMotion, HeldSample, HoldReport, IkGoal,
        IkState, IkTarget, Injected, InjectedWrite, LateState, PoseSample, PoseStage, ReleasePose,
        RigConfig, RigInput, RigResult, RigSample, RoomscaleState, Settings, SlingState,
        SlotTiming, Throw, TierState, VehicleState, ViewState, WireEndSample, WorldMotion,
    },
    model::{Hand, Pose},
};

use crate::{
    dump::Range,
    rerun_export::{ExportOptions, export, path_part, registration::Placement, scene, view},
    rig_eval::EvalOptions,
    schema::Schema,
    scripts,
    synth::{self, Options},
};

#[test]
fn a_version_8_recording_exports_with_the_cockpit_placed_by_its_view() {
    let mut recording = synthetic();
    let frames = recording.frames.len();
    add_version_8_events(&mut recording);
    let out = std::env::temp_dir().join(format!(
        "tracking-synth-rerun-export-{}.rrd",
        std::process::id()
    ));
    let options = ExportOptions {
        range: Range::default(),
        video: None,
        eval: EvalOptions::new(1.8, 1.68),
        source: PathBuf::from("synthetic.jc3trk"),
    };
    let summary = export(&recording, None, &options, &out).expect("the export writes");
    let written = std::fs::metadata(&out).map(|m| m.len());
    let _ = std::fs::remove_file(&out);
    assert_eq!(summary.frames, frames);
    assert!(written.expect("the file exists") > 0);
    // The wrists were drawn where the view puts the grips, so against the view's placement they
    // sit on them; a fitted placement would not land them to the millimetre.
    let error = summary.median_wrist_error_m.expect("a wrist error");
    assert!(error < 1.0e-3, "{error}");
    // Every text entry the version 8 events add is counted: one of each of the notes per frame,
    // and the forces.
    assert!(summary.events >= frames * 6, "{summary:?}");
}

#[test]
fn a_recorded_name_stays_one_path_part() {
    assert_eq!(path_part("render.present / hud"), "render_present___hud");
    assert_eq!(path_part("trigger"), "trigger");
    assert_eq!(path_part(""), "_");
}

fn synthetic() -> Recording {
    let script = scripts::named("stand_still").expect("a built-in script");
    let schema = Schema::payload().expect("the schema reads");
    synth::recording(&script, Options::default(), &schema).expect("the script samples")
}

/// A controller state, a pose pair, a rig frame, the bodies, a hold, two forces, and one of each
/// note after every frame, with the view placing the cockpit and the wrists drawn on the grips.
fn add_version_8_events(recording: &mut Recording) {
    let world_head = Vec3::new(1700.0, 1080.0, 4590.0);
    let mut events = Vec::new();
    for (index, frame) in recording.frames.iter().enumerate() {
        let time_ns = frame.time_ns + 1;
        let head = scene::head_pose(frame).expect("a tracked head");
        let view_state = ViewState {
            head_world: Pose {
                position: world_head + Vec3::X * (index as f32 * 0.01),
                orientation: Quat::IDENTITY,
            },
            head_cockpit: head.position,
            view: Quat::from_rotation_y(0.7),
            world_scale: 1.0,
            camera: Some(CameraState {
                previous: at(world_head),
                current: at(world_head + Vec3::X * 0.02),
                fraction: 0.5,
                rendered: at(world_head + Vec3::X * 0.01),
            }),
        };
        let placement = view::placement(&view_state).expect("the view places the cockpit");
        let controllers = frame.controllers.as_ref().expect("the controllers");
        let wrists = controllers
            .hands
            .each_ref()
            .map(|hand| Some(placement.apply(hand.grip.pose.position)));
        let root = placement.translation;
        let mut kinds = vec![
            EventKind::ControllerState(Box::new(controller_state(
                view_state, wrists, &placement, index,
            ))),
            pose(PoseStage::PreIk, root, index),
            pose(PoseStage::Final, root, index),
            rig(),
            EventKind::BodyIntro(Box::new(BodyIntro {
                id: 7,
                object: Some(0x1000),
                kind: "prop".into(),
                mass_kg: Some(12.0),
                shape: "box".into(),
                half_extents: Vec3::splat(0.25),
                filter: 0,
                repeat: index > 0,
            })),
            EventKind::Bodies(Box::new(BodiesSample {
                step: index as u64,
                origin: root,
                walked: 40,
                peak: 90,
                cost_us: 12,
                bodies: vec![
                    BodySample {
                        id: 7,
                        offset: Vec3::new(0.5, 1.0, -0.5),
                        orientation: Quat::IDENTITY,
                        linear: Vec3::X,
                        angular: Vec3::ZERO,
                        resting: false,
                        held: [false, true],
                    },
                    BodySample {
                        id: 8,
                        offset: Vec3::new(-2.0, 0.0, 1.0),
                        orientation: Quat::IDENTITY,
                        linear: Vec3::ZERO,
                        angular: Vec3::ZERO,
                        resting: true,
                        held: [false; 2],
                    },
                ],
            })),
            EventKind::Held(Box::new(HeldSample {
                hand: Hand::Right,
                kind: "prop".into(),
                object: 0x1000,
                body: 7,
                motion: Some(HeldMotion {
                    pose: at(root + Vec3::new(0.5, 1.0, -0.5)),
                    linear: Vec3::X,
                    angular: Vec3::ZERO,
                }),
                report: Some(HoldReport {
                    mass_kg: 12.0,
                    centre: root + Vec3::new(0.5, 1.0, -0.5),
                    anchor: root + Vec3::new(0.45, 1.1, -0.5),
                    target: root + Vec3::new(0.4, 1.15, -0.45),
                    lag_m: 0.08,
                    load: 0.3,
                    twist: 0.1,
                    joined: false,
                    shared: false,
                    liftable: true,
                }),
            })),
            force(ForceKind::Velocity {
                linear: Vec3::new(1.0, 0.5, 0.0),
                angular: Vec3::ZERO,
            }),
            force(ForceKind::Impulse {
                impulse: Vec3::new(0.0, 6.0, 0.0),
                at: root + Vec3::new(0.5, 1.0, -0.5),
            }),
            EventKind::Injected(Box::new(Injected {
                writes: vec![InjectedWrite {
                    map: "player".into(),
                    action: "Fire".into(),
                    value: 1.0,
                    phase: "held".into(),
                    held_s: Some(0.2),
                    outcome: "applied".into(),
                }],
            })),
            EventKind::Diagnostic(Box::new(Diagnostic {
                source: "door".into(),
                hand: Some(Hand::Left),
                what: "tear refused".into(),
                detail: Some("the door is locked".into()),
            })),
            EventKind::Haptic(Box::new(Haptic {
                hand: Hand::Right,
                event: "grab".into(),
                primitive: "tick".into(),
                amplitude: 0.5,
                duration_s: 0.02,
                frequency_hz: None,
            })),
            EventKind::Settings(Box::new(Settings {
                toml: "[comfort]\nvignette = true\n".into(),
            })),
            EventKind::FrameTiming(Box::new(FrameTiming {
                window_s: 1.0,
                frames: 90,
                sim_ticks: 30,
                frame_ms: 11.1,
                frame_max_ms: 14.0,
                slots: vec![SlotTiming {
                    name: "controllers.update".into(),
                    self_ms: 0.4,
                    max_ms: 0.9,
                    calls: 90,
                }],
            })),
            EventKind::RigConfig(Box::new(RigConfig {
                height_m: 1.8,
                eye_height_m: 1.68,
                body_eye_height_m: 1.6,
                body_hip_height_m: 0.95,
                tuning_json: "{}".into(),
                repeat: index > 0,
            })),
        ];
        if index == 10 {
            kinds.push(EventKind::Throw(Box::new(Throw {
                hand: Hand::Right,
                item: "grenade".into(),
                motion: WorldMotion {
                    linear: Vec3::new(0.0, 2.0, -6.0),
                    angular: Vec3::ZERO,
                },
                time_scale: 1.0,
                source: None,
                armed: Some(true),
                release: Some(ReleasePose {
                    position: root + Vec3::new(0.3, 1.4, -0.3),
                    orientation: None,
                }),
            })));
        }
        events.extend(kinds.into_iter().map(|kind| Event { time_ns, kind }));
    }
    recording.events = events;
}

fn at(position: Vec3) -> Pose {
    Pose {
        position,
        orientation: Quat::IDENTITY,
    }
}

fn pose(stage: PoseStage, root: Vec3, index: usize) -> EventKind {
    let bones = (0..BODY_BONES.len())
        .map(|i| Some(at(Vec3::new(0.0, 0.05 * i as f32, 0.0))))
        .collect();
    EventKind::Pose(Box::new(PoseSample {
        pass: index as u64,
        stage,
        character: Pose {
            position: root,
            orientation: Quat::from_rotation_y(0.3),
        },
        set: BoneSet::Body,
        bones,
        targets: match stage {
            PoseStage::PreIk => vec![
                IkTarget {
                    effector: 3,
                    goal: IkGoal::Position(Vec3::new(0.3, 1.2, -0.3)),
                    reach: 1.0,
                    pull: 0.5,
                },
                IkTarget {
                    effector: 3,
                    goal: IkGoal::Rotation {
                        axis: Vec3::Y,
                        angle_rad: 0.2,
                    },
                    reach: 1.0,
                    pull: 0.0,
                },
            ],
            PoseStage::Final => Vec::new(),
        },
    }))
}

fn rig() -> EventKind {
    EventKind::Rig(Box::new(RigSample {
        input: RigInput {
            eyes: Vec3::new(0.0, 1.68, 0.0),
            head: Quat::IDENTITY,
            hands: [None, None],
            pelvis: Vec2::ZERO,
            moving: false,
            sample_height: false,
            dt_s: 1.0 / 90.0,
        },
        output: RigResult {
            neck: Vec3::new(0.0, 1.5, 0.0),
            waist: Vec3::new(0.0, 0.95, 0.0),
            spine_m: 0.55,
            offset: Vec2::ZERO,
            bend: Vec2::ZERO,
            lean_reach_m: 0.2,
            step_demand: Vec2::ZERO,
            crouch_m: 0.0,
            hands_reaching: false,
            phase: "idle".into(),
            gap: Vec2::ZERO,
            body_yaw: 0.0,
            chest_yaw: 0.0,
            yaw_target: 0.0,
            turning: false,
            turn_cause: None,
            standing_neck_m: 1.5,
            steps: 0,
        },
        floor_m: -1.68,
        snapped_to: None,
    }))
}

fn force(kind: ForceKind) -> EventKind {
    EventKind::Force(Box::new(Force {
        source: "payload/src/controllers/hands/grip.rs".into(),
        line: 120,
        kind,
        body: 7,
        object: Some(0x1000),
        mass_kg: Some(12.0),
        before: Some(WorldMotion {
            linear: Vec3::X,
            angular: Vec3::ZERO,
        }),
    }))
}

fn controller_state(
    view: ViewState,
    wrists: [Option<Vec3>; 2],
    placement: &Placement,
    index: usize,
) -> ControllerState {
    let ik = IkState {
        queued: false,
        skip: None,
        weight: 0.0,
        reach_distance_m: 0.0,
        rotation_deg: 0.0,
        secondary_wrist_off: false,
        secondary_grip_off: false,
        overrode_live_target: false,
    };
    let tier = HandTier {
        assigned: "free".into(),
        effective: "free".into(),
        controller_weight: 1.0,
        anchor_weight: 0.0,
    };
    let late = |wrist| HandLate {
        reason: "applied".into(),
        physical_displacement_m: None,
        relative_rotation_deg: None,
        requested_wrist: None,
        reference_wrist: None,
        latest_wrist: wrist,
        achieved_wrist_model: None,
    };
    let device = placement.translation + Vec3::new(0.2, 1.4, 0.0);
    ControllerState {
        frame_sequence: Some(index as u64),
        publication_age_us: Some(800),
        scheme: "controllers".into(),
        mode: "on_foot".into(),
        traversal: None,
        held: None,
        input_error: None,
        ik: [ik.clone(), ik],
        tiers: TierState {
            state: None,
            hands: [tier.clone(), tier],
        },
        late: LateState {
            hands: wrists.map(late),
            weapons_updated: 0,
            palettes_edited: 0,
        },
        aim: AimState {
            idle: None,
            hands: [None, None],
            grapple: None,
            weapon: None,
            raycast_error: None,
        },
        roomscale: RoomscaleState {
            active: true,
            posture: "standing".into(),
            eye_drop_m: 0.0,
            root_owner: None,
            gap_m: 0.0,
            residual_m: 0.0,
            pushback_m: 0.0,
            cockpit_shift_m: Vec3::ZERO,
            chase_velocity_mps: Vec2::ZERO,
            task_velocity_mps: Vec2::ZERO,
            crouched: false,
            seated_crouch: false,
            seat: "out".into(),
            probe: None,
            fade_level: 0.0,
        },
        root: None,
        character: None,
        view: Some(view),
        grapple: Some(GrappleState {
            hook_state: "attached".into(),
            wire_state: "taut".into(),
            max_length_m: 80.0,
            hook: WireEndSample {
                state: "attached".into(),
                position: device + Vec3::new(5.0, 0.0, -10.0),
                attached: true,
            },
            device: WireEndSample {
                state: "held".into(),
                position: device,
                attached: false,
            },
            target_object: Some(0x2000),
            sling: index.is_multiple_of(2).then_some(SlingState {
                length_m: 11.0,
                tension_n: 300.0,
                max_tension_n: 900.0,
                mass_kg: 40.0,
                braced: true,
                tethered: false,
                anchor: device,
                centre: device + Vec3::new(5.0, 0.0, -10.0),
            }),
        }),
        vehicle: (index > 20).then(|| VehicleState {
            class: "car".into(),
            driver: true,
            object: Some(0x3000),
            transform: Some(at(device)),
            rendered: Some(at(device)),
            step_m: Some(Vec3::new(0.0, 0.0, -0.1)),
            steering: Some(Vec3::new(1.0, 0.0, -0.2)),
            handling: true,
            holds: [Some("wheel".into()), None],
            mounted: false,
            mounted_holds: [false; 2],
            mounted_aim: None,
        }),
        frame: Some(FrameState {
            render_frame: Some(index as u32),
            mirror_frame: Some(index as u64),
            interval_ms: Some(11.1),
            ticks: 1,
            haptics: [None, Some(0.2)],
        }),
    }
}
