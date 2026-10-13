//! Body scale remapping between the player's body and the character's.

use serde::{Deserialize, Serialize};

use crate::controllers::body::{
    alignment::AlignmentConfig, procedural::config::ProceduralBodyConfig, rig::RigConfig,
};

/// Body scale remapping between the player's body and the character's (see
/// [`crate::controllers::body`]).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BodyRemapConfig {
    /// Place the eyes at the character's eye height and scale vertical head travel by the ratio of
    /// the heights. On by default, as part of the full gameplay preset; it moves the view for
    /// every player, controllers or not.
    pub remap_head: bool,
    /// Scale the hands' offsets from the shoulders by the ratio of the arm spans. Acts only under
    /// the controller scheme.
    pub remap_hands: bool,
    /// Keeping the character's body aligned with the player's: the torso estimate, the body
    /// turning under the view, and the chest turn.
    pub alignment: AlignmentConfig,
    /// The procedural body: Rico's pose built from the player's head and body.
    pub procedural: ProceduralBodyConfig,
    /// The body rig: who moves the root, the chase after the player's head, and the lean.
    pub rig: RigConfig,
}

impl BodyRemapConfig {
    pub const fn new() -> Self {
        Self {
            remap_head: true,
            remap_hands: true,
            alignment: AlignmentConfig::new(),
            procedural: ProceduralBodyConfig::new(),
            rig: RigConfig::new(),
        }
    }
}

impl Default for BodyRemapConfig {
    fn default() -> Self {
        Self::new()
    }
}
