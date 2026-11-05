//! The evaluation's sources: the simulated pelvis is unchanged for a recording without version 8
//! events, the recorded inputs reproduce the rig that produced them, the recorded calibration and
//! tuning are taken up, and the pre-IK hips land where the view puts them.

use std::borrow::Cow;

use body_rig::{GroundedRig, RigFrame, RigOutput, RigTuning};
use glam::{Quat, Vec2, Vec3};
use pretty_assertions::assert_eq;
use tracking_format::{
    Recording,
    event::{Event, EventKind, RigConfig, RigInput, RigResult, RigSample, ViewState},
    model::Pose,
};

use super::*;
use crate::{
    rig_eval::{
        frames::{FrameRun, PelvisDrive, run_frames},
        pre_ik::{PreIkTrack, hips_in_cockpit},
    },
    schema::Schema,
    scripts,
    synth::{self, Options},
};

const DT: f32 = 1.0 / 90.0;
const DT_NS: u64 = 11_111_111;

fn synthetic(name: &str) -> Recording {
    let script = scripts::named(name).expect("a built-in script");
    let schema = Schema::payload().expect("the schema reads");
    synth::recording(&script, Options::default(), &schema).expect("the script samples")
}

/// A recording holding only `events`, with a synthetic recording's header.
fn with_events(events: Vec<Event>) -> Recording {
    let mut recording = synthetic("stand_still");
    recording.frames.clear();
    recording.events = events;
    recording.version = 8;
    recording
}

fn options() -> EvalOptions {
    EvalOptions::new(1.8, 1.68)
}

fn replay(source: PelvisSource) -> ReplayOptions {
    ReplayOptions {
        eval: options(),
        source: Some(source),
        recorded_body: true,
        recorded_tuning: true,
    }
}

/// A player who walks forward half a metre, turns their head well past the deadzone, and leans.
fn rig_frames() -> Vec<RigFrame> {
    let standing = options().body.eye_height;
    (0..360)
        .map(|i| {
            let t = i as f32 * DT;
            RigFrame {
                eyes: Vec3::new(0.05 * (t * 2.0).sin(), standing, -(t * 0.4).min(0.5)),
                head: Quat::from_rotation_y((t * 0.6).min(2.0)),
                hands: [Some(Vec3::new(-0.3, 1.0, -0.3)), None],
                pelvis: Vec2::new(0.0, -(t * 0.3).min(0.4)),
                moving: i > 200,
                sample_height: true,
                dt: DT,
            }
        })
        .collect()
}

/// The rig's outputs over `frames`, snapping on `snap`, as the payload would record them.
fn run_reference(
    body: PlayerBody,
    tuning: &RigTuning,
    frames: &[RigFrame],
    snap: usize,
) -> Vec<RigOutput> {
    let mut rig = GroundedRig::new(body);
    frames
        .iter()
        .enumerate()
        .map(|(i, frame)| {
            if i == snap {
                rig.recenter(frame.eyes, frame.head);
            }
            rig.update(frame, tuning)
        })
        .collect()
}

fn rig_event(time_ns: u64, frame: &RigFrame, output: &RigOutput, snapped: bool) -> Event {
    let solve = &output.solve;
    Event {
        time_ns,
        kind: EventKind::Rig(Box::new(RigSample {
            input: RigInput {
                eyes: frame.eyes,
                head: frame.head,
                hands: frame.hands,
                pelvis: frame.pelvis,
                moving: frame.moving,
                sample_height: frame.sample_height,
                dt_s: frame.dt,
            },
            output: RigResult {
                neck: solve.neck,
                waist: solve.waist,
                spine_m: solve.spine_m,
                offset: solve.offset,
                bend: Vec2::new(solve.bend.pitch, solve.bend.roll),
                lean_reach_m: solve.lean_reach_m,
                step_demand: solve.step_demand,
                crouch_m: solve.crouch_m,
                hands_reaching: solve.hands_reaching,
                phase: Cow::Borrowed(output.phase.label()),
                gap: output.gap,
                body_yaw: output.body_yaw,
                chest_yaw: output.chest_yaw,
                yaw_target: output.yaw_target,
                turning: output.turning,
                turn_cause: output.turn_cause.map(|cause| Cow::Borrowed(cause.label())),
                standing_neck_m: output.standing_neck_m,
                steps: output.steps,
            },
            floor_m: 0.0,
            snapped_to: snapped.then_some(Vec2::new(frame.eyes.x, frame.eyes.z)),
        })),
    }
}

fn config_event(time_ns: u64, body: PlayerBody, tuning_json: String) -> Event {
    Event {
        time_ns,
        kind: EventKind::RigConfig(Box::new(RigConfig {
            height_m: 1.7,
            eye_height_m: 1.6,
            body_eye_height_m: body.eye_height,
            body_hip_height_m: body.hip_height,
            tuning_json: Cow::Owned(tuning_json),
            repeat: false,
        })),
    }
}

fn rig_events(frames: &[RigFrame], outputs: &[RigOutput], snap: usize) -> Vec<Event> {
    frames
        .iter()
        .zip(outputs)
        .enumerate()
        .map(|(i, (frame, output))| rig_event(DT_NS * (i as u64 + 1), frame, output, i == snap))
        .collect()
}

#[test]
fn a_recording_without_version_8_events_evaluates_as_before() {
    for name in ["stand_still", "stick_walk", "stick_strafe_and_turn"] {
        let recording = synthetic(name);
        let (rows, summary) = evaluate(&recording, &options(), None);
        assert!(!rows.is_empty(), "{name}");
        let evaluation = evaluate_replay(
            &recording,
            &ReplayOptions {
                source: None,
                ..replay(PelvisSource::Simulated)
            },
            None,
        )
        .expect("the simulated source always runs");
        assert_eq!(evaluation.source, PelvisSource::Simulated, "{name}");
        let replayed: Vec<EvalRow> = evaluation.rows.iter().map(|row| row.row).collect();
        assert_eq!(replayed, rows, "{name}");
        assert_eq!(evaluation.summary, summary, "{name}");
        assert!(evaluation.rows.iter().all(|row| row.recorded.is_none()));
        assert_eq!(evaluation.comparison, None);
        assert_eq!(evaluation.config, None);
    }
}

#[test]
fn the_version_8_sources_refuse_a_recording_without_their_events() {
    let recording = synthetic("stand_still");
    for source in [PelvisSource::Recorded, PelvisSource::PreIk] {
        let error = evaluate_replay(&recording, &replay(source), None).unwrap_err();
        assert!(error.to_string().starts_with("rig-eval: "), "{error}");
    }
}

#[test]
fn the_recorded_inputs_reproduce_the_rig_that_ran_them() {
    let frames = rig_frames();
    let tuning = RigTuning::new();
    let outputs = run_reference(options().body, &tuning, &frames, 120);
    assert!(outputs.iter().any(|output| output.turning));
    let recording = with_events(rig_events(&frames, &outputs, 120));
    assert_eq!(
        PelvisSource::default_for(&recording),
        PelvisSource::Recorded
    );
    let evaluation = evaluate_replay(&recording, &replay(PelvisSource::Recorded), None).unwrap();
    assert_eq!(evaluation.source, PelvisSource::Recorded);
    let replayed: Vec<RigOutput> = evaluation.rows.iter().map(|row| row.row.output).collect();
    assert_eq!(replayed, outputs);
    let comparison = evaluation
        .comparison
        .expect("every row has a recorded frame");
    assert_eq!(comparison.rows, frames.len() as u64);
    assert_eq!(comparison.max_yaw_diff_deg, 0.0);
    assert_eq!(comparison.phase_agreement, 1.0);
    assert!(comparison.mean_offset_diff_m < 1e-6);
}

#[test]
fn a_changed_tuning_on_the_recorded_inputs_shows_in_the_comparison() {
    let frames = rig_frames();
    let outputs = run_reference(options().body, &RigTuning::new(), &frames, usize::MAX);
    let recording = with_events(rig_events(&frames, &outputs, usize::MAX));
    let mut candidate = options();
    candidate.tuning.yaw.turn_deg *= 0.25;
    let evaluation = evaluate_replay(
        &recording,
        &ReplayOptions {
            eval: candidate,
            ..replay(PelvisSource::Recorded)
        },
        None,
    )
    .unwrap();
    let comparison = evaluation.comparison.unwrap();
    assert!(comparison.max_yaw_diff_deg > 1.0, "{comparison:?}");
}

#[test]
fn the_recorded_calibration_and_tuning_are_taken_up() {
    let frames = rig_frames();
    let body = PlayerBody {
        eye_height: 1.55,
        hip_height: 0.85,
    };
    let mut tuning = RigTuning::new();
    tuning.pelvis.lean_radius_m = 0.1;
    tuning.yaw.turn_deg = 20.0;
    let outputs = run_reference(body, &tuning, &frames, usize::MAX);
    let mut events = vec![
        config_event(0, body, serde_json::to_string(&tuning).unwrap()),
        config_event(
            1,
            body,
            "{\"error\":\"a serialization failure\"}".to_owned(),
        ),
    ];
    events.extend(rig_events(&frames, &outputs, usize::MAX));
    let recording = with_events(events);

    let evaluation = evaluate_replay(&recording, &replay(PelvisSource::Recorded), None).unwrap();
    let config = evaluation.config.expect("the note reads");
    assert_eq!(config.notes, 1);
    assert_eq!(config.first.body, body);
    assert_eq!(config.first.tuning, tuning);
    assert!(config.body && config.tuning);
    assert_eq!(evaluation.config_errors.len(), 1);
    assert!(matches!(
        evaluation.config_errors[0].kind,
        ConfigErrorKind::Unserialized(_)
    ));
    let replayed: Vec<RigOutput> = evaluation.rows.iter().map(|row| row.row.output).collect();
    assert_eq!(replayed, outputs);

    // The command line's tuning overrides the recorded one.
    let overridden = evaluate_replay(
        &recording,
        &ReplayOptions {
            recorded_tuning: false,
            ..replay(PelvisSource::Recorded)
        },
        None,
    )
    .unwrap();
    assert!(!overridden.config.unwrap().tuning);
    assert!(overridden.comparison.unwrap().max_yaw_diff_deg > 1.0);
}

#[test]
fn the_pre_ik_hips_land_where_the_view_puts_them() {
    // The view turns the cockpit a quarter turn left into the world at twice the scale, so a
    // point a metre ahead of the head in the cockpit is two metres along the world's -X from it.
    let view = ViewState {
        head_world: Pose {
            position: Vec3::new(10.0, 0.0, 20.0),
            orientation: Quat::IDENTITY,
        },
        head_cockpit: Vec3::new(0.0, 1.5, 0.0),
        view: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        world_scale: 2.0,
        camera: None,
    };
    // A character facing back along +Z whose hips sit a metre to its right, at the world point
    // (8, 0, 20).
    let character = Pose {
        position: Vec3::new(9.0, -1.0, 20.0),
        orientation: Quat::from_rotation_y(std::f32::consts::PI),
    };
    let hips = Pose {
        position: Vec3::new(1.0, 1.0, 0.0),
        orientation: Quat::IDENTITY,
    };
    let cockpit = hips_in_cockpit(&character, &hips, &view).unwrap();
    assert!(
        (cockpit - Vec3::new(0.0, 1.5, -1.0)).length() < 1e-5,
        "{cockpit:?}"
    );
    let degenerate = ViewState {
        world_scale: 0.0,
        ..view
    };
    assert_eq!(hips_in_cockpit(&character, &hips, &degenerate), None);
}

#[test]
fn the_pre_ik_source_runs_the_rig_on_the_game_hips() {
    let recording = synthetic("stick_walk");
    let start = recording.frames[0].time_ns;
    // The hips walk forward at half a metre a second from the first frame.
    let track = PreIkTrack {
        points: recording
            .frames
            .iter()
            .map(|frame| {
                let since_s = (frame.time_ns - start) as f32 / 1.0e9;
                (frame.time_ns, Vec2::new(0.0, -0.5 * since_s))
            })
            .collect(),
        unplaced: 0,
    };
    let eval = options();
    let configs = config::ConfigResolver::fixed(&eval);
    let recorded = compare::RecordedTrack::default();
    let (rows, _, _) = run_frames(
        &recording,
        &FrameRun {
            options: &eval,
            configs: &configs,
            drive: PelvisDrive::PreIk(&track),
            recorded: &recorded,
            clock: None,
        },
    );
    assert!(!rows.is_empty());
    for row in &rows {
        let time_ns = (row.row.time_s * 1.0e9).round() as u64;
        let expected = track.at(time_ns).expect("a fresh pose");
        assert_eq!(row.row.pelvis, expected);
    }
    let walking = &rows[rows.len() / 2].row;
    assert!(
        (walking.speed_mps - 0.5).abs() < 0.1,
        "{}",
        walking.speed_mps
    );
}
