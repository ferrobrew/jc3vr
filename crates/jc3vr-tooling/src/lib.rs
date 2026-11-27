//! The payload's tooling layer: the tools for recording, staging, and diagnosing a session, built
//! on the gameplay, render, and capture layers and naming nothing above them.
//!
//! The crate sits above the gameplay and capture crates and beside the HUD crate, with no edge
//! between the two (`docs/architecture.md`, "Crate layering"). It owns:
//!
//! - [`recorder`]: the video recorder: a take's sidecar, the eye views streamed to the capture
//!   host, and the session recording's take markers. The tracking recorder is the view crate's
//!   ([`jc3vr_view::recorder`]).
//! - [`director`]: the directing tools: time control, actor safety, heat, the teleport bookmarks,
//!   the take markers, and the key bindings that drive them. Its pass also drives the core crate's
//!   clean view ([`jc3vr_core::clean_view`]) and the weapons crate's spawner
//!   ([`jc3vr_weapons::controllers::spawn`]), whose settings it holds.
//! - [`testing`]: the capture harness's request file, the staging of a scene, its text, and the
//!   testing settings. It drives the capture crate's visual stage and benchmark
//!   ([`jc3vr_capture::testing`]).
//! - [`diagnostics`]: the samplers that note the controller subsystems' state, Rico's pose, and
//!   the bodies around him into a tracking recording.
//! - [`tuning`]: the tooling's tuning file ([`tuning::TOOLING`], `docs/mod/tuning.md`).
//!
//! # The public surface
//!
//! The layers above reach these modules by path, so an item is `pub` exactly when a crate above
//! names it: mostly the debug UI's panels and status rows, the window procedure's hotkeys, and the
//! payload's frame driver, composition, and settings aggregate. Everything else keeps its
//! crate-private visibility.
//!
//! The crate defines no detours. Its registrations with the layers below it (the recorder's mirror
//! observer, eye-frame recorder, and take-marker handler, the diagnostics' observers, and the
//! directing tools' start-marker source) are gathered in [`register()`], which the payload's
//! composition calls once before the hooks are installed. Each group's settings slot
//! ([`recorder::RECORDER_CONFIG`], [`director::DIRECTOR_CONFIG`], and [`testing::TESTING_CONFIG`])
//! sits beside the group's type, and the payload binds them to its aggregate.

pub mod diagnostics;
pub mod director;
pub mod recorder;
mod register;
pub mod testing;
pub mod tuning;

pub use crate::register::register;
