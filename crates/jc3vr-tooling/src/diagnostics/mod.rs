//! The diagnostics samplers: the controller subsystems' derived state, Rico's pose, and the bodies
//! around him as tracking-recording events, for diagnosing tracking and hand-control problems after
//! the fact.
//!
//! The frame driver calls [`record`] after the engine's render update, so the late hand snapshot
//! and the frame's sub-tick fraction it is evaluated at are the ones that frame rendered with.
//! While a tracking recording or the lookback collects
//! ([`jc3vr_view::recorder::events::collecting`]), each call notes one
//! [`ControllerState`](tracking_format::event::ControllerState) event, which the recorder stamps on
//! the frame timeline and writes beside the next frame, and a
//! [`BodyState`](tracking_format::event::BodyState) event when the body's animation state changed
//! ([`body_state`]). The controller state combines independently read subsystem snapshots; it does
//! not claim an atomic cross-subsystem instant; its world groups are [`world_state`]'s.
//!
//! The other samplers observe the game where it acts, through the observers the crate's
//! registration installs ([`crate::register()`]): Rico's pose before and after the solves
//! ([`pose`]), at the character hooks' pose-finalization observer, and the bodies near Rico and
//! what his hands hold ([`bodies`]), after each simulation step. The notes the controller subsystems write as they act (refusals, forces, injected actions,
//! haptic cues, the rig's frames) are [`jc3vr_body::controllers::telemetry`]'s, and the
//! settings and the frame timing are the tracking recorder's own.

pub(crate) mod bodies;
mod body_state;
pub(crate) mod pose;
mod sample;
mod state_names;
mod world_state;

use std::time::Instant;

use jc3gi::character::character::Character;
use jc3vr_body::{controllers::body::procedural, hooks::animation::active_state_type_id};
use jc3vr_view::recorder::{events, events::EventKind};

/// Note the controller subsystems' state when a recording or the lookback collects it.
pub fn record() {
    if jc3vr_core::is_shutting_down() || !events::collecting() {
        body_state::forget();
        return;
    }
    // SAFETY: the game thread, after the render update and outside the world update, where the
    // local player and its rule systems are stable for the reads.
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() };
    let rule_state = character.and_then(|character| unsafe { active_state_type_id(character) });
    body_state::note(
        rule_state,
        procedural::published_status().case,
        Instant::now(),
    );
    events::note(EventKind::ControllerState(Box::new(sample::snapshot(
        character,
    ))));
}
