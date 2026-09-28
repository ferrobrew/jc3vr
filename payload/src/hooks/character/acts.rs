//! The act filter: drops the local player's acts that the mod replaces.

use std::sync::OnceLock;

use detours_macro::detour;
use jc3gi::{animation::symbol_table::EventIdSymbolTable, character::character::Character};
use re_utilities::hook_library::HookLibrary;

use crate::{config::Config, controllers::aim_state};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CHARACTER_QUEUE_ACT_BINDER)
}

/// Filter the local player's acts:
///
/// - Drop the vehicle reversing look-behind acts: `ACT_REVERSE` (cars) and `ACT_REVERSE_MOTORBIKE`
///   drive the `S_REVERSE_*` states in `rico_base.afsmb`, where Rico turns to look over his
///   shoulder. With a player-driven head, looking behind is the player's job, so the act is
///   swallowed and the driving pose stays forward; the state machine's rule system drops the
///   matching return transitions on its own (they only fire from the reverse states).
/// - Drop the automatic holsters under the controller scheme's mental-state cuts (see
///   [`aim_state::swallow_local_act`]).
#[detour(address = jc3gi::character::character::Character::QueueAct_ADDRESS)]
fn character_queue_act(character: *mut Character, act: *const u32) {
    if (unsafe { character.as_ref() }).is_some_and(|c| c.m_IsLocalCharacter)
        && let Some(&id) = (unsafe { act.as_ref() })
    {
        let id = id as i32;
        let reverse = Config::lock_query(|c| c.movement.suppress_reverse_look)
            && reverse_act_ids().is_some_and(|ids| ids.contains(&id));
        if reverse || aim_state::swallow_local_act(id) {
            return;
        }
    }
    CHARACTER_QUEUE_ACT.get().unwrap().call(character, act);
}

/// The runtime ids of the reversing acts (`ACT_REVERSE` / `ACT_REVERSE_MOTORBIKE`): act ids are
/// sequential registration indices, not name hashes, so they cannot be computed offline. Resolved
/// the first time the event-id symbol table is live and cached thereafter; `None` until then — after
/// a startup injection the table may not exist yet, and the suppression simply does not fire until
/// it does (the acts are registered by loaded animation data before gameplay queues them).
fn reverse_act_ids() -> Option<[i32; 2]> {
    if let Some(ids) = REVERSE_ACT_IDS.get() {
        return Some(*ids);
    }
    let ids = [
        resolve_act_id(EventIdSymbolTable::ACT_REVERSE)?,
        resolve_act_id(EventIdSymbolTable::ACT_REVERSE_MOTORBIKE)?,
    ];
    Some(*REVERSE_ACT_IDS.get_or_init(|| ids))
}

static REVERSE_ACT_IDS: OnceLock<[i32; 2]> = OnceLock::new();

/// Resolve an `ACT_*` name to its runtime id via the event-id symbol table, or `None` if the table
/// is not live yet (e.g. immediately after a startup injection, before the animation system has
/// created it). Once the table exists the lookup is a pure read: the `ACT_*` names are registered by
/// loaded animation data before gameplay queues acts.
pub(crate) fn resolve_act_id(name: &std::ffi::CStr) -> Option<i32> {
    let table = unsafe { EventIdSymbolTable::get() }?;
    Some(unsafe { table.string_to_id(name.as_ptr() as *const u8) })
}
