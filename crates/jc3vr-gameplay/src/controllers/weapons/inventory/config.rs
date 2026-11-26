//! The settings of the VR inventory (see [`crate::controllers::weapons::inventory`]), held in
//! [`WeaponsConfig::inventory`](crate::controllers::config::WeaponsConfig::inventory).

use serde::{Deserialize, Serialize};

/// The VR inventory. Acts only under the controller scheme. Its fixed tuning is in
/// [`crate::controllers::weapons::inventory`]: the pins ([`PINS`](crate::controllers::weapons::inventory::PINS)),
/// the lost-weapon return, and the sockets' offsets.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InventoryConfig {
    /// Track the weapons the player owns, holds, and lets go of in the loadout table, which
    /// everything else here builds on.
    pub enabled: bool,
    /// Keep weapons beyond the slots on the body's sockets: a weapon picked up into a taken slot
    /// leaves the slot's weapon where it is on the body, and any weapon docks on any free socket
    /// that fits it. Off, every weapon on the body beyond what the slots hold falls where it is.
    pub carry: bool,
    /// Keep the carried weapons across saves and loads, in a sidecar file beside the settings,
    /// tied to the save by a fingerprint of its slots and reserves.
    pub persist: bool,
    /// Make the game's slots a projection of the loadout table each tick, the table being the only
    /// writer of where the weapons are: the slots hold what the table has on the body, the primary
    /// hand's weapon is wielded, and every other weapon in a hand is armed there. Off, the table is
    /// put back into the slots and stops, and the game's own one-weapon model runs, as with the
    /// inventory off.
    pub project_slots: bool,
    /// Bring a lost weapon back to a free socket that fits it, else the player's feet: one out of
    /// the world, or one let go of at speed and left far behind
    /// ([`LOST_RULES`](crate::controllers::weapons::inventory::LOST_RULES)). Off, a lost weapon
    /// stays where it is, and is forgotten once far enough away.
    pub return_lost: bool,
}

impl InventoryConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            carry: true,
            persist: true,
            project_slots: true,
            return_lost: true,
        }
    }
}

impl Default for InventoryConfig {
    fn default() -> Self {
        Self::new()
    }
}
