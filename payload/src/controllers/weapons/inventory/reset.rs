//! The spawn system's reset, seen through the local player's inventory's event handler
//! ([`Inventory::HandleEvent`]). The reset sends its pre-reset event
//! ([`Inventory::m_SpawnSystemPreReset`]) at once, before it releases its references to every
//! spawned object, and the inventory then records its slots for the respawn. The detour releases
//! everything the VR inventory holds at that event, so that no weapon it pinned outlives the reset
//! outside the spawn system.
//!
//! The handler runs for every character's inventory, on whatever thread resets the spawn system,
//! so the detour compares the inventory's character with the published local player before it
//! takes any lock.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::{
    character::inventory::Inventory,
    event::{EventImpl, EventMatches},
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{hands::ik, weapons::inventory};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&HANDLE_EVENT_BINDER)
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
    }
}
