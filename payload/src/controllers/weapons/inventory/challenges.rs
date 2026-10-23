//! The challenges' holds of the player's weapons. The engine side is in
//! `docs/engine/gameplay/weapon-gives-and-restores.md`, "The challenges".
//!
//! A challenge records the player's loadout and takes it away at two seams: its start, when its
//! spawn point overrides the loadout ([`ROMManager::StartROM`], which marks every slot for
//! removal), and its object's initialisation ([`ROMObject::PostInit`], which despawns at once each
//! slot the challenge does not allow). Before either, the table snapshots itself as the challenge's
//! cache, keeping the first snapshot through a retry or a restart, and the slots are given the
//! store projection, so the challenge records what the player has. When either took a weapon from
//! the slots, the table takes every weapon on the body and the carried weapons are stashed, as for
//! a mission. Its give-back is the challenge's abort ([`ROMManager::AbortROM`]) or the restore of its
//! record ([`ROMObject::RestoreWeaponsAndAmmo`]); after it the carried weapons come back and the
//! table restores its snapshot once the slots settle.
//!
//! The detours run on the game thread, outside the world update, and only fold the table's events
//! and give the slots what the table's projection puts there.

use detours_macro::detour;
use jc3gi::mission::{ROMManager, ROMObject};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    loadout::{CacheKind, live},
    weapons::{
        engine,
        inventory::{engine as inventory_engine, missions, reconcile},
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&START_ROM_BINDER)
        .with_static_binder(&ABORT_ROM_BINDER)
        .with_static_binder(&POST_INIT_BINDER)
        .with_static_binder(&RESTORE_WEAPONS_AND_AMMO_BINDER)
}

/// The local player's tracked slots' weapons, by slot, and their removal marks.
fn slots_now() -> Option<([usize; 8], [bool; 8])> {
    // SAFETY: the challenge seams run on the game thread, outside the world update.
    let owner = unsafe { engine::local_player() }?;
    let inventory = &owner.m_Inventory;
    let mut objects = [0; 8];
    let mut marked = [false; 8];
    for slot in inventory_engine::tracked_slots() {
        objects[slot as usize] = inventory.m_Weapons[slot as usize].px as usize;
        marked[slot as usize] = inventory.m_WeaponToRemove[slot as usize];
    }
    Some((objects, marked))
}

/// Snapshot the table as the challenge's cache, and give the slots the store projection.
fn cache() {
    if let Some(store) = live::challenge_caching() {
        // SAFETY: the challenge seams run on the game thread, outside the world update.
        unsafe { reconcile::store(&store) };
        tracing::info!(target: "controllers", "inventory: a challenge records the player's weapons");
    }
}

/// The challenge took a weapon of the slots: the table takes every weapon on the body, and the
/// carried weapons are stashed.
fn took() {
    tracing::info!(target: "controllers", "inventory: a challenge took the player's weapons");
    live::challenge_took();
    missions::ask_stash();
}

/// The challenge gave the player's weapons back.
fn given_back() {
    tracing::info!(target: "controllers", "inventory: a challenge is giving the player's weapons back");
    live::given_back(CacheKind::Challenge);
    missions::ask_give_back();
}

#[detour(address = jc3gi::mission::ROMManager::StartROM_ADDRESS)]
fn start_rom(manager: *mut ROMManager, id: u64) -> bool {
    cache();
    let before = slots_now();
    let started = START_ROM.get().unwrap().call(manager, id);
    // Its override of the loadout marks every slot for removal.
    let stripped = before
        .zip(slots_now())
        .is_some_and(|((objects, was), (_, now))| {
            (0..8).any(|slot| objects[slot] != 0 && now[slot] && !was[slot])
        });
    if stripped {
        took();
    }
    started
}

#[detour(address = jc3gi::mission::ROMManager::AbortROM_ADDRESS)]
fn abort_rom(manager: *mut ROMManager) {
    ABORT_ROM.get().unwrap().call(manager);
    given_back();
}

#[detour(address = jc3gi::mission::ROMObject::PostInit_ADDRESS)]
fn post_init(object: *mut ROMObject) {
    cache();
    let before = slots_now();
    POST_INIT.get().unwrap().call(object);
    // Its disallowed slots are despawned at once.
    let despawned = before.zip(slots_now()).is_some_and(|((was, _), (now, _))| {
        (0..8).any(|slot| was[slot] != 0 && now[slot] != was[slot])
    });
    if despawned {
        took();
    }
}

#[detour(address = jc3gi::mission::ROMObject::RestoreWeaponsAndAmmo_ADDRESS)]
fn restore_weapons_and_ammo(despawn: bool) {
    RESTORE_WEAPONS_AND_AMMO.get().unwrap().call(despawn);
    given_back();
}
