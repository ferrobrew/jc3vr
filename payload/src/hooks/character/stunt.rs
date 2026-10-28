//! Keep native below-vehicle attachment from snapping a current physical handhold onto a roof.

use detours_macro::detour;
use jc3gi::{character::character::Character, physics::hknpWorld};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::physical_hands::climb::native;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&AUTO_ATTACH_BELOW_BINDER)
        .with_static_binder(&DESTROY_BODIES_BINDER)
}

#[detour(address = jc3gi::character::character::Character::AutoAttachToStuntAreaOnVehicleBelow_ADDRESS)]
fn auto_attach_below(character: *mut Character) -> bool {
    // The engine's ground-body support also advertises a vehicle below a physical grip. Only
    // this automatic entry is suppressed; ordinary grapple hang and explicit interactions remain native.
    if unsafe { native::blocks(character) } {
        return false;
    }
    AUTO_ATTACH_BELOW.get().unwrap().call(character)
}

#[detour(address = jc3gi::physics::hknpWorld::destroyBodies_ADDRESS)]
fn destroy_bodies(world: *mut hknpWorld, ids: *const u32, count: i32, activation_mode: i32) {
    if !ids.is_null() && count > 0 {
        for i in 0..count as usize {
            let body = unsafe { *ids.add(i) };
            crate::controllers::physical_hands::climb::lifetime::destroyed(world as usize, body);
        }
    }
    DESTROY_BODIES
        .get()
        .unwrap()
        .call(world, ids, count, activation_mode);
}
