//! The `Character::QueueAct` entry of the local player's act arbiter
//! ([`crate::controllers::acts`]), and the act-name resolution the act queuers share.

use detours_macro::detour;
use jc3gi::{animation::symbol_table::EventIdSymbolTable, character::character::Character};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::acts::{ActDecision, filter_character};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CHARACTER_QUEUE_ACT_BINDER)
}

/// The local player's acts go through the arbiter's character entry
/// ([`filter_character`]), which drops or replaces them; every other character's pass.
#[detour(address = jc3gi::character::character::Character::QueueAct_ADDRESS)]
fn character_queue_act(character: *mut Character, act: *const u32) {
    if let Some(local) = (unsafe { character.as_mut() }).filter(|c| c.m_IsLocalCharacter)
        && let Some(&id) = (unsafe { act.as_ref() })
    {
        // SAFETY: the local character, on the thread queuing its act.
        match unsafe { filter_character(local, id as i32) } {
            ActDecision::Pass => {}
            ActDecision::Drop => return,
            ActDecision::Replace(replacement) => {
                let replacement = replacement as u32;
                CHARACTER_QUEUE_ACT
                    .get()
                    .unwrap()
                    .call(character, &replacement);
                return;
            }
        }
    }
    CHARACTER_QUEUE_ACT.get().unwrap().call(character, act);
}

/// Resolve an `ACT_*` name to its runtime id via the event-id symbol table, or `None` if the table
/// is not live yet (e.g. immediately after a startup injection, before the animation system has
/// created it). Once the table exists the lookup is a pure read: the `ACT_*` names are registered by
/// loaded animation data before gameplay queues acts.
#[inline]
pub fn resolve_act_id(name: &std::ffi::CStr) -> Option<i32> {
    let table = unsafe { EventIdSymbolTable::get() }?;
    Some(unsafe { table.string_to_id(name.as_ptr() as *const u8) })
}
