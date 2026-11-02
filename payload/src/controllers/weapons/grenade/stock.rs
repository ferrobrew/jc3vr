//! The grenade stock: the game's own count of the local player's hand grenades, with its native
//! capacity, and the only ways it changes under the controller scheme.
//!
//! The count is the hand-grenade weapon's magazine plus the inventory's reserve of its
//! ammunition kind, the sum the HUD's grenade readout shows, and the capacity is the reserve's cap
//! ([`Inventory::m_MaxAmmunition`]), which the game's grenade upgrades raise. A pouch draw takes
//! one ([`take`]) and a grenade pressed back onto the pouch with its pin in gives one back
//! ([`give_back`]), both through the game's own setters so the HUD follows. Under the game's
//! unlimited-ammunition option the draw is free.
//!
//! Restocking is physical only: while the controller scheme runs the grenade pouch, the game's own
//! grenade pickup ([`WeaponBase::PickupStart`]), which the proximity pickup and the use input both
//! reach, credits nothing to the local player ([`pickup_start`]). A grenade the hands take from the
//! world goes through the mod's own take, which consumes the native grenade without crediting it
//! ([`consume_through_pickup`]).
//!
//! [`Inventory::m_MaxAmmunition`]: jc3gi::character::inventory::Inventory::m_MaxAmmunition

use std::{cell::Cell, fmt};

use detours_macro::detour;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{AmmunitionType, WeaponBase},
    game::GameObject,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{scheme, weapons::engine},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&PICKUP_START_BINDER)
}

/// The local player's grenades as the game counts them.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Stock {
    /// The grenade weapon's magazine, never below zero.
    pub magazine: i32,
    /// The inventory's reserve of hand grenades, never below zero.
    pub reserve: i32,
    /// The reserve's cap, or `None` for an uncapped reserve (a cap of zero or less, which the
    /// game's own add treats as no cap).
    pub capacity: Option<i32>,
    /// The game's unlimited ammunition applies to the grenade weapon.
    pub unlimited: bool,
}

impl Stock {
    /// The grenades the player has: the HUD's count.
    pub fn count(self) -> i32 {
        self.magazine.saturating_add(self.reserve)
    }

    /// Whether the pouch has a grenade to give.
    pub fn may_draw(self) -> bool {
        self.unlimited || self.count() > 0
    }

    /// What a draw takes from the stock, or `None` when there is nothing to take. The reserve goes
    /// first, so the weapon keeps its magazine loaded for the game's own throw.
    pub fn draw(self) -> Option<Change> {
        if self.unlimited {
            Some(Change::None)
        } else if self.reserve > 0 {
            Some(Change::Reserve(self.reserve - 1))
        } else if self.magazine > 0 {
            Some(Change::Magazine(self.magazine - 1))
        } else {
            None
        }
    }

    /// What giving a grenade back does to the stock. A full stock refuses, except under unlimited
    /// ammunition, where the grenade is put away without a count to raise.
    pub fn give_back(self) -> Result<Change, Full> {
        let room = self
            .capacity
            .is_none_or(|capacity| self.count() < capacity && self.reserve < capacity);
        match (room, self.unlimited) {
            (true, _) => Ok(Change::Reserve(self.reserve.saturating_add(1))),
            (false, true) => Ok(Change::None),
            (false, false) => Err(Full),
        }
    }
}

/// A change to the stock, as the count it sets.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Change {
    /// Nothing changes: unlimited ammunition.
    None,
    /// The reserve becomes this.
    Reserve(i32),
    /// The magazine becomes this.
    Magazine(i32),
}

/// The stock is at its capacity.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Full;

impl fmt::Display for Full {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "grenade stock: the pouch is full")
    }
}

impl std::error::Error for Full {}

/// Whether the pouch has a grenade to give, as the last read found the stock. `false` before the
/// first read.
pub(crate) fn in_stock() -> bool {
    CACHED.lock().is_some_and(Stock::may_draw)
}

/// The stock as the last read found it.
pub(crate) fn last() -> Option<Stock> {
    *CACHED.lock()
}

/// Read the local player's stock and remember it for [`in_stock`].
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn refresh() -> Option<Stock> {
    let stock = unsafe { engine::local_player() }.and_then(|owner| unsafe { read(owner) });
    *CACHED.lock() = stock;
    stock
}

/// Take one grenade for a pouch draw that has already put its item in the hand. `false`, with the
/// stock unchanged, when there was none to take.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn take() -> bool {
    let Some(owner) = (unsafe { engine::local_player() }) else {
        return false;
    };
    let Some(stock) = (unsafe { read(owner) }) else {
        return false;
    };
    let Some(change) = stock.draw() else {
        return false;
    };
    unsafe { apply(owner, change) };
    let _ = unsafe { refresh() };
    true
}

/// Whether a grenade could be given back now.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn may_give_back() -> Result<(), Full> {
    let stock = unsafe { engine::local_player() }
        .and_then(|owner| unsafe { read(owner) })
        .ok_or(Full)?;
    stock.give_back().map(|_| ())
}

/// Give one grenade back to the stock, up to its capacity.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn give_back() -> Result<(), Full> {
    let owner = unsafe { engine::local_player() }.ok_or(Full)?;
    let stock = unsafe { read(owner) }.ok_or(Full)?;
    let change = stock.give_back()?;
    unsafe { apply(owner, change) };
    let _ = unsafe { refresh() };
    Ok(())
}

/// Consume `weapon`, a native hand grenade in the world the hands took, as the game's own pickup
/// does ([`WeaponBase::PickupStart`]): hidden, out of the physics world, handed to the spawn
/// system's despawn, and its picked-up event sent, which is how the placement rule that put it on a
/// locker or rack learns that it is gone. Its rounds are emptied first, so the pickup credits
/// nothing.
///
/// # Safety
///
/// The game thread, before the world update; `weapon` must be held alive.
pub(super) unsafe fn consume_through_pickup(weapon: &mut WeaponBase) -> bool {
    let Some(owner) = (unsafe { engine::local_player() }) else {
        return false;
    };
    unsafe { weapon.SetMagazineAmmo(0) };
    weapon.m_StoredNonMagazineAmmo = 0;
    let activator = (owner as *mut Character as usize + Character::GAME_OBJECT_BASE_OFFSET as usize)
        as *mut GameObject;
    let previous = PERMIT.replace(true);
    unsafe { weapon.PickupStart(activator) };
    PERMIT.set(previous);
    true
}

static CACHED: Mutex<Option<Stock>> = Mutex::new(None);

thread_local! {
    /// The mod's own consume is running ([`consume_through_pickup`]), which the pickup's gate lets
    /// through.
    static PERMIT: Cell<bool> = const { Cell::new(false) };
}

/// Read `owner`'s stock, or `None` without a grenade weapon or with an ammunition kind outside the
/// inventory's arrays.
///
/// # Safety
///
/// `owner` is the live local player, on the game thread.
unsafe fn read(owner: &Character) -> Option<Stock> {
    let weapon = unsafe { grenade_weapon(owner) }?;
    let kind = unsafe { ammunition_kind(weapon) }?;
    let inventory = &owner.m_Inventory;
    let reserve = inventory.m_Ammunition.get(kind)?;
    let cap = inventory.m_MaxAmmunition.get(kind)?;
    let unlimited = unsafe { owner.HasUnlimitedAmmo() }
        || unsafe { weapon.m_Tuning.as_ref() }.is_some_and(|tuning| tuning.m_unlimited_ammo);
    Some(Stock {
        magazine: weapon.m_MagazineAmmo.max(0),
        reserve: (*reserve).max(0),
        capacity: (*cap > 0).then_some(*cap),
        unlimited,
    })
}

/// Apply `change` to `owner`'s stock through the game's own setters, which keep the HUD current.
///
/// # Safety
///
/// As [`read`], with `owner` mutable.
unsafe fn apply(owner: &mut Character, change: Change) {
    match change {
        Change::None => {}
        Change::Reserve(count) => {
            let Some(weapon) = (unsafe { grenade_weapon(owner) }) else {
                return;
            };
            let kind = weapon.m_AmmunitionType;
            unsafe { owner.m_Inventory.SetAmmoCount(kind, count, false) };
        }
        Change::Magazine(count) => {
            if let Some(weapon) = unsafe { grenade_weapon_mut(owner) } {
                unsafe { weapon.SetMagazineAmmo(count) };
            }
        }
    }
}

/// `owner`'s hand-grenade weapon.
///
/// # Safety
///
/// As [`read`].
unsafe fn grenade_weapon(owner: &Character) -> Option<&WeaponBase> {
    let weapon = owner
        .m_Inventory
        .m_Weapons
        .get(WeaponSlot::E_SLOT_HAND_GRENADE as usize)?;
    // SAFETY: the inventory owns its weapons for as long as the owner lives.
    unsafe { weapon.px.as_ref() }
}

/// `owner`'s hand-grenade weapon, mutable.
///
/// # Safety
///
/// As [`read`].
unsafe fn grenade_weapon_mut(owner: &mut Character) -> Option<&mut WeaponBase> {
    let weapon = owner
        .m_Inventory
        .m_Weapons
        .get(WeaponSlot::E_SLOT_HAND_GRENADE as usize)?;
    // SAFETY: as above.
    unsafe { weapon.px.as_mut() }
}

/// The index of `weapon`'s ammunition kind in the inventory's arrays: a limited kind.
///
/// # Safety
///
/// `weapon` must be live.
unsafe fn ammunition_kind(weapon: &WeaponBase) -> Option<usize> {
    // Read as the raw value: an unset kind is out of the enum's range.
    let raw = unsafe { (&raw const weapon.m_AmmunitionType).cast::<i32>().read() };
    ((0..AmmunitionType::NOF_AMMO_TYPES as i32).contains(&raw)
        && raw != AmmunitionType::AMMO_UNLIMITED as i32)
        .then_some(raw as usize)
}

/// Whether the game's own pickup of `weapon` for `activator` is held back: a hand grenade, for the
/// local player, while the controller scheme runs the grenade pouch and the mod's own consume is
/// not the caller.
///
/// # Safety
///
/// `weapon` must be live, on the game thread.
unsafe fn held_back(weapon: &WeaponBase, activator: *mut GameObject) -> bool {
    if PERMIT.get() {
        return false;
    }
    // Read as the raw value: an unset slot is out of the enum's range.
    let slot = unsafe { (&raw const weapon.m_EquipSlot).cast::<i32>().read() };
    if slot != WeaponSlot::E_SLOT_HAND_GRENADE as i32 {
        return false;
    }
    let player = unsafe { Character::GetLocalPlayerCharacter() };
    if player.is_null()
        || activator as usize != player as usize + Character::GAME_OBJECT_BASE_OFFSET as usize
    {
        return false;
    }
    Config::lock_query(|c| c.controllers.weapons.grenade_throw) && scheme::is_controllers()
}

#[detour(address = jc3gi::equipment::weapon::WeaponBase::PickupStart_ADDRESS)]
fn pickup_start(weapon: *mut WeaponBase, activator: *mut GameObject) {
    // SAFETY: the engine's own live weapon, for the duration of its pickup.
    if unsafe { weapon.as_ref() }.is_some_and(|w| unsafe { held_back(w, activator) }) {
        return;
    }
    PICKUP_START.get().unwrap().call(weapon, activator);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock(magazine: i32, reserve: i32, capacity: Option<i32>, unlimited: bool) -> Stock {
        Stock {
            magazine,
            reserve,
            capacity,
            unlimited,
        }
    }

    #[test]
    fn a_draw_takes_from_the_reserve_first_then_the_magazine() {
        assert_eq!(stock(1, 3, Some(4), false).draw(), Some(Change::Reserve(2)));
        assert_eq!(
            stock(1, 0, Some(4), false).draw(),
            Some(Change::Magazine(0))
        );
        assert_eq!(stock(0, 0, Some(4), false).draw(), None);
        assert!(!stock(0, 0, Some(4), false).may_draw());
    }

    #[test]
    fn unlimited_ammunition_makes_the_draw_free_even_when_empty() {
        let empty = stock(0, 0, Some(4), true);
        assert!(empty.may_draw());
        assert_eq!(empty.draw(), Some(Change::None));
    }

    #[test]
    fn giving_back_fills_the_reserve_up_to_the_capacity() {
        assert_eq!(
            stock(0, 2, Some(4), false).give_back(),
            Ok(Change::Reserve(3))
        );
        assert_eq!(stock(0, 4, Some(4), false).give_back(), Err(Full));
        // The magazine counts toward the capacity, as the HUD counts it.
        assert_eq!(stock(1, 3, Some(4), false).give_back(), Err(Full));
        assert_eq!(
            stock(0, 9, None, false).give_back(),
            Ok(Change::Reserve(10))
        );
    }

    #[test]
    fn a_full_stock_under_unlimited_ammunition_takes_the_grenade_without_counting_it() {
        assert_eq!(stock(0, 4, Some(4), true).give_back(), Ok(Change::None));
        assert_eq!(
            stock(0, 1, Some(4), true).give_back(),
            Ok(Change::Reserve(2))
        );
    }

    #[test]
    fn a_draw_then_a_give_back_leaves_the_count_as_it_was() {
        for (magazine, reserve) in [(0, 3), (1, 2), (1, 0)] {
            let before = stock(magazine, reserve, Some(4), false);
            let after = match before.draw().unwrap() {
                Change::Reserve(r) => stock(magazine, r, Some(4), false),
                Change::Magazine(m) => stock(m, reserve, Some(4), false),
                Change::None => before,
            };
            let back = match after.give_back().unwrap() {
                Change::Reserve(r) => stock(after.magazine, r, Some(4), false),
                Change::Magazine(m) => stock(m, after.reserve, Some(4), false),
                Change::None => after,
            };
            assert_eq!(back.count(), before.count());
        }
    }
}
