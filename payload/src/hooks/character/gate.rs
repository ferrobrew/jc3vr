//! The HumanIK solve gate: a pre-call detour on `HumanIK::HasTargets`, whose one caller is the
//! pose-finalization pass, where the procedural body writes the local player's pose
//! ([`at_solve_gate`]) and the controllers' wrist targets are queued on it ([`queue_deferred`]).
//!
//! The gate has no step of its own, so the pass's entry records the pass's (`begin_pass`) on the
//! thread running it, and the gate reads it from there and marks the pass as gated. The engine
//! reaches the gate only with the global HumanIK switch on and the character's reduced level of
//! detail off, so the pass's end (`end_pass`) tells the caller whether it ran.
//!
//! The engine places the weapons on the solved pose only when the gate reports a target
//! ([`Character::PROP_TRANSFORM_AFTER_SOLVE`]); a procedural pose written with no target queued
//! has them placed on it here instead, so the gun never stays where the animated hand held it.

use std::{cell::Cell, mem::offset_of};

use detours_macro::detour;
use jc3gi::{
    animation::ik::{HumanIK, Pass},
    character::character::Character,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::{body::procedural::at_solve_gate, hands::ik::queue_deferred},
    frame_timing::{self, Slot},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&HUMAN_IK_HAS_TARGETS_BINDER)
}

#[detour(address = jc3gi::animation::ik::HumanIK::HasTargets_ADDRESS)]
fn human_ik_has_targets(hik: *mut HumanIK, pass: Pass) -> bool {
    if pass == Pass::MAIN && !hik.is_null() {
        // The solver is embedded in its character, so the character is the solver's address less
        // the field's offset. Only the local player's is acted on.
        let character = hik
            .cast::<u8>()
            .wrapping_sub(offset_of!(Character, m_HIK))
            .cast::<Character>();
        // SAFETY: every solver the gate is called with is a character's `m_HIK`, and the pass runs
        // on the thread that owns that character for its duration.
        if let Some(character) = unsafe { character.as_mut() }.filter(|c| c.m_IsLocalCharacter) {
            let _timing = frame_timing::scope(Slot::BodyGate);
            let dt = PASS.get().map_or(0.0, |pass| pass.dt);
            if let Some(pass) = PASS.get() {
                PASS.set(Some(PassContext {
                    gate_ran: true,
                    ..pass
                }));
            }
            let wrote = unsafe { at_solve_gate(character, dt) };
            // The wrist targets kept for the gate are queued on the torso it just wrote.
            unsafe { queue_deferred(character) };
            drop(_timing);
            let has_targets = HUMAN_IK_HAS_TARGETS.get().unwrap().call(hik, pass);
            if wrote && !has_targets {
                let _timing = frame_timing::scope(Slot::BodyGate);
                // SAFETY: the pass's own call, at the point the engine places the weapons after a
                // solve, on the thread running it.
                unsafe { character.UpdatePropTransforms(Character::PROP_TRANSFORM_AFTER_SOLVE) };
            }
            return has_targets;
        }
    }
    HUMAN_IK_HAS_TARGETS.get().unwrap().call(hik, pass)
}

/// Record the step of the local player's pose-finalization pass that starts on this thread, for
/// its solve gate. Called at the pass's entry.
pub(super) fn begin_pass(dt: f32) {
    PASS.set(Some(PassContext {
        dt,
        gate_ran: false,
    }));
}

/// Forget the pass [`begin_pass`] recorded, and say whether its solve gate ran: `None` for a pass
/// that was not the local player's. Called at the pass's end.
pub(super) fn end_pass() -> Option<bool> {
    PASS.take().map(|pass| pass.gate_ran)
}

/// The local player's pass in progress on this thread.
#[derive(Debug, Copy, Clone, PartialEq)]
struct PassContext {
    /// The pass's step, seconds.
    dt: f32,
    /// Whether the pass reached its solve gate.
    gate_ran: bool,
}

thread_local! {
    static PASS: Cell<Option<PassContext>> = const { Cell::new(None) };
}
