//! Checks for the controller-owned hands, per-hand aiming, and the world-space
//! overlay.

use crate::{
    config::validate::{Invalid, nonnegative, positive, range, vec3},
    controllers::config::{AimConfig, HandsConfig, OverlayConfig},
};

/// Check the controller-owned hands' settings. Paths are relative to `controllers.hands`.
pub(super) fn hands(c: &HandsConfig) -> Result<(), Invalid> {
    vec3("hands.grip.position_m", c.grip.position_m, 5.0)?;
    vec3("hands.grip.rotation_deg", c.grip.rotation_deg, 360.0)?;
    nonnegative("hands.anchor.blend_s", c.anchor.blend_s, 60.0)?;
    nonnegative(
        "hands.anchor.swing_deadzone_m",
        c.anchor.swing_deadzone_m,
        10.0,
    )?;
    positive("hands.anchor.swing_full_m", c.anchor.swing_full_m, 10.0)?;
    range(
        "hands.anchor.swing_scale",
        c.anchor.swing_scale,
        -100.0,
        100.0,
    )
}

/// Check the per-hand aiming settings. Paths are relative to `controllers.aim`.
pub(super) fn aim(c: &AimConfig) -> Result<(), Invalid> {
    range("aim.assist_strength", c.assist_strength, 0.0, 1.0)?;
    nonnegative(
        "aim.assist_max_distance_m",
        c.assist_max_distance_m,
        10_000.0,
    )?;
    nonnegative(
        "aim.assist_occlusion_tolerance_m",
        c.assist_occlusion_tolerance_m,
        100.0,
    )?;
    positive("aim.max_distance_m", c.max_distance_m, 100_000.0)?;
    nonnegative("aim.min_target_distance_m", c.min_target_distance_m, 100.0)?;
    vec3("aim.grapple_origin_m", c.grapple_origin_m, 5.0)?;
    nonnegative(
        "aim.sight_tuning.marker_distance_m",
        c.sight_tuning.marker_distance_m,
        10_000.0,
    )
}

/// Check the world-space overlay's settings. Paths are relative to `controllers.overlay`.
pub(super) fn overlay(c: &OverlayConfig) -> Result<(), Invalid> {
    nonnegative("overlay.depth_tolerance_m", c.depth_tolerance_m, 100.0)?;
    nonnegative("overlay.depth_tolerance_rel", c.depth_tolerance_rel, 100.0)?;
    positive("overlay.min_line_px", c.min_line_px, 10_000.0)?;
    range("overlay.xray_alpha", c.xray_alpha, 0.0, 1.0)
}
