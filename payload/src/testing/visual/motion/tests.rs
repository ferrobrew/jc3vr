use glam::{Quat, Vec3};

use super::*;
use crate::testing::visual::{OrientationOffset, pose::orientations_match};

fn request(period_frames: u32, yaw: f32, pitch: f32, roll: f32) -> MotionRequest {
    MotionRequest::new(
        RequestId::parse("motion-sweep").unwrap(),
        period_frames,
        yaw,
        pitch,
        roll,
    )
    .unwrap()
}

fn anchor() -> PoseValues {
    PoseValues {
        position: glam::Vec3::new(12.5, 2.25, -37.75),
        yaw_deg: 24.0,
        pitch_deg: -11.0,
        roll_deg: 6.0,
    }
}

fn advance(motion: &mut MotionState, frame: i64, applied: PoseValues) -> PoseValues {
    match motion.advance(frame, applied).unwrap() {
        AdvanceOutcome::Advanced(next) => next,
        AdvanceOutcome::Duplicate => panic!("frame {frame} was already completed"),
    }
}

#[test]
fn the_trajectory_is_the_phase_angle_not_wall_time() {
    let request = request(32, 10.0, 20.0, 30.0);
    let anchor = anchor();
    assert_eq!(
        request.offset(0),
        OrientationOffset {
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            roll_deg: 0.0,
        }
    );
    let quarter = request.offset(8);
    assert!((quarter.yaw_deg - 10.0).abs() < 1e-6);
    assert!(quarter.pitch_deg.abs() < 1e-6);
    assert!((quarter.roll_deg - 30.0).abs() < 1e-6);
    let half = request.offset(16);
    assert!(half.yaw_deg.abs() < 1e-6);
    assert!(half.pitch_deg.abs() < 1e-6);
    assert!(half.roll_deg.abs() < 1e-6);
    let three_quarters = request.offset(24);
    assert!((three_quarters.yaw_deg + 10.0).abs() < 1e-6);
    assert!(three_quarters.pitch_deg.abs() < 1e-6);
    assert!((three_quarters.roll_deg + 30.0).abs() < 1e-6);

    for phase in 0..32 {
        let target = request.target(anchor, phase);
        assert_eq!(target.position, anchor.position);
        let expected = anchor.orientation()
            * Quat::from_euler(
                crate::vr::pose_control::EULER,
                request.offset(phase).yaw_deg.to_radians(),
                request.offset(phase).pitch_deg.to_radians(),
                request.offset(phase).roll_deg.to_radians(),
            );
        assert!(orientations_match(target.orientation(), expected));
        assert_eq!(request.target(anchor, phase), target);
    }
}

#[test]
fn advance_records_the_rendered_pose_and_wraps_at_the_period() {
    let request = request(32, 10.0, 0.0, 0.0);
    let anchor = anchor();
    let mut motion = MotionState::new(request.clone());
    motion.begin(anchor);
    for phase in 0..32 {
        assert_eq!(motion.phase(), phase);
        let target = request.target(anchor, phase);
        let next = advance(&mut motion, phase as i64 + 1, target);
        assert_eq!(next, request.target(anchor, (phase + 1) % 32));
        assert_eq!(motion.history.back().unwrap().phase, phase);
        assert_eq!(
            motion.history.back().unwrap().applied,
            PoseTransform::from(target)
        );
    }
    assert_eq!(motion.phase(), 0);
    assert_eq!(motion.cycle, 1);
    assert_eq!(motion.history.len(), HISTORY_FRAMES);
}

#[test]
fn a_duplicate_rendered_frame_never_advances_the_scheduler() {
    let request = request(32, 10.0, 0.0, 0.0);
    let anchor = anchor();
    let mut motion = MotionState::new(request.clone());
    motion.begin(anchor);
    advance(&mut motion, 1, request.target(anchor, 0));
    assert_eq!(motion.phase(), 1);
    assert_eq!(motion.history.len(), 1);
    assert_eq!(
        motion.advance(1, request.target(anchor, 1)).unwrap(),
        AdvanceOutcome::Duplicate
    );
    assert_eq!(motion.phase(), 1);
    assert_eq!(motion.history.len(), 1);
    assert_eq!(motion.history.back().unwrap().phase, 0);
}

#[test]
fn advance_rejects_a_pose_that_is_not_the_phase_target() {
    let request = request(32, 10.0, 20.0, 30.0);
    let anchor = anchor();
    let mut motion = MotionState::new(request);
    motion.begin(anchor);
    let wrong = PoseValues {
        yaw_deg: anchor.yaw_deg + 1.0,
        ..anchor
    };
    assert_eq!(
        motion.advance(1, wrong),
        Err(MotionFrameError::PoseMismatch {
            phase: 0,
            rendered_frame: 1,
        })
    );
    assert_eq!(motion.phase(), 0);
    assert!(motion.history.is_empty());
}

#[test]
fn capture_due_requires_a_full_cycle_and_eight_chronological_frames() {
    let request = request(32, 10.0, 0.0, 0.0);
    let anchor = anchor();
    let mut motion = MotionState::new(request.clone());
    motion.begin(anchor);
    for frame in 1..=8 {
        let target = request.target(anchor, motion.phase());
        advance(&mut motion, frame, target);
    }
    assert_eq!(motion.phase(), 8);
    assert_eq!(motion.cycle, 0);
    assert_eq!(motion.history.len(), 8);
    assert!(!motion.capture_due(8), "a full cycle has not completed");
    for frame in 9..=32 {
        let target = request.target(anchor, motion.phase());
        advance(&mut motion, frame, target);
    }
    assert_eq!(motion.phase(), 0);
    assert_eq!(motion.cycle, 1);
    assert!(motion.capture_due(0));
    assert!(!motion.capture_due(1));
    for frame in 33..=40 {
        let target = request.target(anchor, motion.phase());
        advance(&mut motion, frame, target);
    }
    assert_eq!(motion.phase(), 8);
    assert!(motion.capture_due(8));
    assert!(motion.history.len() == HISTORY_FRAMES);
}

#[test]
fn history_is_capped_at_eight_and_stays_chronological() {
    let request = request(32, 10.0, 0.0, 0.0);
    let anchor = anchor();
    let mut motion = MotionState::new(request.clone());
    motion.begin(anchor);
    for frame in 1..=40 {
        let target = request.target(anchor, motion.phase());
        advance(&mut motion, frame, target);
    }
    assert_eq!(motion.history.len(), HISTORY_FRAMES);
    let entries: Vec<_> = motion.history.iter().collect();
    for pair in entries.windows(2) {
        assert!(pair[0].rendered_frame < pair[1].rendered_frame);
    }
    let mut expected = motion.phase();
    for entry in entries.iter().rev() {
        expected = previous_phase(expected, request.period_frames());
        assert_eq!(entry.phase, expected);
    }
}

#[test]
fn metadata_reports_the_phase_and_the_prior_eight_frames() {
    let request = request(32, 10.0, 0.0, 0.0);
    let anchor = anchor();
    let mut motion = MotionState::new(request.clone());
    motion.begin(anchor);
    for frame in 1..=20 {
        let target = request.target(anchor, motion.phase());
        advance(&mut motion, frame, target);
    }
    let metadata = motion.metadata(21);
    assert_eq!(metadata.request, request);
    assert_eq!(metadata.phase, motion.phase());
    assert_eq!(metadata.cycle, motion.cycle);
    assert_eq!(metadata.rendered_frame, 21);
    assert_eq!(metadata.history.len(), HISTORY_FRAMES);
    assert_eq!(metadata.history.last().unwrap().rendered_frame, 20);
}

#[test]
fn motion_requests_validate_period_and_amplitude_bounds() {
    let id = || RequestId::parse("motion-sweep").unwrap();
    for period in [0, 1, 2, 3, 31, 601, u32::MAX] {
        assert_eq!(
            MotionRequest::new(id(), period, 0.0, 0.0, 0.0),
            Err(MotionError::PeriodOutOfRange(period))
        );
    }
    for period in [MIN_PERIOD_FRAMES, 300, MAX_PERIOD_FRAMES] {
        assert!(MotionRequest::new(id(), period, 45.0, 0.0, 45.0).is_ok());
    }
    for (angle, value) in [("yaw", -0.1), ("pitch", 45.1), ("roll", 90.0)] {
        let (yaw, pitch, roll) = match angle {
            "yaw" => (value, 0.0, 0.0),
            "pitch" => (0.0, value, 0.0),
            _ => (0.0, 0.0, value),
        };
        let error = MotionRequest::new(id(), 32, yaw, pitch, roll).unwrap_err();
        assert_eq!(error, MotionError::AmplitudeOutOfRange(angle));
    }
    for (angle, value) in [
        ("yaw", f32::NAN),
        ("pitch", f32::INFINITY),
        ("roll", f32::NEG_INFINITY),
    ] {
        let (yaw, pitch, roll) = match angle {
            "yaw" => (value, 0.0, 0.0),
            "pitch" => (0.0, value, 0.0),
            _ => (0.0, 0.0, value),
        };
        let error = MotionRequest::new(id(), 32, yaw, pitch, roll).unwrap_err();
        assert_eq!(error, MotionError::NonFiniteAmplitude(angle));
    }
}

#[test]
fn a_motion_table_parses_only_the_agreed_wire_shape() {
    let table: MotionTable = toml::from_str(
        "case_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = 10.0\npitch_deg = 5.0\nroll_deg = 2.5",
    )
    .unwrap();
    let request = MotionRequest::try_from(table).unwrap();
    assert_eq!(&request.case_id, &RequestId::parse("motion-sweep").unwrap());
    assert_eq!(request.period_frames(), 32);
    assert!(toml::from_str::<MotionTable>("case_id = 'motion-sweep'\nperiod_frames = 32").is_err());
    assert!(
        toml::from_str::<MotionTable>(
            "case_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = 1.0\npitch_deg = 0.0\nroll_deg = 0.0\nunknown = 1"
        )
        .is_err()
    );
}

#[test]
fn a_translation_swings_along_the_anchor_axes() {
    let table: MotionTable = toml::from_str(
        "case_id = 'surge'\nperiod_frames = 40\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0\n\
         forward_m = 2.0",
    )
    .unwrap();
    let surge = MotionRequest::try_from(table).unwrap();
    let anchor = PoseValues::from_pose(Vec3::new(1.0, 2.0, 3.0), Quat::IDENTITY);
    assert_eq!(surge.translation(0), TranslationOffset::ZERO);
    let quarter = surge.target(anchor, 10);
    assert!((quarter.position - Vec3::new(1.0, 2.0, 1.0)).length() < 1e-5);
    assert_eq!(surge.evidence(anchor, 10, quarter).translation.forward, 2.0);
    for (right, up, forward) in [(20.5, 0.0, 0.0), (0.0, f32::NAN, 0.0), (0.0, 0.0, -21.0)] {
        let request = request(40, 0.0, 0.0, 0.0);
        assert!(request.with_translation(right, up, forward).is_err());
    }
}
