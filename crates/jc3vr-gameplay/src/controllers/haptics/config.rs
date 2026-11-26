//! The haptics settings: the master switch and strength, and the game's rumble.

use serde::{Deserialize, Serialize};

use crate::controllers::haptics::cues::Category;

/// Controller vibration (see [`crate::controllers::haptics`]), as `controllers.haptics`. The
/// primitives' shapes are fixed at [`VOCABULARY`](crate::controllers::haptics::vocabulary::VOCABULARY)
/// and the mixer's rules at [`MIX`](crate::controllers::haptics::mixer::MIX); every category of the
/// mod's own cues plays, each with its primitive's frequency.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HapticsConfig {
    /// Off, nothing vibrates.
    pub enabled: bool,
    /// Scales every vibration's amplitude, `0..=1`.
    pub intensity: f32,
    /// Forward the game's rumble to the controllers: a weapon's own fire rumble to the hand holding
    /// that gun, and the game's per-frame left and right motor strength to the hands by side.
    pub game_rumble: bool,
    /// Also let the physical gamepad rumble under the controller scheme, instead of silencing it
    /// so only the controllers vibrate.
    pub gamepad_rumble: bool,
}

impl HapticsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            intensity: 1.0,
            game_rumble: true,
            gamepad_rumble: false,
        }
    }

    /// Whether `category` plays, the master switch aside: the game's rumble by its switch, and
    /// every category of the mod's own cues always.
    pub const fn category(&self, category: Category) -> bool {
        match category {
            Category::Game => self.game_rumble,
            Category::Contact
            | Category::Weapons
            | Category::Vehicles
            | Category::Traversal
            | Category::Interface => true,
        }
    }
}

impl Default for HapticsConfig {
    fn default() -> Self {
        Self::new()
    }
}
