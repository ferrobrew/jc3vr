//! The weapon-handling settings (the sockets, the recoil, and the hands' hold), held in
//! [`WeaponsConfig`] beside the other weapon-handling subsystems' toggles. The reload's are in
//! [`crate::controllers::weapons::reload::config`], and the grapple snatch's in [`SnatchConfig`].
//! Each feature keeps its switches here; its fixed tuning is a constant in the module that owns it.

mod group;
mod settings;
mod snatch;

use serde::{Deserialize, Serialize};

pub use self::{
    group::{WEAPONS_CONFIG, WeaponsConfig},
    settings::WeaponSettings,
    snatch::{SnatchConfig, SwingConfig},
};

/// The body's sockets (see [`crate::controllers::weapons::docks`]), whose fixed tuning is
/// [`SOCKETS`](crate::controllers::weapons::volumes::SOCKETS). They act only under the controller
/// scheme.
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
    /// Draw a weapon let go of near a free socket that takes it into the socket, though the hand
    /// is outside the socket's own reach: over the shoulder for the back, beside the thigh for a
    /// hip. A weapon let go of in front of the body or thrown still falls.
    pub dock_attraction: bool,
    /// Pick weapons up only by hand: the game's weapon pickup panel and its button glyph are
    /// hidden, and the use-item button does not pick a weapon up
    /// ([`crate::controllers::weapons::hold::button_pickup`]).
    pub hand_pickup_only: bool,
}

impl HoldConfig {
    pub const fn new() -> Self {
        Self {
            either_hand: true,
            grip_to_hold: true,
            dock_attraction: true,
            hand_pickup_only: true,
        }
    }
}

impl Default for HoldConfig {
    fn default() -> Self {
        Self::new()
    }
}
