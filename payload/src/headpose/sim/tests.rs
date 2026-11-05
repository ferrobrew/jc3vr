use std::f32::consts::FRAC_PI_2;

use super::*;

#[test]
fn test_latch_engages_at_threshold() {
    let latch = update_latch(LatchState::Decoupled, LATCH_THRESHOLD_DEG, HeadMode::OnFoot);
    assert_eq!(latch, LatchState::BodyFollowing);
}

#[test]
fn test_latch_does_not_engage_below_threshold() {
    let latch = update_latch(
        LatchState::Decoupled,
        LATCH_THRESHOLD_DEG - 1.0,
        HeadMode::OnFoot,
    );
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_latch_engages_at_negative_threshold() {
    let latch = update_latch(
        LatchState::Decoupled,
        -LATCH_THRESHOLD_DEG,
        HeadMode::OnFoot,
    );
    assert_eq!(latch, LatchState::BodyFollowing);
}

#[test]
fn test_latch_disengages_below_hysteresis() {
    let latch = update_latch(
        LatchState::BodyFollowing,
        LATCH_DISENGAGE_THRESHOLD_DEG,
        HeadMode::OnFoot,
    );
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_latch_hysteresis_prevents_jitter() {
    // Between disengage and engage thresholds: should stay in current state.
    let mid = (LATCH_DISENGAGE_THRESHOLD_DEG + LATCH_THRESHOLD_DEG) / 2.0;
    let latch = update_latch(LatchState::BodyFollowing, mid, HeadMode::OnFoot);
    assert_eq!(latch, LatchState::BodyFollowing);
    let latch = update_latch(LatchState::Decoupled, mid, HeadMode::OnFoot);
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_latch_always_decoupled_in_other_mode() {
    let latch = update_latch(
        LatchState::BodyFollowing,
        LATCH_THRESHOLD_DEG + 10.0,
        HeadMode::Other,
    );
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_free_look_clamping_yaw() {
    let limit = FREE_LOOK_YAW_LIMIT_DEG.to_radians();
    let clamped = (limit + 1.0).clamp(-limit, limit);
    assert_eq!(clamped, limit);
    let clamped = (-limit - 1.0).clamp(-limit, limit);
    assert_eq!(clamped, -limit);
}

#[test]
fn test_free_look_clamping_pitch() {
    let limit = FREE_LOOK_PITCH_LIMIT_DEG.to_radians();
    let clamped = (limit + 1.0).clamp(-limit, limit);
    assert_eq!(clamped, limit);
    let clamped = (-limit - 1.0).clamp(-limit, limit);
    assert_eq!(clamped, -limit);
}

#[test]
fn test_mode_detection_from_counter() {
    // Counter advanced: the orientation evaluator ran, so the player is on foot.
    assert_eq!(detect_mode(0, 1), HeadMode::OnFoot);
    assert_eq!(detect_mode(5, 8), HeadMode::OnFoot);
    // Counter unchanged: not on foot.
    assert_eq!(detect_mode(5, 5), HeadMode::Other);
}

#[test]
fn test_yaw_forward_convention() {
    // At zero yaw, the forward is -Z.
    assert!((yaw_forward(0.0) - glam::Vec3::NEG_Z).length() < 1e-5);
    // A quarter turn about +Y rotates -Z to -X.
    assert!((yaw_forward(FRAC_PI_2) - glam::Vec3::NEG_X).length() < 1e-5);
    // The forward is always unit length on the ground plane.
    let forward = yaw_forward(0.5);
    assert!((forward.length() - 1.0).abs() < 1e-5);
    assert!(forward.y.abs() < 1e-6);
}

#[test]
fn test_body_turn_compensation_keeps_head_world_anchored() {
    // The on-foot compensation: when the body turns by delta, the body-relative yaw shifts by
    // -delta, keeping the world yaw (body + relative) constant.
    let body_then = 0.3_f32;
    let body_now = 0.8_f32;
    let relative = 1.0_f32;
    let compensated = wrap(relative - wrap(body_now - body_then));
    assert!(((body_now + compensated) - (body_then + relative)).abs() < 1e-5);
}
