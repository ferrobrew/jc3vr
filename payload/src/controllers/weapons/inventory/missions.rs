//! The carried weapons while a mission holds the player's weapons. The engine side is in
//! `docs/engine/gameplay/weapon-ownership-and-despawn.md`.
//!
//! A mission that takes the player's weapons away caches the slots in a graph script's weapon
//! cache and forces the weapons out ([`NodeWeaponCache::CacheAllWeaponSlots`] with its removal
//! set, detoured). The weapons beyond the slots should go with them, so the detour asks the next
//! input tick to stash them: hidden, with no grab volumes, still carried. The give-back has no
//! seam of its own (it is inlined into its callers), so the carried weapons come back when a
//! tracked slot holds a weapon again, which is when the mission gives the slots back, or any
//! weapon the player picks up in the meantime.
//!
//! The pistol slot's weapon goes the same way: what it was (one pistol or the game's pair) is noted
//! as the cache takes it, so the give-back restores it as it was
//! ([`crate::controllers::weapons::dual_wield::single`]).
//!
//! The detour also tells the live loadout table ([`crate::controllers::loadout::live`]) of the
//! cache and the removal. It runs on whatever thread runs the graph script, so it only sets flags
//! and folds the table's events.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use detours_macro::detour;
use jc3gi::{
    character::inventory::{Inventory, WeaponSlot},
    graph_script::NodeWeaponCache,
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    hands::ik,
    loadout::live,
    weapons::{
        dual_wield::single,
        inventory::{carry, reconcile},
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CACHE_ALL_WEAPON_SLOTS_BINDER)
}

/// Stash the carried weapons when a mission took the slots' weapons, and bring them back once a
/// slot holds a weapon again (`slots_filled`).
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn input_tick(slots_filled: bool) {
    match TOOK_PISTOL.swap(TOOK_NOTHING, Ordering::Relaxed) {
        TOOK_PAIR => single::cover_mission(false),
        TOOK_SINGLE => single::cover_mission(true),
        _ => {}
    }
    if STASH_ASKED.swap(false, Ordering::Relaxed) {
        // The table took the hands' weapons with the rest, so the reconcile disarms any held.
        if unsafe { carry::set_stashed(true) } {
            tracing::info!(
                target: "controllers",
                carried = carry::count(),
                "inventory: a mission took the player's weapons; the carried weapons are stashed"
            );
        }
        return;
    }
    if !carry::stashed() {
        EMPTIED.store(false, Ordering::Relaxed);
        return;
    }
    // The slots must be seen empty first, so a removal that lands a tick late does not bring the
    // carried weapons straight back.
    if !slots_filled {
        EMPTIED.store(true, Ordering::Relaxed);
        return;
    }
    if EMPTIED.swap(false, Ordering::Relaxed) && unsafe { carry::set_stashed(false) } {
        tracing::info!(
            target: "controllers",
            carried = carry::count(),
            "inventory: a slot holds a weapon again; the carried weapons are back"
        );
    }
}

/// Set by the detour for the next input tick.
static STASH_ASKED: AtomicBool = AtomicBool::new(false);
/// What the pistol slot held when a mission took the slots' weapons, set by the detour for the
/// next input tick: nothing, the game's pair, or a single pistol, which its give-back restores.
static TOOK_PISTOL: AtomicU8 = AtomicU8::new(TOOK_NOTHING);
const TOOK_NOTHING: u8 = 0;
const TOOK_PAIR: u8 = 1;
const TOOK_SINGLE: u8 = 2;
/// Whether the slots have been seen empty since the carried weapons were stashed.
static EMPTIED: AtomicBool = AtomicBool::new(false);

#[detour(address = jc3gi::graph_script::NodeWeaponCache::CacheAllWeaponSlots_ADDRESS)]
fn cache_all_weapon_slots(cache: *mut NodeWeaponCache, inventory: *mut Inventory, remove: bool) {
    let local = ik::local_character();
    // SAFETY: the cache is given a live inventory.
    let ours = unsafe { inventory.as_ref() }
        .is_some_and(|i| local != 0 && i.m_Character as usize == local);
    // The pistol slot is read before the cache forces it out.
    let pistol = unsafe { inventory.as_ref() }
        .filter(|_| ours && remove)
        .and_then(|i| i.m_Weapons.get(WeaponSlot::E_SLOT_DUAL_WIELD as usize))
        .map(|slot| slot.px as usize)
        .filter(|&object| object != 0);
    if ours && let Some(store) = live::mission_caching() {
        // SAFETY: the graph script runs on the game thread, outside the world update.
        unsafe { reconcile::store(&store) };
    }
    CACHE_ALL_WEAPON_SLOTS
        .get()
        .unwrap()
        .call(cache, inventory, remove);
    if let Some(object) = pistol {
        let took = if single::is(object) {
            TOOK_SINGLE
        } else {
            TOOK_PAIR
        };
        TOOK_PISTOL.store(took, Ordering::Relaxed);
    }
    if remove && ours {
        live::mission_took();
    }
    if remove && ours && carry::count() > 0 {
        STASH_ASKED.store(true, Ordering::Relaxed);
    }
}
