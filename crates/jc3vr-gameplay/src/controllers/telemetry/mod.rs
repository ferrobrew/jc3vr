//! The notes the controller subsystems write into a tracking recording as they act, for diagnosing
//! tracking and hand-control problems after the fact. Each producer calls in where it acts, and
//! nothing is built unless a recording or the lookback collects
//! ([`jc3vr_render::recorder::tracking::events::collecting`]).
//!
//! The refusals, forces, injected actions, and haptic cues are [`notes`]; a throw's or a release's
//! moment is [`moments`]; the grounded rig's frames are [`rig`]; and the arm joints of the drawn
//! body are [`drawn_arms`]. The samplers that read the game's state on their own schedule are
//! `diagnostics`'s.

mod drawn_arms;
mod moments;
mod notes;
mod rig;

pub(crate) use crate::controllers::telemetry::{
    drawn_arms::{drawn_arm_joints, sample_drawn_arms},
    moments::{Released, Thrown, note_release, note_throw, note_throw_at},
    notes::{Applied, BodyBefore, Cue, Write, note_force, note_haptic, note_injected, report},
    rig::{RigRun, note_rig},
};
