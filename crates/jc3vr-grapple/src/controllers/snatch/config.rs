//! The grapple snatch's fixed tuning. Its settings, held as `weapons.snatch`, live with the
//! weapon-handling settings ([`SnatchConfig`]).

use jc3vr_body::controllers::overlay::config::OverlayConfig;
use jc3vr_physical::controllers::physical_hands::config::PhysicalHandsConfig;
use jc3vr_weapons::controllers::weapons::config::HoldConfig;
pub use jc3vr_weapons::controllers::weapons::config::{SnatchConfig, SwingConfig};

use crate::{
    controllers::snatch::landing::LandingTuning, grapple::physical::config::PhysicalGrappleConfig,
};

/// The settings one snatch tick reads: its own group and the fields of the other groups it
/// consults. The bindings fill it from the snapshot of the settings they take for the tick.
#[derive(Copy, Clone)]
pub struct SnatchSettings {
    /// The snatch's own settings.
    pub snatch: SnatchConfig,
    /// Which hand holds a weapon, which decides whether a weapon held by a character is offered.
    pub hold: HoldConfig,
    /// The world-space overlay, which draws the armed snatch's latch.
    pub overlay: OverlayConfig,
    /// The physical grapple, whose free-hand line the swing hands over to.
    pub grapple: PhysicalGrappleConfig,
    /// The physical hands, whose door handling decides which doors are snatched.
    pub physical_hands: PhysicalHandsConfig,
}

/// The mass the line's [`strength_n`](SwingConfig::strength_n) is given at, kilograms.
pub const REFERENCE_MASS_KG: f32 = 10.0;

/// The grapple snatch's tuning, fixed at [`SNATCH`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SnatchTuning {
    /// How near the grapple hand's wrist the other hand's wrist must be for its grip to brace it,
    /// metres.
    pub brace_take_m: f32,
    /// How far the bracing hand's wrist may drift from the grapple hand's before the brace breaks,
    /// metres.
    pub brace_break_m: f32,
    /// The farthest a target may be, metres. The hook's line caps it.
    pub range_m: f32,
    /// How far off the grapple's ray a gun in a character's hands or holster, or a weapon lying in
    /// the world, may be and still be chosen, degrees.
    pub cone_deg: f32,
    /// How much wider the cone is for a weapon lying in the world at the range, degrees, in
    /// proportion to its distance ([`floor_cone`](crate::controllers::snatch::rules::floor_cone)).
    pub floor_cone_widen_deg: f32,
    /// The speed the line yanks the target toward the hand at while it is farther than
    /// [`approach_m`](Self::approach_m), metres per second.
    pub line_speed_mps: f32,
    /// The speed the line asks of the target at the hand, metres per second: over the last
    /// [`approach_m`](Self::approach_m) the line's speed falls in proportion to the distance from
    /// [`line_speed_mps`](Self::line_speed_mps) to this, so the target still arrives hard but
    /// slower than it flew ([`line_speed`](crate::controllers::snatch::rules::line_speed)).
    pub arrival_speed_mps: f32,
    /// The hardest the line accelerates the target, metres per second squared: the line's pull is
    /// the target's mass times this, so a heavy target comes as hard as a light one, and a target
    /// a step away reaches its speed within a tick or two.
    pub line_accel_mps2: f32,
    /// How near the hand the final approach starts, metres. Within it the line's speed falls
    /// toward [`arrival_speed_mps`](Self::arrival_speed_mps), its arc fades to nothing, and it
    /// pulls at the target's centre rather than at the hook, so the spin the off-centre pull gave
    /// the target does not carry the centre off the line when it goes slack.
    pub approach_m: f32,
    /// How high the line aims above the hand, as a share of the distance still to go, so a far
    /// target clears the ground on its way. It fades to nothing over the final approach.
    pub arc: f32,
    /// How long before the target would reach the hand the line goes slack, seconds: the slack
    /// radius is the target's speed times this, kept between
    /// [`slack_min_m`](Self::slack_min_m) and [`slack_max_m`](Self::slack_max_m)
    /// ([`slack_radius`](crate::controllers::snatch::rules::slack_radius)). The target then flies
    /// on by its own momentum for about two ticks, wherever the hand goes.
    pub coast_s: f32,
    pub slack_min_m: f32,
    pub slack_max_m: f32,
    /// How near a hand's palm the target's centre must pass for the hand to catch it, metres:
    /// the hand's own reach around the target, with no pull toward the hand.
    pub catch_zone_m: f32,
    /// The mass at which a target's catch zone reaches [`heavy_catch_zone_m`](Self::heavy_catch_zone_m)
    /// and the line's speed falls to [`heavy_speed_share`](Self::heavy_speed_share), kilograms. A
    /// target at or below half of it keeps the light values. A heavy target's centre cannot come as near
    /// the palm as a light one's, so the hand catches it from further out, and it needs less speed
    /// to arrive without passing the hand.
    pub heavy_mass_kg: f32,
    /// The catch zone for a heavy target, metres.
    pub heavy_catch_zone_m: f32,
    /// The line's speed for a heavy target as a share of [`line_speed_mps`](Self::line_speed_mps),
    /// `0..=1`.
    pub heavy_speed_share: f32,
    /// How long after the target was last within a hand's catch zone the hand's grip may still
    /// close on it and catch it, seconds.
    pub catch_grace_s: f32,
    /// How long a slack target may fly before it counts as missed, seconds.
    pub max_slack_s: f32,
    /// The fastest the line's pull at the hook may spin the target, radians per second.
    pub max_spin_rps: f32,
    /// How little the target may move over [`stall_window_s`](Self::stall_window_s) before the
    /// pull counts as snagged, metres.
    pub stall_distance_m: f32,
    pub stall_window_s: f32,
    /// How long the hook may fly before it counts as having missed, seconds.
    pub attach_timeout_s: f32,
    /// How long a gun a character lets go of may take to lie in the world before the snatch gives
    /// up on it, seconds.
    pub free_timeout_s: f32,
    /// How near the grapple's aim must hit a door's handle area, metres.
    pub door_reach_m: f32,
    /// The fastest a vehicle may move for the line to pull its door open, metres per second.
    pub door_max_vehicle_speed_mps: f32,
    /// How hard the line swings a door outward once it comes open: the door's mass times this,
    /// metres per second.
    pub door_swing_mps: f32,
    /// How a snatched gun is steered into the pose the catching hand will hold it in over its
    /// final approach.
    pub landing: LandingTuning,
}

/// The grapple snatch's tuning.
pub const SNATCH: SnatchTuning = SnatchTuning {
    brace_take_m: 0.15,
    brace_break_m: 0.3,
    range_m: 20.0,
    cone_deg: 6.0,
    floor_cone_widen_deg: 4.0,
    line_speed_mps: 18.0,
    arrival_speed_mps: 12.0,
    line_accel_mps2: 240.0,
    approach_m: 2.5,
    arc: 0.08,
    coast_s: 0.07,
    slack_min_m: 0.4,
    slack_max_m: 1.0,
    catch_zone_m: 0.4,
    heavy_mass_kg: 60.0,
    heavy_catch_zone_m: 0.8,
    heavy_speed_share: 0.7,
    catch_grace_s: 0.15,
    max_slack_s: 0.5,
    max_spin_rps: 25.0,
    stall_distance_m: 0.1,
    stall_window_s: 0.5,
    attach_timeout_s: 2.5,
    free_timeout_s: 0.5,
    door_reach_m: 0.5,
    door_max_vehicle_speed_mps: 2.0,
    door_swing_mps: 3.0,
    landing: LandingTuning {
        ramp_m: 0.25,
        ramp_ticks: 1.0,
        min_t_go_s: 1.0 / 30.0,
        max_t_go_s: 0.5,
        max_spin_rps: 40.0,
        max_angular_accel: 1200.0,
        lateral_window_m: 0.3,
        hysteresis_m: 0.1,
        max_extrapolate_s: 0.1,
        settled_rad: 5.0 * DEGREE,
        settled_m: 0.02,
        full_slide_rad: 30.0 * DEGREE,
    },
};

/// One degree, radians.
const DEGREE: f32 = std::f32::consts::PI / 180.0;

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::intent::yank::YankDetector;

    use super::*;

    const DT: f32 = 1.0 / 30.0;

    fn yanks(distances: &[f32]) -> usize {
        let config = SnatchConfig::new().yank;
        let mut detector = YankDetector::new();
        distances
            .iter()
            .filter(|&&d| detector.step(DT, Some(d), true, &config).is_some())
            .count()
    }

    #[test]
    fn the_snatch_fires_on_a_sharp_pull_back_along_the_aim() {
        assert_eq!(yanks(&[0.6, 0.6, 0.52, 0.42, 0.34, 0.33]), 1);
    }

    #[test]
    fn a_deliberate_moderate_pull_fires_it() {
        // About 1.2 m/s over 0.16 m, after a pause at the grip.
        assert_eq!(yanks(&[0.6, 0.6, 0.6, 0.56, 0.52, 0.48, 0.44, 0.44]), 1);
    }

    #[test]
    fn a_hand_held_still_or_drifting_does_not_fire_it() {
        assert_eq!(yanks(&[0.6; 30]), 0, "aiming, still");
        let drift: Vec<f32> = (0..40).map(|i| 0.6 - i as f32 * 0.005).collect();
        assert_eq!(yanks(&drift), 0, "drifting in at 0.15 m/s");
        assert_eq!(yanks(&[0.4, 0.5, 0.6, 0.7]), 0, "a reach out");
    }
}
