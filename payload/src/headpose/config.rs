//! Headpose configuration: the thresholds and tunables for the headpose simulation.

use serde::{Deserialize, Serialize};

use crate::grapple::GrappleComfortConfig;

/// All thresholds and tunables for the headpose simulation, added to [`crate::config::Config`] as
/// `headpose`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HeadPoseConfig {
    /// Master switch for headpose-driven head control.
    pub enabled: bool,
    /// VR on-foot body-yaw (turning) settings. In VR the HMD owns the head, so the look input
    /// (mouse and right stick) is free to turn the body instead (see [`VrTurnConfig`]).
    #[serde(default)]
    pub vr_turn: VrTurnConfig,
    /// Grapple comfort settings (see [`crate::grapple`]). Owned by the grapple module; carried
    /// here so it persists and live-edits with the rest of the headpose tunables.
    #[serde(default)]
    pub grapple: GrappleComfortConfig,
}
impl HeadPoseConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            vr_turn: VrTurnConfig::new(),
            grapple: GrappleComfortConfig::new(),
        }
    }
}

/// How the VR body-yaw responds to the look input while on foot.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VrTurnMode {
    /// Continuous rotation proportional to the look input. The most direct mapping — the mouse and
    /// stick turn the body just as they turned the head in flatscreen.
    #[default]
    Smooth,
    /// Discrete steps of [`VrTurnConfig::snap_angle_deg`], one per look-input flick past the
    /// threshold. Reduces vection for players prone to motion sickness.
    Snap,
}

/// VR on-foot body-yaw (turning) settings.
///
/// The head is HMD-driven, so the look effectors (mouse and right stick) do not steer the head
/// and are repurposed to turn the body. The turned heading is fed to the locomotion hook as the
/// body's face-dir target, so the character's whole frame — body and, composed onto it, the head —
/// rotates with the input.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct VrTurnConfig {
    /// Smooth (continuous) or snap (discrete step) turning.
    pub mode: VrTurnMode,
    /// Smooth-turn scale: the per-tick body yaw is the look delta times
    /// [`MOUSE_SENSITIVITY`](crate::headpose::sim::MOUSE_SENSITIVITY) times this, so `1.0` turns the
    /// body with the look input exactly as flatscreen turned the head.
    pub smooth_scale: f32,
    /// Snap-turn step (degrees), applied once per look flick past [`snap_threshold`](Self::snap_threshold).
    pub snap_angle_deg: f32,
    /// The look-input magnitude that arms a snap step; the step re-arms once the input falls back
    /// below half this, giving hysteresis so one flick is one step.
    pub snap_threshold: f32,
    /// Look-input magnitudes below this are ignored, rejecting stick drift. Applies to *absolute*
    /// (stick) input only; delta-based (mouse) input skips it, since a slow mouse move is a small but
    /// meaningful per-tick delta, not drift.
    pub deadzone: f32,
}

impl VrTurnConfig {
    pub const fn new() -> Self {
        Self {
            mode: VrTurnMode::Smooth,
            smooth_scale: 1.0,
            snap_angle_deg: 30.0,
            snap_threshold: 0.6,
            deadzone: 0.02,
        }
    }
}

impl Default for VrTurnConfig {
    fn default() -> Self {
        Self::new()
    }
}
