//! The settings of the VR inventory (see [`crate::controllers::weapons::inventory`]), held in
//! [`WeaponsConfig::inventory`](crate::controllers::config::WeaponsConfig::inventory).

use serde::{Deserialize, Serialize};

use crate::controllers::{
    frame::Hand,
    weapons::inventory::{
        pins::PinSettings,
        sockets::{Layout, Offsets},
    },
};

/// The VR inventory. Acts only under the controller scheme.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InventoryConfig {
    /// Track the weapons the player owns, holds, and lets go of: the registry everything else here
    /// builds on.
    pub enabled: bool,
    /// Keep the weapons the player let go of nearby from the game's despawn while they are near.
    pub pin_world: bool,
    /// A weapon in the world this near the player is kept, metres.
    pub pin_radius_m: f32,
    /// A kept weapon is let go of once it is this far away, metres.
    pub unpin_radius_m: f32,
    /// The most weapons in the world kept at once, the nearest first.
    pub max_pinned: usize,
    /// A weapon in the world this far away and not kept is left to the game, metres.
    pub forget_m: f32,
    /// Carry weapons beyond the slots on the body's sockets: a weapon picked up into a taken slot
    /// moves the slot's weapon onto a free socket instead of dropping it, a held weapon put to a
    /// free socket docks there, and a carried weapon grabbed off its socket comes into the hand.
    pub carry: bool,
    /// How many long guns the back carries beyond the slots, `0..=3`.
    pub back_sockets: u8,
    /// How many pistols the hips carry beyond the slots, `0..=2`.
    pub hip_sockets: u8,
    /// Whether the chest carries a submachine gun beyond the slots.
    pub chest_socket: bool,
    /// The radius of the grab volume on a carried weapon, and on a free socket, metres.
    pub socket_radius_m: f32,
    /// Where each socket's weapon sits relative to its bone.
    pub offsets: Offsets,
    /// Keep the carried weapons across saves and loads, in a sidecar file beside the settings,
    /// tied to the save by a fingerprint of its slots and reserves.
    pub persist: bool,
}

impl InventoryConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            pin_world: true,
            pin_radius_m: 8.0,
            unpin_radius_m: 12.0,
            max_pinned: 6,
            forget_m: 60.0,
            carry: true,
            back_sockets: 3,
            hip_sockets: 2,
            chest_socket: true,
            socket_radius_m: 0.12,
            offsets: Offsets::new(),
            persist: true,
        }
    }

    /// The sockets these settings give, for `dominant`.
    pub fn layout(&self, dominant: Hand) -> Layout {
        Layout {
            back: self.back_sockets.min(3),
            hips: self.hip_sockets.min(2),
            chest: self.chest_socket,
            dominant,
        }
    }

    /// The pin policy these settings give.
    pub const fn pins(&self) -> PinSettings {
        PinSettings {
            enabled: self.pin_world,
            radius_m: self.pin_radius_m,
            release_m: self.unpin_radius_m,
            max_pinned: self.max_pinned,
            forget_m: self.forget_m,
            max_tracked: MAX_TRACKED,
        }
    }
}

impl Default for InventoryConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The most weapons in the world the registry keeps.
const MAX_TRACKED: usize = 24;
