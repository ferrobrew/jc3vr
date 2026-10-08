//! A weapon taken off the ground straight into the grabbing hand.
//!
//! The game picks a weapon up through its use-item action: a full-body pickup animation whose
//! track message completes the pickup ([`WeaponBase::PickupItem`]), with whichever weapon the
//! character's interaction proxy offers. A swimming character instead calls `PickupItem` at once,
//! with no animation. The direct pickup makes that same call on the exact weapon the hand
//! touched: the weapon goes into the inventory and is equipped there and then, its magazine as it
//! was, and the hold puts it in the grabbing hand ([`crate::controllers::weapons::hold`]). A
//! weapon already in its slot is dropped or despawned by the game's own inventory add, as with
//! any pickup.
//!
//! A weapon `PickupItem` does not put into the inventory (a hand grenade, which the game collects
//! by proximity, and a weapon with [`WeaponBase::m_InfinitePickup`], which gives a copy) and a
//! pickup the animation forbids
//! ([`Inventory::GetAnimationAllowsPickup`](jc3gi::character::inventory::Inventory::GetAnimationAllowsPickup))
//! are refused, and the caller falls back to the use-item click.

use std::fmt;

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponState},
    game::GameObject,
};

use crate::controllers::{
    aim::gun,
    frame::Hand,
    weapons::{
        dual_wield,
        engine::{self, slot_from_raw},
        hold::{self, holder::Cause},
        holster::slot_label,
    },
};

/// Why a direct pickup did not take the weapon.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// There is no local player.
    NoCharacter,
    /// The weapon is no longer lying in the world.
    NotDropped,
    /// The weapon's slot is out of range.
    UnknownSlot,
    /// A hand grenade: the game collects it by proximity.
    Grenade,
    /// The weapon gives a copy when picked up.
    InfinitePickup,
    /// The character's animation forbids a pickup now.
    AnimationForbids,
    /// The pickup ran, but the weapon is not in the hands afterwards.
    NotWielded,
}

impl Refusal {
    /// Whether the game's use-item action should be tried instead: the direct pickup did not touch
    /// the weapon.
    pub fn falls_back(self) -> bool {
        !matches!(self, Refusal::NotWielded)
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            Refusal::NoCharacter => "there is no local player",
            Refusal::NotDropped => "the weapon is not lying in the world",
            Refusal::UnknownSlot => "the weapon's slot is out of range",
            Refusal::Grenade => "a hand grenade is collected by the game",
            Refusal::InfinitePickup => "the weapon gives a copy when picked up",
            Refusal::AnimationForbids => "the animation forbids a pickup",
            Refusal::NotWielded => "the weapon is not in the hands after the pickup",
        };
        write!(f, "weapon pickup: {why}")
    }
}

impl std::error::Error for Refusal {}

/// Pick `weapon` up into `hand` directly. With `independent_pistols`, a dual-wield pair taken this
/// way draws only the grabbing hand's pistol. Returns the slot the weapon went into.
///
/// # Safety
///
/// `weapon` must be a live weapon (its body checked to belong to it this tick), on the main thread
/// outside the world update.
pub(crate) unsafe fn take(
    hand: Hand,
    weapon: &mut WeaponBase,
    independent_pistols: bool,
) -> Result<WeaponSlot, Refusal> {
    let owner = unsafe { engine::local_player() }.ok_or(Refusal::NoCharacter)?;
    if weapon.m_State != WeaponState::E_WS_DROPPED {
        return Err(Refusal::NotDropped);
    }
    // Read as the raw value: an unset slot is out of the enum's range.
    let slot = unsafe { (&raw const weapon.m_EquipSlot).cast::<i32>().read() };
    let slot = slot_from_raw(slot).ok_or(Refusal::UnknownSlot)?;
    if slot == WeaponSlot::E_SLOT_HAND_GRENADE {
        return Err(Refusal::Grenade);
    }
    if weapon.m_InfinitePickup {
        return Err(Refusal::InfinitePickup);
    }
    if !unsafe { owner.m_Inventory.GetAnimationAllowsPickup() } {
        return Err(Refusal::AnimationForbids);
    }
    let address = weapon as *mut WeaponBase as usize;
    hold::begin(hand, Cause::Pickup, Some(address));
    if independent_pistols && slot == WeaponSlot::E_SLOT_DUAL_WIELD {
        dual_wield::attach::begin_draw(hand);
    }
    let activator = (owner as *mut Character as usize + Character::GAME_OBJECT_BASE_OFFSET as usize)
        as *mut GameObject;
    unsafe { weapon.PickupItem(activator) };
    let wielded = unsafe { gun::held_weapon(owner) }
        .is_some_and(|held| held as *const WeaponBase as usize == address);
    if !wielded {
        return Err(Refusal::NotWielded);
    }
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        slot = slot_label(slot),
        "hold: picked the weapon up into the {} hand",
        hand.label()
    );
    Ok(slot)
}
