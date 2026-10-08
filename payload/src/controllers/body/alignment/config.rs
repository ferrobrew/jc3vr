//! The body alignment's settings, under `controllers.body.alignment`.

use serde::{Deserialize, Serialize};

use crate::controllers::body::torso::TorsoConfig;

/// Keeping the character's body aligned with the player's: the torso estimate, the body turning
/// under the view to face the player's torso, and the chest turned toward it. The estimate only
/// measures and feeds the readouts and the holsters' placement, so it defaults on; everything that
/// moves the character defaults off until it has been checked in the headset.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlignmentConfig {
    /// Estimate the player's torso from the head and hands while the headset drives the view.
    pub estimate: bool,
    /// How the estimate behaves.
    pub torso: TorsoConfig,
    /// On foot, turn the character to face the player's torso under the view, which keeps its
    /// heading: only the turn input turns the view. Needs the estimate.
    pub root_follow: bool,
    /// How far the torso may turn from the character's facing before the character turns, degrees.
    /// The chest turn covers what is left.
    pub follow_deadband_deg: f32,
    /// Turn the character's chest toward the player's torso with the body IK, in place of the
    /// share of the head's turn it takes otherwise. Needs the estimate and the body IK.
    pub drive_chest: bool,
    /// How much of the torso's turn from the character's facing the chest takes, `0..=1`.
    pub chest_weight: f32,
    /// The furthest the chest is turned from the character's facing, degrees.
    pub chest_limit_deg: f32,
    /// How much of the torso's turn from the character's facing the squared stance's pelvis and
    /// feet take while it squares a standing character, `0..=1`. Needs the squared stance and
    /// [`drive_chest`](Self::drive_chest).
    pub stance_turn_share: f32,
    /// Estimate the player's shoulders for the hands' arm remap from the torso's heading rather
    /// than the head's, so that looking over a shoulder does not swing the arms' pivots with it.
    pub torso_shoulders: bool,
}

impl AlignmentConfig {
    pub const fn new() -> Self {
        Self {
            estimate: true,
            torso: TorsoConfig::new(),
            root_follow: true,
            follow_deadband_deg: 10.0,
            drive_chest: true,
            chest_weight: 1.0,
            chest_limit_deg: 45.0,
            stance_turn_share: 1.0,
            torso_shoulders: true,
        }
    }
}

impl Default for AlignmentConfig {
    fn default() -> Self {
        Self::new()
    }
}
