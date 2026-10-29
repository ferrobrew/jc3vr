//! The weapon-handling settings (the sockets, the pouches, the foregrip, the recoil, the weight, the
//! hands' hold, the grenade throw, and the C4 throw), held in
//! [`WeaponsConfig`](crate::controllers::config::WeaponsConfig) beside the other weapon-handling
//! subsystems' toggles. The reload's are in [`crate::controllers::weapons::reload::config`]. Each
//! feature keeps its switches here; its fixed tuning is a constant in the module that owns it.

use serde::{Deserialize, Serialize};

/// The body's sockets (see [`crate::controllers::weapons::docks`]), whose fixed tuning is
/// [`SOCKETS`](crate::controllers::weapons::volumes::SOCKETS). They act only under the controller
/// scheme.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HolstersConfig {
    /// Keep the move stick's click cycling to the next weapon beside the sockets, as a fallback for
    /// players who cannot reach them. With a gun in the off-hand the click retracts the tethers
    /// instead.
    pub stick_click_cycles: bool,
}

impl HolstersConfig {
    pub const fn new() -> Self {
        Self {
            stick_click_cycles: true,
        }
    }
}

impl Default for HolstersConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The pouches on the front of the body (see [`crate::controllers::weapons::volumes`]), whose fixed
/// placement is [`POUCHES`](crate::controllers::weapons::volumes::POUCHES).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PouchesConfig {
    /// The game's models of what the pouches hold, drawn on them.
    pub models: PouchModelsConfig,
}

impl PouchesConfig {
    pub const fn new() -> Self {
        Self {
            models: PouchModelsConfig::new(),
        }
    }
}

impl Default for PouchesConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The game's models of what the pouches hold, drawn on the body where the pouches are (see
/// [`crate::controllers::weapons::pouch_models`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PouchModelsConfig {
    pub enabled: bool,
}

impl PouchModelsConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for PouchModelsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The two-handed weapon's foregrip (see [`crate::controllers::weapons::foregrip`]). Acts only under
/// the controller scheme, so it defaults on. Its fixed tuning is
/// [`FOREGRIP`](crate::controllers::weapons::foregrip::FOREGRIP).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ForegripConfig {
    /// Let the off-hand grab a two-handed weapon's foregrip, which then sets the weapon's direction.
    pub enabled: bool,
}

impl ForegripConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for ForegripConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The recoil offset (see [`crate::controllers::weapons::recoil`]): each shot kicks the gun hand
/// away from the controller pose, and a damped spring returns it. Acts only under the controller
/// scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecoilConfig {
    pub enabled: bool,
    /// Scales the kick taken from the game's per-shot recoil.
    pub kick_scale: f32,
}

impl RecoilConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            kick_scale: 1.0,
        }
    }
}

impl Default for RecoilConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The weight of a long weapon (see [`crate::controllers::weapons::heft`]): held in one hand, a
/// long gun sags, trails the hand on a spring, and sways, its kick grows, and its spread widens,
/// all scaled by the weapon's class; braced on the foregrip, it keeps a lighter weight of its own.
/// Acts only under the controller scheme, so it defaults on. Its fixed tuning is
/// [`HEFT`](crate::controllers::weapons::heft::tuning::HEFT).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeftConfig {
    /// The one-handed weight.
    pub enabled: bool,
    /// The weight of a gun braced in both hands.
    pub braced: BracedHeftConfig,
}

impl HeftConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            braced: BracedHeftConfig::new(),
        }
    }
}

impl Default for HeftConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The weight of a long gun braced in both hands, on the foregrip. Pistols and revolvers carry
/// none. Acts only under the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BracedHeftConfig {
    pub enabled: bool,
}

impl BracedHeftConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for BracedHeftConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Which hand holds the weapon (see [`crate::controllers::weapons::hold`]). Acts only under the
/// controller scheme.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HoldConfig {
    /// Hold a weapon in the hand that drew it, picked it up, took it, or was handed it, rather than
    /// always in the right hand, and fire each held gun from its own hand's trigger. The free hand
    /// takes the held gun at its grip, either hand draws any weapon off the body while the other
    /// holds one, and a grab takes a gun out of a character's hands or off a rack.
    pub either_hand: bool,
    /// Hold a gun only while its grip is held: opening the grip over a socket that takes it docks
    /// it there, and anywhere else lets it fall, with the hand's motion. The game's pistol pair,
    /// which cannot fall, goes back onto the body. Off, guns are sticky: a gun stays in the hand
    /// until a grab of a socket docks it.
    pub grip_to_hold: bool,
}

impl HoldConfig {
    pub const fn new() -> Self {
        Self {
            either_hand: true,
            grip_to_hold: true,
        }
    }
}

impl Default for HoldConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The physical grenade throw (see [`crate::controllers::weapons::grenade`]), whose fixed tuning is
/// [`GRENADE`](crate::controllers::weapons::grenade::tuning::GRENADE).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrenadeConfig {
    /// Catching live grenades.
    pub catch: GrenadeCatchConfig,
}

impl GrenadeConfig {
    pub const fn new() -> Self {
        Self {
            catch: GrenadeCatchConfig::new(),
        }
    }
}

impl Default for GrenadeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Catching a live grenade near the player and throwing it on. Acts only under the controller
/// scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrenadeCatchConfig {
    /// Let a free hand catch a live grenade with a grip press at it.
    pub enabled: bool,
}

impl GrenadeCatchConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for GrenadeCatchConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The C4 throw, the press onto surfaces, and the wrist detonator (see
/// [`crate::controllers::weapons::c4`]), whose fixed tuning is
/// [`C4`](crate::controllers::weapons::c4::tuning::C4).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct C4Config {
    /// The detonator button on the off-hand's wrist.
    pub detonator: WristDetonatorConfig,
    /// Pressing a held charge onto a surface sticks it there.
    pub stick: C4StickConfig,
}

impl C4Config {
    pub const fn new() -> Self {
        Self {
            detonator: WristDetonatorConfig::new(),
            stick: C4StickConfig::new(),
        }
    }
}

impl Default for C4Config {
    fn default() -> Self {
        Self::new()
    }
}

/// The detonator button on the inside of the off-hand's wrist (see
/// [`crate::controllers::weapons::c4::press`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WristDetonatorConfig {
    /// Draw the button on the wrist, facing out of it: dim with no charges placed, lit with a pip
    /// beside it for each placed charge, and flashing on a press and on the detonation.
    pub overlay: bool,
}

impl WristDetonatorConfig {
    pub const fn new() -> Self {
        Self { overlay: true }
    }
}

impl Default for WristDetonatorConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Pressing a held charge onto a surface sticks it there. Acts only under the controller scheme,
/// so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct C4StickConfig {
    /// Stick a held charge to the surface it is pressed onto.
    pub enabled: bool,
}

impl C4StickConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for C4StickConfig {
    fn default() -> Self {
        Self::new()
    }
}
