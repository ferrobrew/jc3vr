//! The game's own use-item pickup of a weapon, routed into the loadout table's take. The design is
//! the two-handed inventory's "No automatic moves" (`docs/mod/input/two-handed-inventory.md`), the
//! engine side in `docs/engine/gameplay/item-pickup.md`.
//!
//! The game completes a pickup through [`WeaponBase::PickupItem`]: from the pickup animation's track
//! message on land, and at once for a swimming character. It unequips the wielded weapon first,
//! which takes the other hand's gun away. For the local player, while the table runs and holds the
//! weapons in the hands that took them, the detour does not run it: it keeps the weapon alive and
//! queues it, and the next input tick takes it through the table into a free hand, the dominant
//! first ([`crate::controllers::weapons::hold::pickup::take_by_table`]), or puts it back into the
//! world with both hands full. A weapon of the heavy slot and the embedded power core are left to
//! the game's pickup, and a weapon with infinite pickups, which the pickup puts into no slot, only
//! loses its stored rounds, as the pickup would do, without the unequip.
//!
//! The mod's own calls of the pickup ([`own_pickup`], for the takes the table leaves to the game)
//! pass through. The detour also makes a pistol taken from the world one pistol while pistols are
//! single, before the game's add equips and shows it, and its pickup start's detour
//! ([`WeaponBase::PickupStart`]) notes a rack's copy of a pistol
//! ([`crate::controllers::weapons::dual_wield::single`]).
//!
//! The pickup runs wherever the character's track messages are handled, so the detour takes only
//! its own queue's lock and reads only the weapon's own fields.

use std::cell::Cell;

use detours_macro::detour;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
    game::GameObject,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        hands::ik,
        loadout::{TakeSource, live, take::TakeRoute},
        weapons::{
            dual_wield::single,
            hold::{
                self, Cause,
                pickup::{self, TakeOptions},
                refs::Strong,
            },
            inventory::{
                engine::{slot_of, strong_to, weapon_of},
                gives,
            },
        },
    },
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&PICKUP_ITEM_BINDER)
        .with_static_binder(&PICKUP_START_BINDER)
}

/// Complete the game's pickup of `weapon` for `activator` as the mod's own call, which the detour
/// passes through.
///
/// # Safety
///
/// As [`WeaponBase::PickupItem`]: `weapon` live, on the main thread outside the world update.
pub(crate) unsafe fn own_pickup(weapon: &mut WeaponBase, activator: *mut GameObject) {
    OWN.with(|own| own.set(true));
    unsafe { weapon.PickupItem(activator) };
    OWN.with(|own| own.set(false));
}

/// Take the weapons the game's pickup queued since the last input tick. Called each input tick
/// while the table runs.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(owner: &mut Character) {
    let queued = std::mem::take(&mut *QUEUED.lock());
    for strong in queued {
        let weapon = unsafe { &mut *weapon_of(&strong) };
        unsafe { take(owner, weapon) };
    }
}

/// Forget the queued pickups, putting each weapon back into the world. Called when the table stops.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn stand_down() {
    let queued = std::mem::take(&mut *QUEUED.lock());
    for strong in queued {
        unsafe { (*weapon_of(&strong)).OnDrop() };
    }
}

thread_local! {
    /// Whether the mod is making the pickup call on this thread.
    static OWN: Cell<bool> = const { Cell::new(false) };
}

/// The weapons the game's pickup handed to the table, held alive until the next input tick.
static QUEUED: Mutex<Vec<Strong>> = Mutex::new(Vec::new());

/// Take the queued `weapon` into a free hand through the table, or through the game's pickup where
/// the table cannot hold it, or put it back into the world with both hands full.
unsafe fn take(owner: &mut Character, weapon: &mut WeaponBase) {
    let activator = pickup::activator(owner);
    let route = unsafe { pickup::route(weapon, None) };
    let hold = Config::lock_query(|c| c.controllers.weapons.hold);
    let result = match route {
        Ok(TakeRoute::Table { hand }) => {
            let options = TakeOptions {
                drop_replaced: hold.drop_replaced,
                slide_s: if hold.take_slide { hold.snap_s } else { 0.0 },
                cause: Cause::Pickup,
                touch: None,
            };
            unsafe { pickup::take_by_table(owner, hand, weapon, TakeSource::World, options) }
                .map(|()| hand)
        }
        Ok(TakeRoute::Game(why)) => {
            tracing::debug!(target: "controllers", why = why.label(), "hold: the game's pickup takes the weapon");
            unsafe { own_pickup(weapon, activator) };
            return;
        }
        Err(refusal) => Err(refusal),
    };
    match result {
        Ok(hand) => tracing::info!(
            target: "controllers",
            hand = hand.label(),
            "hold: the game's pickup put the weapon into the {} hand",
            hand.label()
        ),
        Err(refusal) => {
            tracing::info!(target: "controllers", %refusal, "hold: the game's pickup took nothing; the weapon stays in the world");
            hold::refused(refusal);
            unsafe { weapon.OnDrop() };
        }
    }
}

/// Whether `activator` is the local player's `CGameObject` subobject.
fn is_local_player(activator: *mut GameObject) -> bool {
    let local = ik::local_character();
    local != 0 && activator as usize == local + Character::GAME_OBJECT_BASE_OFFSET as usize
}

/// Whether the table takes the local player's pickup of `weapon` rather than the game: it runs,
/// the weapons ride the hands that took them, and the weapon is one the table holds in a hand.
fn table_takes(weapon: &WeaponBase) -> bool {
    let either_hand = Config::lock_query(|c| c.controllers.weapons.hold.either_hand);
    let tracked = unsafe { slot_of(weapon) }.is_some_and(|slot| {
        matches!(
            slot,
            WeaponSlot::E_SLOT_DUAL_WIELD
                | WeaponSlot::E_SLOT_TWO_HANDED
                | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
        )
    });
    either_hand && tracked && live::running()
}

#[detour(address = jc3gi::equipment::weapon::WeaponBase::PickupItem_ADDRESS)]
fn pickup_item(weapon: *mut WeaponBase, activator: *mut GameObject) {
    let own = OWN.with(Cell::get);
    // SAFETY: the pickup is given a live weapon.
    let Some(w) = (unsafe { weapon.as_mut() }).filter(|_| is_local_player(activator)) else {
        PICKUP_ITEM.get().unwrap().call(weapon, activator);
        return;
    };
    if !own && table_takes(w) {
        if w.m_InfinitePickup {
            // It gives a copy from its pickup start and goes into no slot.
            w.m_StoredNonMagazineAmmo = 0;
            return;
        }
        if let Some(strong) = unsafe { strong_to(w) } {
            QUEUED.lock().push(strong);
            return;
        }
    }
    if !w.m_InfinitePickup {
        unsafe { single::designate_picked(w) };
    }
    PICKUP_ITEM.get().unwrap().call(weapon, activator);
}

#[detour(address = jc3gi::equipment::weapon::WeaponBase::PickupStart_ADDRESS)]
fn pickup_start(weapon: *mut WeaponBase, activator: *mut GameObject) {
    // SAFETY: the pickup is given a live weapon.
    let local = unsafe { weapon.as_ref() }.filter(|_| is_local_player(activator));
    let Some(w) = local else {
        PICKUP_START.get().unwrap().call(weapon, activator);
        return;
    };
    single::pickup_started(w);
    if w.m_InfinitePickup {
        // The start gives a copy, which the table then places.
        gives::within_copy_start(|| PICKUP_START.get().unwrap().call(weapon, activator));
    } else {
        PICKUP_START.get().unwrap().call(weapon, activator);
    }
}
