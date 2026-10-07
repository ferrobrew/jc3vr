//! The state signals the procedural body's hand-over table needs that the character does not keep
//! (`controllers::body::procedural::signals`): a post-call on the grapple's ledge climb task.

use std::{ffi::c_void, time::Instant};

use detours_macro::detour;
use jc3gi::state::StateContext;
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::body::procedural::signals::note_ledge_climb,
    hooks::input::locomotion::character_from_context,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&LEDGE_CLIMB_UPDATE_BINDER)
}

#[detour(
    address = jc3gi::equipment::grappling_hook::NStateTask_ReeledInLedgeClimbMotionTask::Update_ADDRESS
)]
fn ledge_climb_update(ctx: *mut StateContext, p1: *mut c_void, p2: *mut c_void) {
    LEDGE_CLIMB_UPDATE.get().unwrap().call(ctx, p1, p2);
    // SAFETY: the task's context names its character, on the thread running its state machines.
    if unsafe { character_from_context(ctx) }.is_some_and(|c| c.m_IsLocalCharacter) {
        note_ledge_climb(Instant::now());
    }
}
