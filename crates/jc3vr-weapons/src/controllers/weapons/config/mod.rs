//! The weapon-handling settings: the sockets', the recoil's, and the hands' hold's groups, held in
//! [`WeaponsConfig`] beside the reload's ([`crate::controllers::weapons::reload::config`]) and the
//! wheel's ([`crate::controllers::weapons::wheel::config`]), and [`WeaponSettings`], the copy one
//! tick reads. Only switches live here; each feature's tuning is in the crate's `tuning/*.toml`.

mod group;
mod settings;

use serde::{Deserialize, Serialize};

pub use self::{
    group::{WEAPONS_CONFIG, WeaponsConfig},
    settings::WeaponSettings,
};

/// The body's sockets (see [`crate::controllers::weapons::docks`]), whose reach is tuned by
/// [`SocketTuning`](crate::controllers::weapons::volumes::SocketTuning). They act only under the
/// controller scheme.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HolstersConfig {
    /// Keep the move stick's click cycling to the next weapon beside the sockets, as a fallback for
    /// players who cannot reach them. Off by default: the click is half of the debug panel's chord,
    /// and cycling on it switched weapons each time the panel was opened. With a gun in the
    /// off-hand the click retracts the tethers instead.
    pub stick_click_cycles: bool,
}

impl HolstersConfig {
    pub const fn new() -> Self {
        Self {
            stick_click_cycles: false,
        }
    }
}

impl Default for HolstersConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The recoil offset (see [`crate::controllers::weapons::recoil`]): each shot kicks the gun hand
/// away from the controller pose, and a damped spring returns it. Acts only under the controller
/// scheme.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecoilConfig {
    /// Scales the kick taken from the game's per-shot recoil.
    pub kick_scale: f32,
}

impl RecoilConfig {
    pub const fn new() -> Self {
        Self { kick_scale: 1.0 }
    }
}

impl Default for RecoilConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The hands' hold of a weapon (see [`crate::controllers::weapons::hold`]). Acts only under the
/// controller scheme.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HoldConfig {
    /// Hold a gun only while its grip is held: opening the grip over a socket that takes it docks
    /// it there, and anywhere else lets it fall, with the hand's motion. The game's pistol pair,
    /// which cannot fall, goes back onto the body. Off, guns are sticky: a gun stays in the hand
    /// until a grab of a socket docks it.
    pub grip_to_hold: bool,
}

impl HoldConfig {
    pub const fn new() -> Self {
        Self { grip_to_hold: true }
    }
}

impl Default for HoldConfig {
    fn default() -> Self {
        Self::new()
    }
}
