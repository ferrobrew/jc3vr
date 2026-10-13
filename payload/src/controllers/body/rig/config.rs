//! The body rig's settings, under `controllers.body.rig`.

use serde::{Deserialize, Serialize};

use crate::config::validate::Invalid;

/// The body rig: who moves the root, the chase that keeps it under the physical head, and the
/// lean the chase leaves alone (`docs/mod/body/rig.md`). Both default on.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigConfig {
    /// The body walking after the player's head in the standing posture.
    pub chase: ChaseConfig,
    /// The lean over the feet that the chase leaves alone.
    pub lean: LeanConfig,
}

impl RigConfig {
    pub const fn new() -> Self {
        Self {
            chase: ChaseConfig::new(),
            lean: LeanConfig::new(),
        }
    }

    /// Check the settings whose types admit values the rig cannot use. Paths are relative to
    /// `controllers`.
    pub(crate) fn validate(&self) -> Result<(), Invalid> {
        let checks = [
            ("chase.gain", self.chase.gain, 0.01, 1.0),
            ("chase.feed_forward", self.chase.feed_forward, 0.0, 1.5),
            ("chase.max_speed_mps", self.chase.max_speed_mps, 0.1, 12.0),
            ("chase.warp_min_m", self.chase.warp_min_m, 1.0, 1_000.0),
            ("lean.gain", self.lean.gain, 0.0, 10.0),
            ("lean.deadband_m", self.lean.deadband_m, 0.0, 0.5),
            ("lean.max_m", self.lean.max_m, 0.0, 1.0),
            ("lean.side_share", self.lean.side_share, 0.0, 1.0),
            ("lean.back_share", self.lean.back_share, 0.0, 1.0),
        ];
        for (name, value, min, max) in checks {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(Invalid(format!(
                    "body.rig.{name} must be finite and in [{min}, {max}], got {value}"
                )));
            }
        }
        Ok(())
    }
}

impl Default for RigConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The chase: in the standing posture, the root walks toward the ground point under the player's
/// upright eyes, with no deadzone, on top of whatever the stick and the hands ask for.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChaseConfig {
    /// On by default: it moves the character from head motion, which the headset alone produces.
    pub enabled: bool,
    /// The share of the remaining gap the chase closes each simulation tick, `0..=1`.
    pub gain: f32,
    /// How much of the head's own velocity the chase adds ahead of the gap, `0..`: one cancels the
    /// lag a walk in the room would otherwise leave.
    pub feed_forward: f32,
    /// The fastest the chase moves the body, metres per second.
    pub max_speed_mps: f32,
    /// The shortest single-tick jump of the body, metres, that counts as a teleport the game made
    /// without announcing it. The threshold also grows with the body's speed.
    pub warp_min_m: f32,
}

impl ChaseConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            gain: 0.4,
            feed_forward: 1.0,
            max_speed_mps: 4.0,
            warp_min_m: 8.0,
        }
    }
}

impl Default for ChaseConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The lean: how far the neck pivot may be out over the feet, without the chase stepping after
/// it, for how far it has dropped below the player's standing height.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LeanConfig {
    /// On by default: without it, leaning over a table walks the body into the table.
    pub enabled: bool,
    /// Metres of lean allowed per metre of drop past the deadband.
    pub gain: f32,
    /// The drop below the standing height that allows no lean, metres: a standing head's bob.
    pub deadband_m: f32,
    /// The furthest lean allowed, metres, forward of the torso.
    pub max_m: f32,
    /// The lean allowed to the side, as a share of the forward lean, `0..=1`.
    pub side_share: f32,
    /// The lean allowed backward, as a share of the forward lean, `0..=1`.
    pub back_share: f32,
}

impl LeanConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            gain: 4.0,
            deadband_m: 0.04,
            max_m: 0.35,
            side_share: 0.5,
            back_share: 0.25,
        }
    }
}

impl Default for LeanConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_validate() {
        assert_eq!(RigConfig::new().validate(), Ok(()));
        let mut bad = RigConfig::new();
        bad.chase.gain = f32::NAN;
        assert!(bad.validate().is_err());
        bad = RigConfig::new();
        bad.lean.side_share = 2.0;
        assert!(
            bad.validate()
                .unwrap_err()
                .0
                .contains("body.rig.lean.side_share")
        );
    }
}
