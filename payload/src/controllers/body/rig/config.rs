//! The body rig's settings, under `controllers.body.rig`.

use serde::{Deserialize, Serialize};

use crate::config::validate::Invalid;

/// The body rig: who moves the root, the chase that keeps it under the physical head, the lean the
/// chase leaves alone, the push-back at walls, the torso, and the pelvis (`docs/mod/body/rig.md`).
/// All default on.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigConfig {
    /// The body walking after the player's head in the standing posture.
    pub chase: ChaseConfig,
    /// The lean over the feet that the chase leaves alone.
    pub lean: LeanConfig,
    /// Holding the view at a wall the body cannot pass.
    pub pushback: PushbackConfig,
    /// The player's torso, and the character turning to face it.
    pub torso: TorsoConfig,
    /// The pelvis leaning into the body's motion.
    pub pelvis: PelvisConfig,
}

impl RigConfig {
    pub const fn new() -> Self {
        Self {
            chase: ChaseConfig::new(),
            lean: LeanConfig::new(),
            pushback: PushbackConfig::new(),
            torso: TorsoConfig::new(),
            pelvis: PelvisConfig::new(),
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
            (
                "pushback.obstructed_m",
                self.pushback.obstructed_m,
                0.0,
                1.0,
            ),
            ("pushback.authored_m", self.pushback.authored_m, 0.0, 2.0),
            ("pushback.reach_m", self.pushback.reach_m, 0.0, 2.0),
            ("pushback.min_gap_m", self.pushback.min_gap_m, 0.0, 1.0),
            ("pushback.debt_cap_m", self.pushback.debt_cap_m, 0.0, 2.0),
            ("pushback.fade_span_m", self.pushback.fade_span_m, 0.01, 1.0),
            ("pushback.reset_s", self.pushback.reset_s, 0.0, 10.0),
            (
                "torso.follow_deadband_deg",
                self.torso.follow_deadband_deg,
                0.0,
                90.0,
            ),
            (
                "torso.chest_limit_deg",
                self.torso.chest_limit_deg,
                0.0,
                180.0,
            ),
            ("torso.head_weight", self.torso.head_weight, 0.0, 5.0),
            (
                "torso.neck_limit_deg",
                self.torso.neck_limit_deg,
                0.0,
                180.0,
            ),
            ("torso.follow_s", self.torso.follow_s, 0.0, 2.0),
            ("pelvis.follow_s", self.pelvis.follow_s, 0.0, 1.0),
            ("pelvis.hinge_share", self.pelvis.hinge_share, 0.0, 1.0),
            ("pelvis.speed_lean", self.pelvis.speed_lean, 0.0, 1.0),
            (
                "pelvis.max_speed_lean",
                self.pelvis.max_speed_lean,
                0.0,
                1.0,
            ),
            ("pelvis.accel_lean", self.pelvis.accel_lean, 0.0, 1.0),
            (
                "pelvis.max_accel_lean",
                self.pelvis.max_accel_lean,
                0.0,
                1.0,
            ),
            ("pelvis.max_roll", self.pelvis.max_roll, 0.0, 1.0),
            (
                "pelvis.motion_smoothing_s",
                self.pelvis.motion_smoothing_s,
                0.0,
                2.0,
            ),
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

/// The push-back: where the body cannot follow the head (a wall, a table's edge), the view is held
/// at a threshold past the root by sliding the cockpit back, up to a cap per episode, and fades
/// to black past the cap.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PushbackConfig {
    /// On by default. Off, the view goes through the wall and only the fade answers it.
    pub enabled: bool,
    /// How far past the root the view may go while the player's chase is obstructed, metres.
    pub obstructed_m: f32,
    /// How far while the animation carries the body and the chase is suspended, metres: only a
    /// runaway gap.
    pub authored_m: f32,
    /// How far while climbing or seated, metres: about an arm's-length lean.
    pub reach_m: f32,
    /// The smallest gap that counts as obstructed, metres.
    pub min_gap_m: f32,
    /// The most the view is slid back in one episode, metres; past it the view fades.
    pub debt_cap_m: f32,
    /// How far past the threshold, with the cap spent, the fade reaches black, metres.
    pub fade_span_m: f32,
    /// How long the gap must stay under half the threshold for the episode to end, seconds.
    pub reset_s: f32,
}

impl PushbackConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            obstructed_m: 0.10,
            authored_m: 0.30,
            reach_m: 0.45,
            min_gap_m: 0.05,
            debt_cap_m: 0.30,
            fade_span_m: 0.15,
            reset_s: 0.2,
        }
    }
}

impl Default for PushbackConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The player's torso: its heading from the head and the hands, the character turning under the
/// view to face it, and the chest turning toward it.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TorsoConfig {
    /// On foot, turn the character to face the player's torso under the view, which keeps its
    /// heading: only the turn input turns the view.
    pub follow: bool,
    /// How far the torso may turn from the character's facing before the character turns, degrees.
    /// The chest turn covers what is left.
    pub follow_deadband_deg: f32,
    /// The furthest the chest is turned from the character's facing, degrees.
    pub chest_limit_deg: f32,
    /// How much the head's heading counts against the hands', which count up to one each.
    pub head_weight: f32,
    /// The furthest the head may be turned from the torso, degrees.
    pub neck_limit_deg: f32,
    /// How quickly the heading follows, seconds (the time constant of a critically damped filter).
    pub follow_s: f32,
}

impl TorsoConfig {
    pub const fn new() -> Self {
        Self {
            follow: true,
            follow_deadband_deg: 10.0,
            chest_limit_deg: 45.0,
            head_weight: 0.3,
            neck_limit_deg: 65.0,
            follow_s: 0.08,
        }
    }
}

impl Default for TorsoConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The pelvis: held under the neck pivot by a stiff spring, hinged back by a share of the lean, and
/// leaning into the body's motion.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PelvisConfig {
    /// How quickly the pelvis follows its target, seconds (the time constant of a critically damped
    /// spring): short, so a quick lean bends the spine first and the pelvis is otherwise held.
    pub follow_s: f32,
    /// How far back the pelvis goes per metre of lean, as a share: the hip hinge.
    pub hinge_share: f32,
    /// The forward lean per metre per second of speed, radians.
    pub speed_lean: f32,
    /// The most the speed leans the body, radians.
    pub max_speed_lean: f32,
    /// The forward lean per metre per second squared of acceleration, radians.
    pub accel_lean: f32,
    /// The most the acceleration leans the body either way, radians.
    pub max_accel_lean: f32,
    /// The most a sideways acceleration rolls the body, radians.
    pub max_roll: f32,
    /// How much the measured velocity and acceleration are smoothed, seconds.
    pub motion_smoothing_s: f32,
}

impl PelvisConfig {
    pub const fn new() -> Self {
        Self {
            follow_s: 0.06,
            hinge_share: 0.3,
            speed_lean: 0.035,
            max_speed_lean: 0.14,
            accel_lean: 0.02,
            max_accel_lean: 0.1,
            max_roll: 0.08,
            motion_smoothing_s: 0.1,
        }
    }
}

impl Default for PelvisConfig {
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
