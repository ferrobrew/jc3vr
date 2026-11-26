//! Physical weapon handling's settings.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

use crate::controllers::weapons::{
    ads::config::AdsConfig,
    config::{HoldConfig, HolstersConfig, RecoilConfig, SnatchConfig},
    inventory::config::InventoryConfig,
    reload::config::ReloadConfig,
    wheel::config::WheelConfig,
};

/// The weapon-handling settings, at `controllers.weapons` in the settings file; the payload binds
/// the slot at startup.
pub static WEAPONS_CONFIG: ConfigSlot<WeaponsConfig> = ConfigSlot::new("controllers.weapons");

/// Physical weapon handling. Acts only under the controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WeaponsConfig {
    /// Draw and dock weapons with a grip at the body's sockets.
    pub holsters: HolstersConfig,
    /// The recoil offset between the controller and the rendered weapon.
    pub recoil: RecoilConfig,
    /// Make the two pistols two weapons: the pair the game hands over becomes the slot's pistol
    /// and a second one carried on the other hip, each its own item, picked up, held, docked,
    /// dropped, reloaded, and fired on its own, and a pistol taken from the world is one pistol.
    /// Needs the VR inventory and its carrying; without them, or while the second pistol has not
    /// come, the pistols stay the game's pair, one item with a gun in each hand.
    pub separate_pistols: bool,
    /// The hands' hold: whether a gun stays in the hand only while its grip is held.
    pub hold: HoldConfig,
    /// The true reload: eject, load from the belt pouch, and chamber by hand, in place of the
    /// game's timed reload.
    pub reload: ReloadConfig,
    /// Let the game switch to another weapon when the trigger of an empty weapon that cannot
    /// reload is pulled. Off, the empty weapon stays in the hand until it is holstered or another
    /// is drawn, and the pull only dry-fires.
    pub auto_switch_when_empty: bool,
    /// The weapon wheel: the game's selection cross, opened with a held input and steered by the
    /// hand.
    pub wheel: WheelConfig,
    /// Aiming a long gun down its sights: the sights onto the eye, the stock into the shoulder,
    /// and a scope's cheek weld.
    pub ads: AdsConfig,
    /// The VR inventory: the weapons the player owns, holds, and lets go of, and the ones kept
    /// from the game's despawn.
    pub inventory: InventoryConfig,
    /// The grapple snatch: a grip of the empty grapple hand at a weapon out of reach, and a yank,
    /// pull it into the hand on the game's hook.
    pub snatch: SnatchConfig,
}

impl WeaponsConfig {
    pub const fn new() -> Self {
        Self {
            holsters: HolstersConfig::new(),
            recoil: RecoilConfig::new(),
            separate_pistols: true,
            hold: HoldConfig::new(),
            reload: ReloadConfig::new(),
            auto_switch_when_empty: false,
            wheel: WheelConfig::new(),
            ads: AdsConfig::new(),
            inventory: InventoryConfig::new(),
            snatch: SnatchConfig::new(),
        }
    }
}

impl Default for WeaponsConfig {
    fn default() -> Self {
        Self::new()
    }
}
