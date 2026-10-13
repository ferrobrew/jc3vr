//! Checks for posture and roomscale: the auto-posture timers, the seated reference, the crouch, and
//! the head fade.

use crate::{
    config::validate::{Invalid, nonnegative, positive, range},
    controllers::config::RoomscaleConfig,
};

/// Check the roomscale settings. Paths are relative to `controllers.roomscale`.
pub(super) fn roomscale(r: &RoomscaleConfig) -> Result<(), Invalid> {
    nonnegative(
        "roomscale.auto_posture.stand_after_s",
        r.auto_posture.stand_after_s,
        600.0,
    )?;
    nonnegative(
        "roomscale.auto_posture.sit_after_s",
        r.auto_posture.sit_after_s,
        3_600.0,
    )?;
    range(
        "roomscale.seated_reference.window_s",
        r.seated_reference.window_s,
        10.0,
        3_600.0,
    )?;
    nonnegative(
        "roomscale.seated_reference.rate_mps",
        r.seated_reference.rate_mps,
        1.0,
    )?;
    nonnegative(
        "roomscale.seated_reference.deadband_m",
        r.seated_reference.deadband_m,
        1.0,
    )?;
    nonnegative("roomscale.crouch.enter_drop_m", r.crouch.enter_drop_m, 10.0)?;
    nonnegative("roomscale.crouch.exit_drop_m", r.crouch.exit_drop_m, 10.0)?;
    nonnegative(
        "roomscale.crouch.seated_drop_m",
        r.crouch.seated_drop_m,
        10.0,
    )?;
    nonnegative(
        "roomscale.head_fade.start_depth_m",
        r.head_fade.start_depth_m,
        100.0,
    )?;
    positive(
        "roomscale.head_fade.full_depth_m",
        r.head_fade.full_depth_m,
        100.0,
    )?;
    nonnegative("roomscale.fade.out_s", r.fade.out_s, 60.0)?;
    nonnegative("roomscale.fade.in_s", r.fade.in_s, 60.0)?;
    nonnegative("roomscale.seat_fade_hold_s", r.seat_fade_hold_s, 60.0)
}
