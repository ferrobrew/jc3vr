//! The body rig's settings, under `controllers.body.rig`.

use serde::{Deserialize, Serialize};

use crate::config::validate::Invalid;

/// The body rig's switches: the chase that keeps the root under the physical head, the lean the
/// chase leaves alone, the push-back at walls, and the character turning to face the torso; and
/// the legs (`docs/mod/body/rig.md`). All default on. The tuning is fixed in the modules that own
/// it.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigConfig {
    /// The body walking after the player's head in the standing posture.
    pub chase: ChaseConfig,
    /// The lean over the feet that the chase leaves alone.
    pub lean: LeanConfig,
    /// Holding the view at a wall the body cannot pass.
    pub pushback: PushbackConfig,
    /// The character turning to face the player's torso.
    pub torso: TorsoConfig,
    /// The legs: the walk and the standing steps.
    pub legs: LegsConfig,
}

impl RigConfig {
    pub const fn new() -> Self {
        Self {
            chase: ChaseConfig::new(),
            lean: LeanConfig::new(),
            pushback: PushbackConfig::new(),
            torso: TorsoConfig::new(),
            legs: LegsConfig::new(),
        }
    }

    /// Check the settings whose types admit values the rig cannot use. Paths are relative to
    /// `controllers`.
    pub(crate) fn validate(&self) -> Result<(), Invalid> {
        let checks = [
            ("legs.moving_mps", self.legs.moving_mps, 0.01, 5.0),
            ("legs.standing_mps", self.legs.standing_mps, 0.0, 5.0),
            ("legs.rest_error_m", self.legs.rest_error_m, 0.01, 1.0),
            ("legs.turn_error_deg", self.legs.turn_error_deg, 1.0, 180.0),
            ("legs.step_s", self.legs.step_s, 0.05, 2.0),
            ("legs.step_height_m", self.legs.step_height_m, 0.0, 0.5),
            (
                "legs.support_margin_m",
                self.legs.support_margin_m,
                0.0,
                0.5,
            ),
            ("legs.capture_gain", self.legs.capture_gain, 0.0, 2.0),
            ("legs.reach_share", self.legs.reach_share, 0.1, 1.0),
        ];
        for (name, value, min, max) in checks {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(Invalid(format!(
                    "body.rig.{name} must be finite and in [{min}, {max}], got {value}"
                )));
            }
        }
        if self.legs.standing_mps >= self.legs.moving_mps {
            return Err(Invalid(format!(
                "body.rig.legs.standing_mps ({}) is not below moving_mps ({})",
                self.legs.standing_mps, self.legs.moving_mps
            )));
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
}

impl ChaseConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for ChaseConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The lean: how far the neck pivot may be out over the feet, without the chase stepping after
/// it, for how far it has dropped below the player's standing height
/// ([`crate::controllers::body::rig::lean`]).
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LeanConfig {
    /// On by default: without it, leaning over a table walks the body into the table.
    pub enabled: bool,
}

impl LeanConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for LeanConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The push-back: where the body cannot follow the head (a wall, a table's edge), the view is held
/// at a threshold past the root by sliding the cockpit back, up to a cap per episode, and fades
/// to black past the cap ([`crate::controllers::body::rig::root::pushback`]).
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PushbackConfig {
    /// On by default. Off, the view goes through the wall and only the fade answers it.
    pub enabled: bool,
}

impl PushbackConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for PushbackConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The character turning under the view to face the player's torso.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TorsoConfig {
    /// On foot, turn the character to face the player's torso under the view, which keeps its
    /// heading: only the turn input turns the view. The character turns once the torso is
    /// [`FOLLOW_DEADBAND_DEG`](crate::controllers::body::rig::torso::FOLLOW_DEADBAND_DEG) from its
    /// facing.
    pub follow: bool,
}

impl TorsoConfig {
    pub const fn new() -> Self {
        Self { follow: true }
    }
}

impl Default for TorsoConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The legs: the walk while the body moves, and the standing steps while it does not.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LegsConfig {
    /// Walk the feet while the body moves; off, they only step standing.
    pub gait: bool,
    /// The measured speed above which the feet start walking, metres per second.
    pub moving_mps: f32,
    /// The measured speed below which they stand again, metres per second.
    pub standing_mps: f32,
    /// How far a standing foot may be from its rest place before it steps, metres.
    pub rest_error_m: f32,
    /// How far a standing foot may be turned from its rest orientation before it steps, degrees.
    pub turn_error_deg: f32,
    /// How long a standing step takes, seconds.
    pub step_s: f32,
    /// How high a standing step lifts the foot, metres.
    pub step_height_m: f32,
    /// How far past the footprints the standing support reaches, metres.
    pub support_margin_m: f32,
    /// The capture term's weight in a walking landing.
    pub capture_gain: f32,
    /// The furthest a foot lands from its hip, as a share of the leg's length.
    pub reach_share: f32,
}

impl LegsConfig {
    pub const fn new() -> Self {
        Self {
            gait: true,
            moving_mps: 0.35,
            standing_mps: 0.15,
            rest_error_m: 0.22,
            turn_error_deg: 35.0,
            step_s: 0.25,
            step_height_m: 0.06,
            support_margin_m: 0.04,
            capture_gain: 0.5,
            reach_share: 0.42,
        }
    }
}

impl Default for LegsConfig {
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
        bad.legs.step_s = f32::NAN;
        assert!(bad.validate().is_err());
        bad = RigConfig::new();
        bad.legs.reach_share = 2.0;
        assert!(
            bad.validate()
                .unwrap_err()
                .0
                .contains("body.rig.legs.reach_share")
        );
    }
}
