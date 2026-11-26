//! Physical weapon handling's settings.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

use crate::controllers::weapons::{
    ads::config::AdsConfig,
    config::{HoldConfig, HolstersConfig, RecoilConfig, SnatchConfig},
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
    /// The hands' hold: whether a gun stays in the hand only while its grip is held.
    pub hold: HoldConfig,
    /// The true reload: eject, load from the belt pouch, and chamber by hand, in place of the
    /// game's timed reload.
    pub reload: ReloadConfig,
    /// The weapon wheel: the game's selection cross, opened with a held input and steered by the
    /// hand.
    pub wheel: WheelConfig,
    /// Aiming a long gun down its sights: the sights onto the eye, the stock into the shoulder,
    /// and a scope's cheek weld.
    pub ads: AdsConfig,
    /// The grapple snatch: a grip of the empty grapple hand at a weapon out of reach, and a yank,
    /// pull it into the hand on the game's hook.
    pub snatch: SnatchConfig,
}

impl WeaponsConfig {
    pub const fn new() -> Self {
        Self {
            holsters: HolstersConfig::new(),
            recoil: RecoilConfig::new(),
            hold: HoldConfig::new(),
            reload: ReloadConfig::new(),
            wheel: WheelConfig::new(),
            ads: AdsConfig::new(),
            snatch: SnatchConfig::new(),
        }
    }
}

impl Default for WeaponsConfig {
    fn default() -> Self {
        Self::new()
    }
}
