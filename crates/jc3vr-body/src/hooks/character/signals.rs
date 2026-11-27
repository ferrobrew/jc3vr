//! The state signals the procedural body's hand-over table needs that the character does not keep
//! (`controllers::body::procedural::signals`): a post-call on the grapple's ledge climb task, and a
//! pre-call on the animated model's act queue, which the hit reactions queue on directly. The same
//! pre-call is the act arbiter's model entry (`controllers::acts`), which drops the wingsuit's
//! barrel roll, which the air physics also queues there directly.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::{character::character::AnimatedModel, state::StateContext};
use jc3vr_core::published::Stamp;
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::{
        acts::{ActDecision, filter_model},
        body::procedural::signals::{note_act, note_ledge_climb},
    },
    hooks::input::locomotion::character_from_context,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&LEDGE_CLIMB_UPDATE_BINDER)
        .with_static_binder(&ANIMATED_MODEL_QUEUE_ACT_BINDER)
}

#[detour(
    address = jc3gi::equipment::grappling_hook::NStateTask_ReeledInLedgeClimbMotionTask::Update_ADDRESS
)]
fn ledge_climb_update(ctx: *mut StateContext, p1: *mut c_void, p2: *mut c_void) {
    LEDGE_CLIMB_UPDATE.get().unwrap().call(ctx, p1, p2);
    // SAFETY: the task's context names its character, on the thread running its state machines.
    if unsafe { character_from_context(ctx) }.is_some_and(|c| c.m_IsLocalCharacter) {
        note_ledge_climb(Stamp::now());
    }
}

#[detour(address = jc3gi::character::character::AnimatedModel::QueueAct_ADDRESS)]
fn animated_model_queue_act(model: *mut AnimatedModel, act: *const u32) {
    // SAFETY: the act is the caller's live `idstring`.
    if let Some(&id) = unsafe { act.as_ref() } {
        if filter_model(model, id as i32) == ActDecision::Drop {
            return;
        }
        note_act(model, id as i32);
    }
    ANIMATED_MODEL_QUEUE_ACT.get().unwrap().call(model, act);
}
