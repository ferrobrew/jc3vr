//! Body scale remapping between the player's body and the character's.

use jc3vr_core::config::ConfigSlot;
use serde::{Deserialize, Serialize};

use crate::controllers::body::{estimator::EstimatorConfig, ik_body::IkBodyConfig, rig::RigConfig};

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
    /// The body rig: the step drive's switch and the pure core's tuning.
    pub rig: RigConfig,
    /// The learned body estimator: whether it runs and its weights.
    pub estimator: EstimatorConfig,
    /// The IK-first on-foot body: the firm ragdoll's target switch and the feet's contacts.
    pub ik: IkBodyConfig,
}

impl BodyRemapConfig {
    pub const fn new() -> Self {
        Self {
            remap_head: true,
            remap_hands: true,
            rig: RigConfig::new(),
            estimator: EstimatorConfig::new(),
            ik: IkBodyConfig::new(),
        }
    }
}

impl Default for BodyRemapConfig {
    fn default() -> Self {
        Self::new()
    }
}
