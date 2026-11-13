use std::time::{Duration, Instant};

use body_estimator::{EgoPoserOutput, smpl::to_6d};
use glam::{DMat3, DQuat, DVec3, Quat, Vec3};

use super::*;
use crate::controllers::body::estimator::producer::Producer;

/// An upright body facing the room yaw `yaw`, every joint at rest under the pelvis.
fn facing(yaw: f64) -> EgoPoserOutput {
    let upright = DMat3::from_cols(DVec3::NEG_X, DVec3::Z, DVec3::Y);
    let pelvis = DMat3::from_rotation_z(yaw) * upright;
    let rest = to_6d(DMat3::IDENTITY).map(|v| v as f32);
    let mut pose_body = [0.0f32; 126];
    for joint in pose_body.chunks_mut(6) {
        joint.copy_from_slice(&rest);
    }
    EgoPoserOutput {
        root_orient: to_6d(pelvis).map(|v| v as f32),
        pose_body,
        betas: None,
    }
}

fn reading(yaw: f64, window_end: Instant) -> Reading {
    Reading {
        output: facing(yaw),
        window_end,
        posture: EffectivePosture::Standing,
        submitted_at: window_end,
        read_at: window_end + Duration::from_millis(12),
    }
}

#[test]
fn a_reading_turns_into_the_cockpit_frame_as_it_stands() {
    let now = Instant::now();
    let r = reading(0.7, now);
    let still = r.estimate(DQuat::IDENTITY, now + Duration::from_millis(40));
    assert!(
        (still.pelvis_yaw - 0.7).abs() < 1e-5,
        "{}",
        still.pelvis_yaw
    );
    assert!((still.chest_yaw - 0.7).abs() < 1e-5);
    assert_eq!(still.age, Duration::from_millis(40));
    assert_eq!(still.readback, Duration::from_millis(12));
    // A recenter that turned the cockpit frame by 0.3 (new from old) leaves the room frame mapped
    // from the cockpit by the inverse turn: the body faces 0.3 further round in the new frame.
    let turned = r.estimate(DQuat::from_rotation_y(-0.3), now);
    assert!(
        (turned.pelvis_yaw - 1.0).abs() < 1e-5,
        "{}",
        turned.pelvis_yaw
    );
    // The pelvis joint's forward (+Z) lies along that heading in the cockpit frame.
    let forward = turned.joints[0] * Vec3::Z;
    let expected = body_rig::angle::direction(1.0);
    assert!((forward - expected).length() < 1e-4, "{forward} {expected}");
}

#[test]
fn the_rig_takes_a_fresh_estimate_whole_and_a_stale_one_not_at_all() {
    let tuning = body_rig::EstimateTuning::new();
    let now = Instant::now();
    let fresh = reading(0.2, now).estimate(DQuat::IDENTITY, now);
    let rig = for_rig(Some(&fresh), &tuning).unwrap();
    assert_eq!(rig.confidence, 1.0);
    assert!((rig.pelvis_yaw - 0.2).abs() < 1e-5);
    let old = reading(0.2, now).estimate(
        DQuat::IDENTITY,
        now + Duration::from_secs_f32(tuning.stale_s + 0.1),
    );
    assert!(old.stale());
    assert_eq!(for_rig(Some(&old), &tuning), None);
    assert_eq!(for_rig(None, &tuning), None);
}

fn observation(now: Instant, posture: EffectivePosture) -> Observation {
    let head = Quat::IDENTITY;
    let eyes = Vec3::new(0.0, 1.7, 0.0);
    Observation {
        now,
        eyes,
        head,
        grips: [
            Some((eyes + Vec3::new(-0.2, -0.6, -0.2), head)),
            Some((eyes + Vec3::new(0.2, -0.6, -0.2), head)),
        ],
        posture,
    }
}

#[test]
fn the_producer_posts_at_the_submit_rate_once_the_window_is_full() {
    let mut producer = Producer::new();
    let start = Instant::now();
    let frame = Duration::from_secs_f64(1.0 / 90.0);
    let mut posted = Vec::new();
    for i in 0..(90 * 3) {
        let now = start + frame * i;
        if producer
            .observe(&observation(now, EffectivePosture::Standing), 30.0)
            .is_some()
        {
            posted.push(now);
        }
    }
    // Nothing until the window holds 1.35 s of poses; then about 30 a second.
    assert!(
        posted[0] >= start + Duration::from_millis(1300),
        "{posted:?}"
    );
    let rate = (posted.len() - 1) as f64
        / posted[posted.len() - 1]
            .duration_since(posted[0])
            .as_secs_f64();
    assert!((28.0..=31.0).contains(&rate), "{rate}");
}

#[test]
fn a_change_of_posture_starts_the_window_again() {
    let mut producer = Producer::new();
    let start = Instant::now();
    let frame = Duration::from_secs_f64(1.0 / 90.0);
    for i in 0..200 {
        producer.observe(
            &observation(start + frame * i, EffectivePosture::Standing),
            30.0,
        );
    }
    assert_eq!(producer.builder.len(), body_estimator::WINDOW_POSES);
    producer.observe(
        &observation(start + frame * 200, EffectivePosture::Seated),
        30.0,
    );
    assert_eq!(producer.builder.len(), 1);
}
