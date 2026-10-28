//! Observe native dispatch and preserve body-event flag intent without changing wrapper tables.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::{
    havok::hknpContactImpulseEvent,
    physics::{HavokCollisionListener, hknpWorld},
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::physical_hands::impacts::{self, state::FlagCall};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CONTACT_BINDER)
        .with_static_binder(&ENABLE_FLAGS_BINDER)
        .with_static_binder(&DISABLE_FLAGS_BINDER)
}

pub(super) unsafe fn apply(world: &mut hknpWorld, call: FlagCall) {
    if call.enable {
        ENABLE_FLAGS
            .get()
            .unwrap()
            .call(world, call.body, call.flags, call.mode);
    } else {
        DISABLE_FLAGS
            .get()
            .unwrap()
            .call(world, call.body, call.flags, call.mode);
    }
}

#[detour(address = HavokCollisionListener::onContactImpulseEvent_ADDRESS)]
fn contact(
    this: *mut HavokCollisionListener,
    input: *const c_void,
    event: *const hknpContactImpulseEvent,
) {
    unsafe { impacts::record(event) };
    CONTACT.get().unwrap().call(this, input, event);
}

#[detour(address = hknpWorld::enableBodyFlags_ADDRESS)]
fn enable_flags(world: *mut hknpWorld, body: u32, flags: u32, mode: i32) {
    let call = impacts::native_flags(FlagCall {
        world: world as usize,
        body,
        flags,
        mode,
        enable: true,
    });
    ENABLE_FLAGS
        .get()
        .unwrap()
        .call(world, body, call.flags, call.mode);
}

#[detour(address = hknpWorld::disableBodyFlags_ADDRESS)]
fn disable_flags(world: *mut hknpWorld, body: u32, flags: u32, mode: i32) {
    let call = impacts::native_flags(FlagCall {
        world: world as usize,
        body,
        flags,
        mode,
        enable: false,
    });
    DISABLE_FLAGS
        .get()
        .unwrap()
        .call(world, body, call.flags, call.mode);
}
