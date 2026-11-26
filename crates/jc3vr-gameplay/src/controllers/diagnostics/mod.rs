//! Re-exports of the diagnostics at their old path, for the consumers not yet moved: the note API
//! is [`crate::controllers::telemetry`]'s, the samplers are `diagnostics`'s, and the
//! settings note is the tracking recorder's.

pub use jc3vr_render::recorder::tracking::note_settings;

pub(crate) use crate::controllers::telemetry::{
    Applied, BodyBefore, Cue, Released, RigRun, Thrown, Write, drawn_arm_joints, note_force,
    note_haptic, note_injected, note_release, note_rig, note_throw, note_throw_at, report,
    sample_drawn_arms,
};
