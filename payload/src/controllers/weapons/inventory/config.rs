//! The settings of the VR inventory (see [`crate::controllers::weapons::inventory`]), held in
//! [`WeaponsConfig::inventory`](crate::controllers::config::WeaponsConfig::inventory).

use serde::{Deserialize, Serialize};

use crate::controllers::weapons::inventory::pins::PinSettings;

/// The VR inventory. Acts only under the controller scheme.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InventoryConfig {
    /// Track the weapons the player owns, holds, and lets go of: the registry everything else here
    /// builds on.
    pub enabled: bool,
    /// Keep the weapons the player let go of nearby from the game's despawn while they are near.
    pub pin_world: bool,
    /// A weapon in the world this near the player is kept, metres.
    pub pin_radius_m: f32,
    /// A kept weapon is let go of once it is this far away, metres.
    pub unpin_radius_m: f32,
    /// The most weapons in the world kept at once, the nearest first.
    pub max_pinned: usize,
    /// A weapon in the world this far away and not kept is left to the game, metres.
    pub forget_m: f32,
}

impl InventoryConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            pin_world: true,
            pin_radius_m: 8.0,
            unpin_radius_m: 12.0,
            max_pinned: 6,
            forget_m: 60.0,
        }
    }

    /// The pin policy these settings give.
    pub const fn pins(&self) -> PinSettings {
        PinSettings {
            enabled: self.pin_world,
            radius_m: self.pin_radius_m,
            release_m: self.unpin_radius_m,
            max_pinned: self.max_pinned,
            forget_m: self.forget_m,
            max_tracked: MAX_TRACKED,
        }
    }
}

impl Default for InventoryConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The most weapons in the world the registry keeps.
const MAX_TRACKED: usize = 24;
