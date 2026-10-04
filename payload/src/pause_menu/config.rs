//! The VR menu's configuration.

use serde::{Deserialize, Serialize};

/// VR-menu settings. See `docs/mod/pause-menu.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PauseMenuConfig {
    /// Add the VR entry, with a tab per VR page, to the pause menu's root menu. Read when the
    /// menu opens, so a change takes effect the next time it does.
    pub enabled: bool,
}

impl PauseMenuConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for PauseMenuConfig {
    fn default() -> Self {
        Self::new()
    }
}
