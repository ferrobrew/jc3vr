//! The IK replay: a player standing still in an upright estimate takes no step, and the summary
//! counts glances and the steps they set off.

use body_estimator::EgoPoserOutput;
use body_rig::contact::{ContactFrame, FootPose, Release};
use glam::Vec3;

use super::*;
use crate::{
    estimate::EstimateRow,
    rig_eval::{EstimateCadence, ik::stats::IkAccumulator},
    schema::Schema,
    scripts,
    synth::{self, Options},
};

fn synthetic(name: &str) -> Recording {
    let script = scripts::named(name).expect("a built-in script");
    let schema = Schema::payload().expect("the schema reads");
    synth::recording(&script, Options::default(), &schema).expect("the script samples")
}

/// An upright estimate facing the room's forward, every joint at rest, at 60 Hz over `seconds`.
fn upright(seconds: f64) -> EstimateTrack {
    let mut pose_body = [0.0f32; 126];
    for joint in pose_body.chunks_mut(6) {
        joint.copy_from_slice(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    }
    let rows: Vec<EstimateRow> = (0..(seconds * 60.0) as usize)
        .map(|i| EstimateRow {
            time_s: i as f64 / 60.0,
            output: EgoPoserOutput {
                root_orient: [-1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                pose_body,
                betas: None,
            },
        })
        .collect();
    EstimateTrack::from_rows(&rows, EstimateCadence::GAME)
}

#[test]
fn standing_still_in_an_upright_estimate_takes_no_step() {
    let recording = synthetic("stand_still");
    let end = recording.frames.last().unwrap().time_ns as f64 / 1.0e9;
    let summary = evaluate_ik(
        &recording,
        &IkEvalOptions::new(1.8, 1.68),
        &upright(end + 1.0),
    );
    assert!(summary.standing_s > end * 0.5, "{summary:?}");
    assert_eq!(summary.steps, 0, "{summary:?}");
    assert_eq!(summary.handovers, 0);
    assert_eq!(summary.reach_violations, 0, "{summary:?}");
    assert!(summary.max_drop_m < 0.02, "{summary:?}");
    assert!(summary.twist_p99_deg < 5.0, "{summary:?}");
}

#[test]
fn without_an_estimate_the_replay_stands_for_nothing() {
    let recording = synthetic("stand_still");
    let summary = evaluate_ik(
        &recording,
        &IkEvalOptions::new(1.8, 1.68),
        &EstimateTrack::default(),
    );
    assert_eq!(summary.standing_s, 0.0);
    assert_eq!(summary.spells, 0);
}

fn standing_frame() -> ContactFrame {
    ContactFrame {
        pelvis: Vec3::new(0.0, 1.0, 0.0),
        pelvis_yaw: 0.0,
        neck: Vec3::new(0.0, 1.5, 0.0),
        hips: [Vec3::new(-0.1, 0.9, 0.0), Vec3::new(0.1, 0.9, 0.0)],
        leg_m: 0.82,
        ankle_height_m: 0.08,
        root: Vec3::ZERO,
        stance: DEFAULT_STANCE,
        animated: [FootPose {
            ankle: Vec3::ZERO,
            yaw: 0.0,
        }; 2],
        dt: 1.0 / 60.0,
    }
}

#[test]
fn a_step_soon_after_a_glance_is_the_glances() {
    let mut stats = IkAccumulator::new(40.0, 0.2);
    let dt = 1.0 / 60.0;
    let mut t = 0.0;
    let mut yaw = 0.0f32;
    // Still, then a glance of 40 degrees over a quarter of a second, then still again.
    for i in 0..60 {
        if (30..45).contains(&i) {
            yaw += 40f32.to_radians() / 15.0;
        }
        stats.observe_head(t, Some(yaw), dt, true);
        stats.observe_standing(&standing_frame(), [None; 2], dt);
        t += f64::from(dt);
    }
    stats.step(t, Release::Twist);
    // A step long after it is not.
    stats.step(t + 5.0, Release::Tidy);
    let summary = stats.finish();
    assert_eq!(summary.glances, 1, "{summary:?}");
    assert_eq!(summary.glance_steps, 1);
    assert_eq!(summary.steps, 2);
    assert_eq!(summary.reasons[3], 1);
    assert!((summary.steps_per_glance - 1.0).abs() < 1e-9);
}

#[test]
fn a_planted_foot_out_of_reach_counts_once_per_excursion() {
    let mut stats = IkAccumulator::new(40.0, 0.2);
    let near = FootPose {
        ankle: Vec3::new(-0.1, 0.08, 0.0),
        yaw: 10f32.to_radians(),
    };
    // Further across the floor than the leg reaches however low the pelvis comes.
    let far = FootPose {
        ankle: Vec3::new(-0.1, 0.0, 0.9),
        yaw: 10f32.to_radians(),
    };
    let right = FootPose {
        ankle: Vec3::new(0.1, 0.08, 0.0),
        yaw: -10f32.to_radians(),
    };
    for foot in [near, far, far, near, far] {
        stats.observe_standing(&standing_frame(), [Some(foot), Some(right)], 0.1);
    }
    let summary = stats.finish();
    assert_eq!(summary.reach_violations, 2);
    assert!((summary.reach_over_s - 0.3).abs() < 1e-6);
    assert!(summary.max_drop_m >= 0.2);
    // The feet stand square under the pelvis, toe-out taken out: no twist.
    assert!(summary.twist_p99_deg < 1e-3, "{summary:?}");
}
