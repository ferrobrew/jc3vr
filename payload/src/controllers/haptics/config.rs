//! The haptics settings: the master switch and strength, one switch per category of source, the
//! game's rumble, the vocabulary's shapes, and the mixer's rules.

use serde::{Deserialize, Serialize};

use crate::controllers::haptics::{cues::Category, mixer::MixConfig, vocabulary::Vocabulary};

/// Controller vibration (see [`crate::controllers::haptics`]), as `controllers.haptics`.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HapticsConfig {
    /// Off, nothing vibrates.
    pub enabled: bool,
    /// Scales every vibration's amplitude, `0..=1`.
    pub intensity: f32,
    /// The categories of the mod's own cues.
    pub categories: Categories,
    /// Forward the game's rumble to the controllers: a weapon's own fire rumble to the hand holding
    /// that gun, and the game's per-frame left and right motor strength to the hands by side.
    pub game_rumble: bool,
    /// Also let the physical gamepad rumble under the controller scheme, instead of silencing it
    /// so only the controllers vibrate.
    pub gamepad_rumble: bool,
    /// Send each primitive's frequency hint; off leaves every vibration at the runtime's default
    /// frequency.
    pub frequencies: bool,
    /// How much a shot's recoil kick adds to its pulse's amplitude, per radian of kick.
    pub kick_amplitude_per_rad: f32,
    /// The longest a weapon's fire rumble plays on a controller, seconds: the game's rumbles can
    /// last long enough to blur into one another.
    pub max_shot_s: f32,
    /// The primitives' shapes.
    pub vocabulary: Vocabulary,
    /// How the cues reaching a hand mix.
    pub mix: MixConfig,
}

impl HapticsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            intensity: 1.0,
            categories: Categories::new(),
            game_rumble: true,
            gamepad_rumble: false,
            frequencies: true,
            kick_amplitude_per_rad: 6.0,
            max_shot_s: 0.4,
            vocabulary: Vocabulary::new(),
            mix: MixConfig::new(),
        }
    }

    /// Whether `category` is switched on, the master switch aside.
    pub const fn category(&self, category: Category) -> bool {
        match category {
            Category::Contact => self.categories.contact,
            Category::Weapons => self.categories.weapons,
            Category::Vehicles => self.categories.vehicles,
            Category::Traversal => self.categories.traversal,
            Category::Interface => self.categories.interface,
            Category::Game => self.game_rumble,
        }
    }
}

impl Default for HapticsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// One switch per category of the mod's cues.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Categories {
    pub contact: bool,
    pub weapons: bool,
    pub vehicles: bool,
    pub traversal: bool,
    pub interface: bool,
}

impl Categories {
    pub const fn new() -> Self {
        Self {
            contact: true,
            weapons: true,
            vehicles: true,
            traversal: true,
            interface: true,
        }
    }
}

impl Default for Categories {
    fn default() -> Self {
        Self::new()
    }
}
