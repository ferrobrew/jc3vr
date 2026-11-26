//! The stunt positions' settings.

use serde::{Deserialize, Serialize};

/// Deliberate roof-mantle entry and grip-open exit from the game's existing helicopter hang
/// (see [`crate::controllers::physical_hands::stunt`]). Physical grips retain their handholds
/// through the hand climb instead of requesting the animated hang.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StuntConfig {
    /// Hold stunt positions with the grip.
    pub enabled: bool,
}

impl StuntConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for StuntConfig {
    fn default() -> Self {
        Self::new()
    }
}
