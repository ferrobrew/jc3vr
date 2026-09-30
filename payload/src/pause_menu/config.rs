//! The VR menu's configuration.

use serde::{Deserialize, Serialize};

/// VR-menu settings. See `docs/mod/pause-menu.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PauseMenuConfig {
    /// Add the VR pages to the pause menu's options menu. Read when the menu opens, so a change
    /// takes effect the next time it does. Off by default until the menu is verified in game.
    pub enabled: bool,
}

impl PauseMenuConfig {
    pub const fn new() -> Self {
        Self { enabled: false }
    }
}

impl Default for PauseMenuConfig {
    fn default() -> Self {
        Self::new()
    }
}
