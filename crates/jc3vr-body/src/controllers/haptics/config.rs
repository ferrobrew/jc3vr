//! The haptics settings: the master switch and strength, and the game's rumble.

use jc3vr_core::config::engine::ConfigSlot;
use jc3vr_input::haptics::cues::Category;
use serde::{Deserialize, Serialize};

/// The controller vibration settings, at `controllers.haptics` in the settings file; the payload
/// binds the slot at startup.
pub static HAPTICS_CONFIG: ConfigSlot<HapticsConfig> = ConfigSlot::new("controllers.haptics");

/// Controller vibration (see [`crate::controllers::haptics`]), as `controllers.haptics`. The
/// primitives' shapes, the mixer's rules, and the cues are tuning
/// ([`HAPTICS`](jc3vr_view::tuning::HAPTICS)); every category of the mod's own cues plays, each
/// with its primitive's frequency.
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
}

impl HapticsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            intensity: 1.0,
            game_rumble: true,
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
