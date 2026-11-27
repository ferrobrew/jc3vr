//! Keep native below-vehicle attachment from snapping a current physical handhold onto a roof, and
//! tell the subsystems that hold Havok bodies when the world destroys some.
//!
//! The subsystems contribute what they need at installation: the refusals of the automatic entry
//! into a vehicle's stunt area below the player
//! ([`crate::contributions::BodyContributions::auto_attach_blockers`]), any one of which refuses
//! it, and the observer of a body destruction
//! ([`crate::contributions::BodyContributions::bodies_destroying`]), called before the world
//! destroys them.

use detours_macro::detour;
use jc3gi::{character::character::Character, physics::hknpWorld};
use re_utilities::hook_library::HookLibrary;

use crate::contributions;

/// A refusal of the automatic attachment: called with the character the engine is about to
/// attach, it returns whether the attachment is refused. It is called from the detour on the
/// attachment, while the native caller keeps the character and its proxy member alive; it may read
/// only members of that character.
pub type BlocksAutoAttach = unsafe fn(*const Character) -> bool;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&AUTO_ATTACH_BELOW_BINDER)
        .with_static_binder(&DESTROY_BODIES_BINDER)
}

#[detour(address = jc3gi::character::character::Character::AutoAttachToStuntAreaOnVehicleBelow_ADDRESS)]
fn auto_attach_below(character: *mut Character) -> bool {
    // The engine's ground-body support also advertises a vehicle below a physical grip. Only
    // this automatic entry is suppressed; ordinary grapple hang and explicit interactions remain native.
    let blocked = contributions::get().is_some_and(|contributions| {
        contributions
            .auto_attach_blockers
            .iter()
            .any(|blocks| unsafe { blocks(character) })
    });
    if blocked {
        return false;
    }
    AUTO_ATTACH_BELOW.get().unwrap().call(character)
}

#[detour(address = jc3gi::physics::hknpWorld::destroyBodies_ADDRESS)]
fn destroy_bodies(world: *mut hknpWorld, ids: *const u32, count: i32, activation_mode: i32) {
    if let Some(observer) =
        contributions::get().and_then(|contributions| contributions.bodies_destroying)
    {
        unsafe { observer(world, ids, count) };
    }
    DESTROY_BODIES
        .get()
        .unwrap()
        .call(world, ids, count, activation_mode);
}
