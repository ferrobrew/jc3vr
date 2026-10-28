//! The grapple snatch's settings, held as `weapons.snatch`.

use serde::{Deserialize, Serialize};

use crate::controllers::bindings::yank::YankConfig;

/// The grapple snatch (see [`crate::controllers::snatch`]). Acts only under the controller scheme.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnatchConfig {
    /// Snatch a weapon out of reach into the empty grapple hand: grip at it under the grapple's
    /// aim, then yank the hand back. Also lets the grapple's aim see weapons lying in the world,
    /// which a tap of the grapple button then tosses to the player's feet, and a hold tethers.
    pub enabled: bool,
    /// Hold B after gripping a remote target to keep its rope length and swing it physically.
    pub sling: bool,
    /// The radial rope stiffness, per second squared.
    pub sling_stiffness: f32,
    /// The radial damping relative to the tracked anchor, per second.
    pub sling_damping: f32,
    /// The greatest tension acceleration, metres per second squared.
    pub sling_max_accel_mps2: f32,
    /// The farthest a target may be, metres. The hook's line caps it.
    pub range_m: f32,
    /// How far off the grapple's ray a gun in a character's hands may be and still be chosen,
    /// degrees.
    pub cone_deg: f32,
    /// The yank that fires the snatch.
    pub yank: YankConfig,
    /// The fastest the pull carries the target, metres per second.
    pub max_speed_mps: f32,
    /// The hardest the pull accelerates the target, metres per second squared.
    pub max_accel_mps2: f32,
    /// The pull's speed toward the hand per metre the target is from it, per second.
    pub stiffness_per_s: f32,
    /// How high the pull carries the target above the line to the hand, as a share of the
    /// distance still to go.
    pub arc: f32,
    /// How near the hand the target arrives and is taken, metres.
    pub capture_radius_m: f32,
    /// How little the target may move over [`stall_window_s`](Self::stall_window_s) before the
    /// pull counts as snagged, metres.
    pub stall_distance_m: f32,
    pub stall_window_s: f32,
    /// How long the hook may fly before it counts as having missed, seconds.
    pub attach_timeout_s: f32,
    /// How long a gun a character lets go of may take to lie in the world before the snatch gives
    /// up on it, seconds.
    pub free_timeout_s: f32,
    /// Snatch a gun holstered on a character's body, as from its hands: the character drops it
    /// and staggers. Needs the steal (`weapons.hold.steal`).
    pub holstered: bool,
    /// Snatch a loose object one hand can carry, into the hand's ordinary hold.
    pub props: bool,
    /// Pull a vehicle's closed door open with the line, at its handle under the aim.
    pub doors: bool,
    /// How near the grapple's aim must hit a door's handle area, metres.
    pub door_reach_m: f32,
    /// The fastest a vehicle may move for the line to pull its door open, metres per second.
    pub door_max_vehicle_speed_mps: f32,
    /// How hard the line swings a door outward once it comes open: the door's mass times this,
    /// metres per second.
    pub door_swing_mps: f32,
}

impl SnatchConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            sling: true,
            sling_stiffness: 400.0,
            sling_damping: 30.0,
            sling_max_accel_mps2: 120.0,
            range_m: 20.0,
            cone_deg: 6.0,
            yank: YankConfig {
                enabled: true,
                min_distance_m: 0.12,
                min_speed_mps: 1.5,
                window_s: 0.25,
                cooldown_s: 0.5,
                body_drop_m: 0.3,
            },
            max_speed_mps: 16.0,
            max_accel_mps2: 80.0,
            stiffness_per_s: 4.0,
            arc: 0.2,
            capture_radius_m: 0.4,
            stall_distance_m: 0.1,
            stall_window_s: 0.5,
            attach_timeout_s: 2.5,
            free_timeout_s: 0.5,
            holstered: true,
            props: true,
            doors: true,
            door_reach_m: 0.5,
            door_max_vehicle_speed_mps: 2.0,
            door_swing_mps: 3.0,
        }
    }
}

impl Default for SnatchConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::bindings::yank::YankDetector;

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
    fn the_snatch_fires_on_a_sharp_pull_toward_the_body() {
        assert_eq!(yanks(&[0.6, 0.6, 0.52, 0.42, 0.34, 0.33]), 1);
    }

    #[test]
    fn a_hand_held_still_or_drifting_does_not_fire_it() {
        assert_eq!(yanks(&[0.6; 30]), 0, "aiming, still");
        let drift: Vec<f32> = (0..40).map(|i| 0.6 - i as f32 * 0.005).collect();
        assert_eq!(yanks(&drift), 0, "drifting in at 0.15 m/s");
        assert_eq!(yanks(&[0.4, 0.5, 0.6, 0.7]), 0, "a reach out");
    }
}
