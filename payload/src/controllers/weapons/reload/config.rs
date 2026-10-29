//! The true reload's settings (see [`crate::controllers::weapons::reload`]), held in
//! [`WeaponsConfig::reload`](crate::controllers::config::WeaponsConfig::reload). The fixed tuning
//! is [`RELOAD`](crate::controllers::weapons::reload::tuning::RELOAD).

use serde::{Deserialize, Serialize};

/// The true reload. Acts only under the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReloadConfig {
    /// Reload by hand: the game's own reloads of the weapons the mod reloads are stopped, and a
    /// magazine is ejected, taken from the belt pouch, seated in the well, and chambered by hand.
    /// Off, the game's timed reload plays from the reload button as it does on a gamepad.
    pub enabled: bool,
    /// Eject a gun's magazine with a sharp flick of the hand holding it.
    pub flick_eject: bool,
    /// The one-handed belt reload: push the gun's well down onto the pouch.
    pub belt: BeltConfig,
    /// The slide, charging handle, bolt, and pump.
    pub action: ActionConfig,
    /// Ejected magazines that stay in the world with their rounds.
    pub loose: LooseMagazineConfig,
    /// Grip-and-pull removal of an inserted detachable magazine.
    pub extraction: ExtractionConfig,
}

impl ReloadConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            flick_eject: false,
            belt: BeltConfig::new(),
            action: ActionConfig::new(),
            loose: LooseMagazineConfig::new(),
            extraction: ExtractionConfig::new(),
        }
    }
}

impl Default for ReloadConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Magazines in the world (see [`crate::controllers::weapons::reload::loose`]). Acts only under the
/// controller scheme with the reload on, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LooseMagazineConfig {
    /// An ejected magazine keeps the rounds it held as an object in the world, and a dropped
    /// magazine that fits the held gun can be picked up and seated with its rounds.
    pub enabled: bool,
}

impl LooseMagazineConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for LooseMagazineConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Grip-and-pull removal of a detachable magazine, with no release-button latch.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtractionConfig {
    pub enabled: bool,
    /// The inserted magazine's grab radius, metres at the player's scale.
    pub grab_radius_m: f32,
    /// The outward hand travel required to detach the magazine, metres at the player's scale.
    pub pull_m: f32,
    /// How far the hand may move sideways from its acquired path, metres at the player's scale.
    pub lateral_tolerance_m: f32,
}

impl ExtractionConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            grab_radius_m: 0.06,
            pull_m: 0.08,
            lateral_tolerance_m: 0.06,
        }
    }
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The one-handed belt reload (see [`crate::controllers::weapons::reload::belt`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeltConfig {
    pub enabled: bool,
}

impl BeltConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for BeltConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The slide, charging handle, bolt, and pump (see
/// [`crate::controllers::weapons::reload::chamber`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionConfig {
    /// The action is a part that follows the hand through its travel: a slide or a charging handle
    /// runs home on its spring when let go, a slide catches at the rear on an empty magazine, a
    /// pump stays where it is left, and a bolt turns up before it slides. Off, the action is a
    /// gesture: a pull to the pull share and a release or a push home.
    pub physical: bool,
}

impl ActionConfig {
    pub const fn new() -> Self {
        Self { physical: true }
    }
}

impl Default for ActionConfig {
    fn default() -> Self {
        Self::new()
    }
}
