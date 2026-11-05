//! Each hand's grab hint ([`crate::hud::grab_hint`]) from the input tick's grip resolution: the
//! icon of what a grip on a body slot would take, or the swap a release over a taken socket would
//! make.

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
};

use crate::{
    controllers::{
        frame::Hand,
        grab::GrabVolume,
        loadout::{ItemView, LoadoutSnapshot, Socket},
    },
    hud::{
        game_icons::IconId,
        grab_hint::{self, Hint},
    },
};

/// What the hands are doing this tick, as the hint reads it.
pub(super) struct HintInputs<'a> {
    pub snapshot: Option<&'a LoadoutSnapshot>,
    /// Each hand's chosen grip target, by [`Hand::index`].
    pub chosen: [Option<GrabVolume>; 2],
    /// The socket whose dock attraction each hand holding a weapon is in, by [`Hand::index`].
    pub attracted: [Option<Socket>; 2],
}

/// Publish each hand's hint.
///
/// # Safety
///
/// `owner` must be the live local player character, and the snapshot's live bindings weapons the
/// table keeps alive, on the game thread before the world update.
pub(super) unsafe fn publish(owner: Option<&Character>, inputs: &HintInputs) {
    let hints = Hand::BOTH.map(|hand| {
        let snapshot = inputs.snapshot?;
        // SAFETY: forwarded from the caller.
        unsafe { hint(owner, snapshot, inputs, hand) }
    });
    grab_hint::publish(hints);
}

/// Forget the hints: the weapon handling stood down.
pub(super) fn clear() {
    grab_hint::clear();
}

/// `hand`'s hint. A hand holding a weapon in the attraction of a taken socket swaps; an empty hand
/// within reach of a socket takes the weapon on it, and either hand within reach of a pouch takes
/// what the pouch gives.
///
/// # Safety
///
/// As [`publish`].
unsafe fn hint(
    owner: Option<&Character>,
    snapshot: &LoadoutSnapshot,
    inputs: &HintInputs,
    hand: Hand,
) -> Option<Hint> {
    let held = snapshot.held(hand);
    if let Some(socket) = inputs.attracted[hand.index()] {
        // A free socket only docks, which needs no hint.
        let outgoing = snapshot.docked(socket)?;
        // SAFETY: forwarded from the caller.
        return unsafe {
            Some(Hint::Swap {
                incoming: item_icon(held?)?,
                outgoing: item_icon(outgoing)?,
            })
        };
    }
    // SAFETY: forwarded from the caller.
    let icon = unsafe {
        match inputs.chosen[hand.index()]? {
            GrabVolume::Socket(socket) if held.is_none() => item_icon(snapshot.docked(socket)?),
            GrabVolume::GrenadePouch => slot_icon(owner?, WeaponSlot::E_SLOT_HAND_GRENADE),
            GrabVolume::C4Pouch => slot_icon(owner?, WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE),
            // The belt's magazine is for the gun in the other hand.
            GrabVolume::MagazinePouch => item_icon(snapshot.held(hand.other())?),
            _ => None,
        }
    }?;
    Some(Hint::Take(icon))
}

/// The icon of `item`'s weapon, while it is bound to a live one.
///
/// # Safety
///
/// As [`publish`].
unsafe fn item_icon(item: &ItemView) -> Option<IconId> {
    item.kind.weapon()?;
    let address = item.binding.address()?.get();
    // SAFETY: the table keeps its live weapons alive; forwarded from the caller.
    unsafe { weapon_icon(address as *const WeaponBase) }
}

/// The icon of the weapon in `owner`'s `slot`.
///
/// # Safety
///
/// As [`publish`].
unsafe fn slot_icon(owner: &Character, slot: WeaponSlot) -> Option<IconId> {
    let weapon = owner.m_Inventory.m_Weapons.get(slot as usize)?.px;
    // SAFETY: the inventory's slots hold live weapons or null; forwarded from the caller.
    unsafe { weapon_icon(weapon) }
}

/// # Safety
///
/// `weapon` must be null or a live weapon.
unsafe fn weapon_icon(weapon: *const WeaponBase) -> Option<IconId> {
    // SAFETY: forwarded from the caller.
    let weapon = unsafe { weapon.as_ref() }?;
    (weapon.m_WeaponId >= 0).then_some(IconId(weapon.m_WeaponId))
}
