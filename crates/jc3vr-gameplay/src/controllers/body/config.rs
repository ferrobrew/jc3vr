//! Body scale remapping between the player's body and the character's.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

use crate::controllers::body::{
    estimator::EstimatorConfig, ik_body::IkBodyConfig, procedural::config::ProceduralBodyConfig,
    rig::RigConfig, unarmed::UnarmedBodyConfig,
};

/// The body remap settings, at `controllers.body` in the settings file; the payload binds the slot
/// at startup.
pub static BODY_REMAP_CONFIG: ConfigSlot<BodyRemapConfig> = ConfigSlot::new("controllers.body");

/// Body scale remapping between the player's body and the character's (see
/// [`crate::controllers::body`]).
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BodyRemapConfig {
    /// Place the eyes at the character's eye height and scale vertical head travel by the ratio of
    /// the heights. On by default, as part of the full gameplay preset; it moves the view for
    /// every player, controllers or not.
    pub remap_head: bool,
    /// Scale the hands' offsets from the shoulders by the ratio of the arm spans. Acts only under
    /// the controller scheme.
    pub remap_hands: bool,
    /// The procedural body: Rico's pose built from the player's head and body.
    pub procedural: ProceduralBodyConfig,
    /// The body rig: who moves the root, the chase after the player's head, the lean, the
    /// push-back, and the torso.
    pub rig: RigConfig,
    /// The learned body estimator: whether it runs, its weights, and how often.
    pub estimator: EstimatorConfig,
    /// The IK-first on-foot body: the lower body posed from the estimate while Rico stands.
    pub ik: IkBodyConfig,
    /// The weapon-agnostic body: which of the game's weapon animations reach the body on foot.
    pub unarmed: UnarmedBodyConfig,
}

impl BodyRemapConfig {
    pub const fn new() -> Self {
        Self {
            remap_head: true,
            remap_hands: true,
            procedural: ProceduralBodyConfig::new(),
            rig: RigConfig::new(),
            estimator: EstimatorConfig::new(),
            ik: IkBodyConfig::new(),
            unarmed: UnarmedBodyConfig::new(),
        }
    }
}

impl Default for BodyRemapConfig {
    fn default() -> Self {
        Self::new()
    }
}
