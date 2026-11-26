//! The payload's bottom layer: what every other layer builds on, naming nothing above it.
//!
//! The crate owns the payload's process-wide plumbing (`docs/architecture.md`, "Crate layering"):
//!
//! - [`lifecycle`]: the retryable shutdown preparation, its participants, the one-shot cleanups,
//!   the shutdown flag ([`is_shutting_down`]), and the build-stamp cell the payload fills at
//!   startup.
//! - [`config`]: the configuration engine, which holds the settings as an erased root behind one
//!   lock, loads and saves `settings.toml`, and hands each subsystem a typed slot. The settings
//!   aggregate itself lives in the payload.
//! - [`hooks`]: the hook plumbing, which installs a composed hook library under thread suspension
//!   and removes it, the shared `Patcher`, the installation and removal steps other layers
//!   register, the gameplay predicate, and the online timing detours ([`hooks::online`]).
//! - [`frame_timing`] and [`render_frame`]: the always-on frame breakdown and the per-frame slots.
//! - `profiler` (with the `profiler` feature): the puffin scopes, the frame advance, and the trace
//!   capture.
//! - [`session`] and [`logging`]: the session folder and the log.
//! - [`thread_role`], [`cpu_placement`], [`module`], [`hash`], [`util`], and
//!   [`director::clean_view`]: small process-wide services.
//!
//! The `test-support` feature exposes the fixtures other crates' tests use: the configuration
//! fixture and the lifecycle harness.

pub mod config;
pub mod cpu_placement;
pub mod director;
pub mod frame_timing;
pub mod hash;
pub mod hooks;
pub mod lifecycle;
pub mod logging;
pub mod module;
#[cfg(feature = "profiler")]
pub mod profiler;
pub mod render_frame;
pub mod session;
pub mod thread_role;
pub mod util;

pub use crate::lifecycle::is_shutting_down;
