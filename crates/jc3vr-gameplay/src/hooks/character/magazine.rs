//! Deferred magazine detachment at the character's native serial post-physics entry: the weapon
//! handling registers its consumer of the detachment requests ([`register_serial_consumer`]), which
//! runs at that entry before the native message poll.

use std::sync::{
    OnceLock,
    atomic::{AtomicUsize, Ordering},
};

use detours_macro::detour;
use jc3gi::{character::character::Character, game::GameObjectUpdateContext};
use re_utilities::hook_library::HookLibrary;

/// Register the consumer of the deferred magazine detachments. One consumer: a second registration
/// is refused and logged.
///
/// The consumer is called only at the native serial post-physics entry, with its native arguments,
/// for every character.
pub fn register_serial_consumer(consumer: unsafe fn(*mut Character, *mut GameObjectUpdateContext)) {
    if SERIAL_CONSUMER.set(consumer).is_err() {
        tracing::error!(
            "character: a magazine serial consumer is already registered; keeping the first"
        );
    }
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CHARACTER_UPDATE_PASS_POST_PHYSICS_SERIAL_BINDER)
}

pub(crate) fn active_entries() -> usize {
    ACTIVE_ENTRIES.load(Ordering::Acquire)
}

static ACTIVE_ENTRIES: AtomicUsize = AtomicUsize::new(0);

static SERIAL_CONSUMER: OnceLock<unsafe fn(*mut Character, *mut GameObjectUpdateContext)> =
    OnceLock::new();

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
    if let Some(consume) = SERIAL_CONSUMER.get() {
        unsafe { consume(character, context) };
    }
    CHARACTER_UPDATE_PASS_POST_PHYSICS_SERIAL
        .get()
        .unwrap()
        .call(character, context);
}
