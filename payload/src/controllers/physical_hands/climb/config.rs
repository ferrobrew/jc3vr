//! The hand climbing's settings.

use serde::{Deserialize, Serialize};

use crate::controllers::weapons::config::PulseSpec;

/// Climbing with the hands (see [`crate::controllers::physical_hands::climb`]): an empty hand whose
/// body touches the static world grips it with the grip, and the player's body moves against the
/// hands' motion. It needs the hand bodies.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClimbConfig {
    /// Grip the static world with an empty hand and climb.
    pub enabled: bool,
    /// How far past the palm a surface may be for the grip to take it, metres of the player's hand.
    pub reach_m: f32,
    /// The share of the gap between where the hands would have the body and where it is that one
    /// physics step closes, `0..=1`.
    pub gain: f32,
    /// The fastest the hands move the body, metres per second.
    pub max_speed_mps: f32,
    /// How far the body may be from where the hands would have it before the grips let go, metres:
    /// a hand pulled much further than the body can follow, through a wall or out of reach.
    pub break_distance_m: f32,
    /// Keep the body's speed when the last grip lets go, so that a pull flings the player.
    pub fling: bool,
    /// The share of the climbing speed a release keeps.
    pub fling_scale: f32,
    /// The fastest fling, metres per second.
    pub max_fling_speed_mps: f32,
    /// How long the climbing speed a fling keeps is smoothed over, seconds.
    pub fling_smoothing_s: f32,
    /// Carry the body over a ledge the hands grip the top of, once they have pulled it near the
    /// top, and let go when it stands there.
    pub mantle: bool,
    /// How far below the gripped top the feet may be for the mantle to start, metres.
    pub mantle_start_m: f32,
    /// How far above the gripped top the mantle lifts the feet before it carries the body over,
    /// metres.
    pub mantle_clearance_m: f32,
    /// How far past the grip the mantle carries the body, metres.
    pub mantle_forward_m: f32,
    /// The mantle's speed, metres per second.
    pub mantle_speed_mps: f32,
    /// The least smoothed upward climbing speed for the mantle to start, metres per second, so that
    /// a twitch upward near a ledge does not start one.
    pub mantle_min_speed_mps: f32,
    /// How far the feet must have climbed since the climb started for the mantle to start while
    /// the player still stands on something, metres.
    pub mantle_min_climb_m: f32,
    /// How far past the ledge's top the mantle carries the feet before it lets them down onto it,
    /// metres: about the player capsule's radius.
    pub mantle_land_past_m: f32,
    /// The least upward share of a gripped surface's normal for it to count as a ledge's top,
    /// `0..=1`.
    pub min_ledge_normal_y: f32,
    /// The pulse of a hand gripping the world.
    pub grab_pulse: PulseSpec,
    /// The pulse of a hand letting go of the world.
    pub release_pulse: PulseSpec,
}

impl ClimbConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            reach_m: 0.05,
            gain: 0.8,
            max_speed_mps: 6.0,
            break_distance_m: 1.2,
            fling: true,
            fling_scale: 1.0,
            max_fling_speed_mps: 8.0,
            fling_smoothing_s: 0.08,
            mantle: true,
            mantle_start_m: 0.9,
            mantle_clearance_m: 0.08,
            mantle_forward_m: 0.4,
            mantle_speed_mps: 2.0,
            mantle_min_speed_mps: 0.3,
            mantle_min_climb_m: 0.25,
            mantle_land_past_m: 0.3,
            min_ledge_normal_y: 0.7,
            grab_pulse: PulseSpec {
                amplitude: 0.6,
                duration_s: 0.04,
            },
            release_pulse: PulseSpec {
                amplitude: 0.25,
                duration_s: 0.02,
            },
        }
    }
}

impl Default for ClimbConfig {
    fn default() -> Self {
        Self::new()
    }
}
