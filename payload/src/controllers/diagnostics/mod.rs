//! The controller subsystems' derived state as tracking-recording events, for diagnosing tracking
//! and hand-control problems after the fact.
//!
//! The game-thread hook calls [`record`] after the engine's render update, so the late hand
//! snapshot and the frame's sub-tick fraction it is evaluated at are the ones that frame rendered
//! with. While a tracking recording or the lookback collects
//! ([`crate::recorder::tracking::events::collecting`]), each call notes one
//! [`ControllerState`](tracking_format::event::ControllerState) event, which the recorder stamps on
//! the frame timeline and writes beside the next frame, and a
//! [`BodyState`](tracking_format::event::BodyState) event when the body's animation state changed
//! ([`body_state`]). The controller state combines independently read subsystem snapshots; it does
//! not claim an atomic cross-subsystem instant; its world groups are [`world_state`]'s.
//!
//! The rest of the recording's evidence is noted where it happens: Rico's pose before and after the
//! solves ([`pose`]), the grounded rig's frames ([`rig`]), the bodies near Rico and what his hands
//! hold after each simulation step ([`bodies`]), and the refusals, forces, injected actions, haptic
//! cues, settings, and frame timing ([`notes`]).

mod bodies;
mod body_state;
mod moments;
mod notes;
mod pose;
mod rig;
mod sample;
mod state_names;
mod world_state;

use std::time::Instant;

use jc3gi::character::character::Character;

pub(crate) use crate::controllers::diagnostics::{
    bodies::after_step,
    moments::{Released, Thrown, note_release, note_throw, note_throw_at},
    notes::{
        Applied, BodyBefore, Cue, Write, note_diagnostic, note_force, note_frame_timing,
        note_haptic, note_injected, note_settings,
    },
    pose::{sample_final, sample_pre_ik},
    rig::{RigRun, note_rig},
};
use crate::{
    controllers::body::procedural,
    hooks::animation::active_state_type_id,
    recorder::tracking::events::{self, EventKind},
};

/// Note the controller subsystems' state when a recording or the lookback collects it.
pub fn record() {
    if crate::is_shutting_down() || !events::collecting() {
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
