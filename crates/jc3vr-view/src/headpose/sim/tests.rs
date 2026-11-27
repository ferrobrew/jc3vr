use super::*;
use crate::tuning::FlatscreenLookTuning;

fn look() -> &'static FlatscreenLookTuning {
    &VIEW.get().flatscreen_look
}

#[test]
fn test_latch_engages_at_threshold() {
    let latch = update_latch(
        LatchState::Decoupled,
        look().latch_threshold_deg,
        HeadMode::OnFoot,
    );
    assert_eq!(latch, LatchState::BodyFollowing);
}

#[test]
fn test_latch_does_not_engage_below_threshold() {
    let latch = update_latch(
        LatchState::Decoupled,
        look().latch_threshold_deg - 1.0,
        HeadMode::OnFoot,
    );
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_latch_engages_at_negative_threshold() {
    let latch = update_latch(
        LatchState::Decoupled,
        -look().latch_threshold_deg,
        HeadMode::OnFoot,
    );
    assert_eq!(latch, LatchState::BodyFollowing);
}

#[test]
fn test_latch_disengages_below_hysteresis() {
    let latch = update_latch(
        LatchState::BodyFollowing,
        look().latch_disengage_threshold_deg,
        HeadMode::OnFoot,
    );
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_latch_hysteresis_prevents_jitter() {
    // Between disengage and engage thresholds: should stay in current state.
    let mid = (look().latch_disengage_threshold_deg + look().latch_threshold_deg) / 2.0;
    let latch = update_latch(LatchState::BodyFollowing, mid, HeadMode::OnFoot);
    assert_eq!(latch, LatchState::BodyFollowing);
    let latch = update_latch(LatchState::Decoupled, mid, HeadMode::OnFoot);
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_latch_always_decoupled_in_other_mode() {
    let latch = update_latch(
        LatchState::BodyFollowing,
        look().latch_threshold_deg + 10.0,
        HeadMode::Other,
    );
    assert_eq!(latch, LatchState::Decoupled);
}

#[test]
fn test_free_look_clamping_yaw() {
    let limit = look().free_look_yaw_limit_deg.to_radians();
    let clamped = (limit + 1.0).clamp(-limit, limit);
    assert_eq!(clamped, limit);
    let clamped = (-limit - 1.0).clamp(-limit, limit);
    assert_eq!(clamped, -limit);
}

#[test]
fn test_free_look_clamping_pitch() {
    let limit = look().free_look_pitch_limit_deg.to_radians();
    let clamped = (limit + 1.0).clamp(-limit, limit);
    assert_eq!(clamped, limit);
    let clamped = (-limit - 1.0).clamp(-limit, limit);
    assert_eq!(clamped, -limit);
}

#[test]
fn test_mode_detection_from_mark() {
    // The mark advanced: the orientation evaluator ran, so the player is on foot.
    assert_eq!(detect_mode(0, 1), HeadMode::OnFoot);
    assert_eq!(detect_mode(5, 8), HeadMode::OnFoot);
    // The mark unchanged: not on foot.
    assert_eq!(detect_mode(5, 5), HeadMode::Other);
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
