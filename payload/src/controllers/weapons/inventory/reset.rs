//! The spawn system's reset, seen through the local player's inventory's event handler
//! ([`Inventory::HandleEvent`]). The reset sends its pre-reset event
//! ([`Inventory::m_SpawnSystemPreReset`]) at once, before it releases its references to every
//! spawned object, and the inventory then records its slots for the respawn. The detour releases
//! everything the VR inventory holds at that event, so that no weapon it pinned outlives the reset
//! outside the spawn system.
//!
//! At the pre-reset event the carried weapons are recorded for the respawn, and at the post-reset
//! event the respawn is set due ([`crate::controllers::weapons::inventory::restore`]). The player
//! save data's store ([`StorePlayerSaveData`]) and load ([`LoadPlayerSaveData`]) are detoured too:
//! before a store the save sidecar is written, and after a load it is read. Each also tells the
//! live loadout table ([`crate::controllers::loadout::live`]) that a reset began or ended, a store
//! is imminent, or a load happened.
//!
//! What the reset is for comes from the seams before it: the player's respawn
//! ([`Player::ReInit`], which revives the player and teleports with the load state: a death, a
//! mission's failure or restart, the teleport to a mission's start, and the return to a challenge's
//! start) makes it a respawn, the frontend's reset ([`GameStateFrontend::ResetSystems`], the quit
//! to the menu) and a load of the save data make it a load, and any other entry to the load state
//! (a fast travel, a challenge's start, retry, or abort) is a travel.
//!
//! The handler runs for every character's inventory, on whatever thread resets the spawn system,
//! so the detour compares the inventory's character with the published local player before it
//! takes any lock.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::{
    character::{inventory::Inventory, player::Player},
    event::{EventImpl, EventMatches},
    save::PlayerSaveData,
    types::math::Matrix4,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        hands::ik,
        loadout::live,
        weapons::inventory::{self, reconcile, restore},
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&HANDLE_EVENT_BINDER)
        .with_static_binder(&RE_INIT_BINDER)
        .with_static_binder(&FRONTEND_RESET_SYSTEMS_BINDER)
        .with_static_binder(&STORE_PLAYER_SAVE_DATA_BINDER)
        .with_static_binder(&LOAD_PLAYER_SAVE_DATA_BINDER)
}

/// Which of the spawn system's events `event` is, for the local player's `inventory`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ResetEvent {
    Pre,
    Post,
}

/// The reset event `event` is, when `inventory` is the local player's.
///
/// # Safety
///
/// `inventory` must be null or live, and `event` the event its handler was given.
unsafe fn reset_event(inventory: *mut Inventory, event: *const EventImpl) -> Option<ResetEvent> {
    let inventory = unsafe { inventory.as_ref() }?;
    let local = ik::local_character();
    if local == 0 || inventory.m_Character as usize != local {
        return None;
    }
    if unsafe { EventMatches(event, &inventory.m_SpawnSystemPreReset) } {
        Some(ResetEvent::Pre)
    } else if unsafe { EventMatches(event, &inventory.m_SpawnSystemPostReset) } {
        Some(ResetEvent::Post)
    } else {
        None
    }
}

#[detour(address = jc3gi::character::inventory::Inventory::HandleEvent_ADDRESS)]
fn handle_event(inventory: *mut Inventory, event: *const EventImpl, data: *mut c_void) {
    let reset = unsafe { reset_event(inventory, event) };
    if reset == Some(ResetEvent::Pre) {
        tracing::info!(target: "controllers", "inventory: the spawn system is resetting");
        inventory::on_reset();
    }
    HANDLE_EVENT.get().unwrap().call(inventory, event, data);
    if reset == Some(ResetEvent::Post) {
        tracing::info!(target: "controllers", "inventory: the spawn system has reset");
        restore::on_post_reset();
        live::reset_ended();
    }
}

#[detour(address = jc3gi::save::StorePlayerSaveData_ADDRESS)]
fn store_player_save_data(save_data: *const PlayerSaveData, archive: *mut c_void) {
    let settings = Config::lock_query(|c| c.controllers.weapons.inventory);
    if let Some(store) = live::store_imminent() {
        // SAFETY: the store runs on the game thread, outside the world update.
        unsafe { reconcile::store(&store) };
    }
    // SAFETY: the store is given live save data.
    unsafe { restore::on_store(save_data, &settings) };
    STORE_PLAYER_SAVE_DATA
        .get()
        .unwrap()
        .call(save_data, archive);
}

#[detour(address = jc3gi::save::LoadPlayerSaveData_ADDRESS)]
fn load_player_save_data(save_data: *mut PlayerSaveData, archive: *mut c_void, version: u8) {
    LOAD_PLAYER_SAVE_DATA
        .get()
        .unwrap()
        .call(save_data, archive, version);
    let settings = Config::lock_query(|c| c.controllers.weapons.inventory);
    // SAFETY: the load was given live save data.
    unsafe { restore::on_load(save_data, &settings) };
    live::loaded();
}

/// The player's respawn: the spawn reset its teleport enters is a respawn. Only the local player is
/// a `CPlayer`.
#[detour(address = jc3gi::character::player::Player::ReInit_ADDRESS)]
fn re_init(player: *mut Player, transform: *mut Matrix4, teleport_flags: u32, reset_flags: u32) {
    tracing::info!(target: "controllers", "inventory: the game is respawning the player");
    live::respawning();
    RE_INIT
        .get()
        .unwrap()
        .call(player, transform, teleport_flags, reset_flags);
}

/// The frontend's reset of the game's systems, the quit to the menu: whatever follows starts the
/// table again, and no reset's record of the carried weapons carries into it.
#[detour(address = jc3gi::game::GameStateFrontend::ResetSystems_ADDRESS)]
fn frontend_reset_systems() {
    tracing::info!(target: "controllers", "inventory: the frontend is resetting the game's systems");
    live::frontend_reset();
    restore::on_frontend();
    FRONTEND_RESET_SYSTEMS.get().unwrap().call();
}
