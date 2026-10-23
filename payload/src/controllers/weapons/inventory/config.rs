//! The settings of the VR inventory (see [`crate::controllers::weapons::inventory`]), held in
//! [`WeaponsConfig::inventory`](crate::controllers::config::WeaponsConfig::inventory).

use serde::{Deserialize, Serialize};

use crate::controllers::weapons::inventory::{pins::PinSettings, sockets::Offsets};

/// The VR inventory. Acts only under the controller scheme.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InventoryConfig {
    /// Track the weapons the player owns, holds, and lets go of in the loadout table, which
    /// everything else here builds on.
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
    /// Keep weapons beyond the slots on the body's sockets: a weapon picked up into a taken slot
    /// leaves the slot's weapon where it is on the body, and any weapon docks on any free socket
    /// that fits it. Off, every weapon on the body beyond what the slots hold falls where it is.
    pub carry: bool,
    /// Where each socket's weapon sits relative to its bone.
    pub offsets: Offsets,
    /// Keep the carried weapons across saves and loads, in a sidecar file beside the settings,
    /// tied to the save by a fingerprint of its slots and reserves.
    pub persist: bool,
    /// Make the game's slots a projection of the loadout table each tick, the table being the only
    /// writer of where the weapons are: the slots hold what the table has on the body, the primary
    /// hand's weapon is wielded, and every other weapon in a hand is armed there. Off, the table is
    /// put back into the slots and stops, and the game's own one-weapon model runs, as with the
    /// inventory off.
    pub project_slots: bool,
    /// Leave the hands and the body as they are through traversal: the base layer's tasks that put
    /// the wielded weapon away, hide the slots' weapons in a seat, stop a weapon's use, or drop a
    /// heavy weapon are refused for the local player, the forced neutral state's unequip is undone,
    /// and where the game's update skips the wielded weapon (a seat, the boost wingsuit) the gun in
    /// a hand is fired by the mod's. Off, the game's tasks run and the table follows them.
    pub keep_through_traversal: bool,
    /// Keep the heavy weapons beyond the slots like the long guns: taken into a hand through the
    /// table, armed there, docked on the back, and never wielded from the heavy slot, whose unequip
    /// drops them. Off, a heavy weapon is the game's: its pickup wields it from its slot and puts
    /// the other hand's weapon away, it docks nowhere, and it is dropped when put away.
    pub heavy_on_back: bool,
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
            offsets: Offsets::new(),
            persist: true,
            project_slots: true,
            keep_through_traversal: true,
            heavy_on_back: true,
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

/// The most weapons lying in the world the table keeps.
const MAX_TRACKED: usize = 24;
