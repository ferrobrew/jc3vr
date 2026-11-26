//! The controls summary's glance and wrist placement parameter types. The HUD's tuning file
//! holds their values.

use glam::Vec3;
use serde::Deserialize;

/// When a look at the wrist counts as a glance.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlanceConfig {
    /// The largest angle between the head's forward and the direction to the panel, degrees.
    pub view_cone_deg: f32,
    /// The largest angle between the panel's face and the direction back to the head, degrees.
    pub facing_deg: f32,
    /// How much both angles widen once the panel is shown, degrees, so that it does not flicker at
    /// the edge.
    pub hysteresis_deg: f32,
    /// How long the look must hold before the panel shows, seconds, so that a hand sweeping past
    /// the view during a fight does not show it.
    pub dwell_s: f32,
    /// How long the panel stays after the look ends, seconds.
    pub linger_s: f32,
}

/// The panel's place on the off-hand.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WristPlacement {
    /// The panel's centre in the controller's grip frame, metres: -X out of the back of the hand,
    /// +Z back along the forearm.
    pub offset_m: Vec3,
    /// The panel's width, metres.
    pub width_m: f32,
}
