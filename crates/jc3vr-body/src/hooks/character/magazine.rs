//! Deferred magazine detachment at the character's native serial post-physics entry: the weapon
//! handling registers its consumer of the detachment requests
//! ([`crate::contributions::BodyContributions::serial_consumer`]), which runs at that entry before
//! the native message poll.

use std::sync::atomic::{AtomicUsize, Ordering};

use detours_macro::detour;
use jc3gi::{character::character::Character, game::GameObjectUpdateContext};
use re_utilities::hook_library::HookLibrary;

use crate::contributions;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CHARACTER_UPDATE_PASS_POST_PHYSICS_SERIAL_BINDER)
}

#[inline]
pub fn active_entries() -> usize {
    ACTIVE_ENTRIES.load(Ordering::Acquire)
}

static ACTIVE_ENTRIES: AtomicUsize = AtomicUsize::new(0);

struct Entry;

impl Drop for Entry {
    fn drop(&mut self) {
        ACTIVE_ENTRIES.fetch_sub(1, Ordering::Release);
    }
}

#[detour(address = jc3gi::character::character::Character::UpdatePassPostPhysics_Serial_ADDRESS)]
fn character_update_pass_post_physics_serial(
    character: *mut Character,
    context: *mut GameObjectUpdateContext,
) {
    ACTIVE_ENTRIES.fetch_add(1, Ordering::AcqRel);
    let _entry = Entry;
    // SAFETY: only the registered native serial post-physics invocation enters this detour.
    if let Some(consume) =
        contributions::get().and_then(|contributions| contributions.serial_consumer)
    {
        unsafe { consume(character, context) };
    }
    CHARACTER_UPDATE_PASS_POST_PHYSICS_SERIAL
        .get()
        .unwrap()
        .call(character, context);
}
