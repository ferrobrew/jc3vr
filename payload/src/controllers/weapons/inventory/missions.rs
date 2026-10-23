//! The missions' and the challenges' holds of the player's weapons, and the mission manager's
//! checkpoint snapshot. The engine side is in `docs/engine/gameplay/weapon-gives-and-restores.md`.
//!
//! - A mission that takes the player's weapons away caches the slots in a graph script's weapon
//!   cache and forces the weapons out ([`NodeWeaponCache::CacheAllWeaponSlots`] with its removal
//!   set, detoured). The table caches first, with the slots given the store projection; the
//!   weapons beyond the slots go with the rest, so the next input tick stashes them: hidden, with
//!   no grab volumes, still carried. The give-back is [`NodeWeaponCache::RestoreWeapons`]
//!   (detoured): the next input tick shows the carried weapons again, and the table restores its
//!   snapshot once the slots' spawns arrive.
//! - The challenges take and give back the same way ([`crate::controllers::weapons::inventory::challenges`]).
//! - At each mission checkpoint the mission manager snapshots the slots
//!   ([`MissionManager::CacheWeapons`], detoured): the table snapshots itself first, the slots are
//!   given the store projection, and the carried weapons are recorded for a restart. A death, a
//!   failure, or a checkpoint restart replays the snapshot ([`MissionManager::RestoreCachedWeapons`],
//!   detoured, after the player's respawn): the reset that follows respawns the checkpoint's
//!   carried weapons, and the table returns to its snapshot.
//!
//! The pistol slot's weapon goes the same way: what it was (one pistol or the game's pair) is noted
//! as the cache takes it, so the give-back restores it as it was
//! ([`crate::controllers::weapons::dual_wield::single`]).
//!
//! The detours run on whatever thread runs the graph script and the mission manager, so they only
//! set flags, fold the table's events, and give the slots what the table's projection puts there.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use detours_macro::detour;
use jc3gi::{
    character::inventory::{Inventory, WeaponSlot},
    graph_script::NodeWeaponCache,
    mission::MissionManager,
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    hands::ik,
    loadout::{CacheKind, live},
    weapons::{
        dual_wield::single,
        inventory::{carry, reconcile, restore},
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CACHE_ALL_WEAPON_SLOTS_BINDER)
        .with_static_binder(&RESTORE_WEAPONS_BINDER)
        .with_static_binder(&CACHE_WEAPONS_BINDER)
        .with_static_binder(&RESTORE_CACHED_WEAPONS_BINDER)
}

/// Stash the carried weapons when a mission or a challenge took the slots' weapons, and show them
/// again at its give-back.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn input_tick() {
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
                "inventory: the player's weapons were taken; the carried weapons are stashed"
            );
        }
        return;
    }
    if GIVEN_BACK.swap(false, Ordering::Relaxed) {
        let shown = unsafe { carry::set_stashed(false) };
        let respawning = restore::give_back();
        if shown || respawning {
            tracing::info!(
                target: "controllers",
                carried = carry::count(),
                respawning,
                "inventory: the player's weapons were given back; the carried weapons are back"
            );
        }
    }
}

/// Stash the carried weapons at the next input tick: a mission or a challenge took the player's
/// weapons.
pub(super) fn ask_stash() {
    if carry::count() > 0 {
        STASH_ASKED.store(true, Ordering::Relaxed);
    }
}

/// Show the carried weapons again at the next input tick, and respawn any a reset held back: a
/// mission or a challenge gave the player's weapons back.
pub(super) fn ask_give_back() {
    GIVEN_BACK.store(true, Ordering::Relaxed);
}

/// Whether `inventory` is the local player's.
fn is_local(inventory: *const Inventory) -> bool {
    let local = ik::local_character();
    // SAFETY: the game's callers hand a live inventory.
    unsafe { inventory.as_ref() }.is_some_and(|i| local != 0 && i.m_Character as usize == local)
}

/// Set by the detour for the next input tick.
static STASH_ASKED: AtomicBool = AtomicBool::new(false);
/// What the pistol slot held when a mission took the slots' weapons, set by the detour for the
/// next input tick: nothing, the game's pair, or a single pistol, which its give-back restores.
static TOOK_PISTOL: AtomicU8 = AtomicU8::new(TOOK_NOTHING);
const TOOK_NOTHING: u8 = 0;
const TOOK_PAIR: u8 = 1;
const TOOK_SINGLE: u8 = 2;
/// Set by a give-back's detour for the next input tick.
static GIVEN_BACK: AtomicBool = AtomicBool::new(false);

#[detour(address = jc3gi::graph_script::NodeWeaponCache::CacheAllWeaponSlots_ADDRESS)]
fn cache_all_weapon_slots(cache: *mut NodeWeaponCache, inventory: *mut Inventory, remove: bool) {
    let ours = is_local(inventory);
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
        ask_stash();
    }
}

/// The graph script's give-back of its cache.
#[detour(address = jc3gi::graph_script::NodeWeaponCache::RestoreWeapons_ADDRESS)]
fn restore_weapons(cache: *mut NodeWeaponCache, inventory: *mut Inventory) {
    RESTORE_WEAPONS.get().unwrap().call(cache, inventory);
    if is_local(inventory) {
        tracing::info!(target: "controllers", "inventory: a mission is giving the player's weapons back");
        live::given_back(CacheKind::Mission);
        ask_give_back();
    }
}

/// The mission manager's snapshot of the player's slots at a checkpoint.
#[detour(address = jc3gi::mission::MissionManager::CacheWeapons_ADDRESS)]
fn cache_weapons(manager: *mut MissionManager) {
    if let Some(store) = live::checkpoint_caching() {
        // SAFETY: the mission manager runs on the game thread, outside the world update.
        unsafe { reconcile::store(&store) };
        restore::checkpoint_cached(carry::records(), single::slot_single());
        tracing::info!(target: "controllers", "inventory: a mission checkpoint snapshots the player's weapons");
    }
    CACHE_WEAPONS.get().unwrap().call(manager);
}

/// The mission manager's replay of its checkpoint snapshot, right after the player's respawn.
#[detour(address = jc3gi::mission::MissionManager::RestoreCachedWeapons_ADDRESS)]
fn restore_cached_weapons(manager: *mut MissionManager) {
    RESTORE_CACHED_WEAPONS.get().unwrap().call(manager);
    tracing::info!(target: "controllers", "inventory: a restart replays the mission checkpoint's weapons");
    live::checkpoint_restoring();
    restore::checkpoint_restoring();
}
