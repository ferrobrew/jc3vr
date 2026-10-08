//! Physical weapon handling's settings.

use serde::{Deserialize, Serialize};

use crate::controllers::weapons::{
    ads::config::AdsConfig,
    collide::config::CollideConfig,
    config::{
        C4Config, DualWieldConfig, ForegripConfig, GrenadeConfig, HeftConfig, HoldConfig,
        HolstersConfig, PouchesConfig, RecoilConfig, ScopeConfig,
    },
    inventory::config::InventoryConfig,
    reload::config::ReloadConfig,
    wheel::config::WheelConfig,
};

/// Physical weapon handling. Acts only under the controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WeaponsConfig {
    /// Draw and holster weapons with a grip press at the body holsters.
    pub holsters: HolstersConfig,
    /// The pouches on the front of the body: the magazines, the grenades, and the C4.
    pub pouches: PouchesConfig,
    /// Let the off-hand grab a two-handed weapon's foregrip, which then sets the weapon's
    /// direction.
    pub foregrip: ForegripConfig,
    /// Hold the fire inputs back while a reload, a draw, or a holster plays (the action-runs hand
    /// tier), so the weapon cannot fire for the action's duration.
    pub block_fire_during_actions: bool,
    /// The recoil offset between the controller and the rendered weapon.
    pub recoil: RecoilConfig,
    /// The one-handed weight of long guns: sag, lag, sway, a stronger kick, and a wider spread,
    /// all gone while the off-hand braces the weapon on its foregrip.
    pub heft: HeftConfig,
    /// Dual-wielded pistols split onto both hands, each fired by its own trigger and aimed by its
    /// own hand.
    pub dual_wield: DualWieldConfig,
    /// Which hand holds the weapon: the hand that drew it, picked it up, or was handed it.
    pub hold: HoldConfig,
    /// The true reload: eject, load from the belt pouch, and chamber by hand, in place of the
    /// game's timed reload.
    pub reload: ReloadConfig,
    /// Throw grenades physically: grab from the chest pouch, release the grip to throw with the
    /// hand's velocity. The grenade pouch exists only while this is on.
    pub grenade_throw: bool,
    /// The physical throw's release, warning, and fallback settings.
    pub grenade: GrenadeConfig,
    /// Throw C4 physically: grab a charge from the chest pouch, release the grip to throw it with
    /// the hand's velocity, and it sticks where it lands. The C4 pouch exists only while this is
    /// on.
    pub c4_throw: bool,
    /// Detonate the placed charges by pushing the dominant index fingertip into the button on the
    /// inside of the off-hand's wrist.
    pub wrist_detonator: bool,
    /// The C4 pouch, the throw's release, and the wrist detonator's placement and press tuning.
    pub c4: C4Config,
    /// Draw a magnified image on a scoped weapon's lens while the scope is at an eye, in place of
    /// the game's precision-aim zoom, which is suppressed separately
    /// ([`ScopeConfig::suppress_precision_aim`]).
    pub scopes: bool,
    /// The scopes' precision-aim suppression and the lens's tuning.
    pub scope: ScopeConfig,
    /// Let the game switch to another weapon when the trigger of an empty weapon that cannot
    /// reload is pulled. Off, the empty weapon stays in the hand until it is holstered or another
    /// is drawn, and the pull only dry-fires.
    pub auto_switch_when_empty: bool,
    /// Let the fire input draw the selected weapon with nothing in the hands, as the game does.
    /// Off, a weapon comes out only from a holster, the weapon wheel, or a slot select, and the
    /// precision-aim input does not draw either.
    pub fire_draws_weapon: bool,
    /// The weapon wheel: the game's selection cross, opened with a held input and steered by the
    /// hand.
    pub wheel: WheelConfig,
    /// Aiming a long gun down its sights: the sights onto the eye, the stock into the shoulder,
    /// and a scope's cheek weld.
    pub ads: AdsConfig,
    /// Held long guns stop at walls: turned aside and pulled back into the hands.
    pub collide: CollideConfig,
    /// The VR inventory: the weapons the player owns, holds, and lets go of, and the ones kept
    /// from the game's despawn.
    pub inventory: InventoryConfig,
}

impl WeaponsConfig {
    pub const fn new() -> Self {
        Self {
            holsters: HolstersConfig::new(),
            pouches: PouchesConfig::new(),
            foregrip: ForegripConfig::new(),
            block_fire_during_actions: true,
            recoil: RecoilConfig::new(),
            heft: HeftConfig::new(),
            dual_wield: DualWieldConfig::new(),
            hold: HoldConfig::new(),
            reload: ReloadConfig::new(),
            grenade_throw: true,
            grenade: GrenadeConfig::new(),
            c4_throw: true,
            wrist_detonator: true,
            c4: C4Config::new(),
            scopes: true,
            scope: ScopeConfig::new(),
            auto_switch_when_empty: false,
            fire_draws_weapon: false,
            wheel: WheelConfig::new(),
            ads: AdsConfig::new(),
            collide: CollideConfig::new(),
            inventory: InventoryConfig::new(),
        }
    }
}

impl Default for WeaponsConfig {
    fn default() -> Self {
        Self::new()
    }
}
