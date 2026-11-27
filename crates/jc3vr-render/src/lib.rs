//! The payload's render layer: the per-eye rendering and the engine-pipeline hooks that put the
//! game on the headset, and the diagnostics that measure them, naming nothing above it.
//!
//! The crate sits above the core, input, and view crates (`docs/architecture.md`, "Crate
//! layering"). The view crate below it holds the OpenXR runtime, the head pose, the camera, and the
//! frame state these modules read. This crate owns:
//!
//! - [`stereo`] and [`fsr`]: the per-eye rendering (multipass and single-pass) and the upscaler
//!   and anti-aliasing chain.
//! - [`hooks`]: the engine-pipeline detours (the graphics engine and its passes, the clock, the
//!   draw counts, and, with the `profiler` feature, the engine-phase scopes).
//! - [`controllers`]: the render-side pieces of the layers above: the overlay and scope renderers
//!   and passes, and their registration in the post-draw.
//! - [`debug`], [`screenshot`], [`readback`], [`image_writer`], and [`profiler`]: the render
//!   diagnostics, the F12 screenshot, the staging readbacks it and the capture harness read GPU
//!   resources through, the image writer, the GPU dispatch records, and, with the `profiler`
//!   feature, the GPU timestamp lane.
//! - [`tuning`]: the render layer's tuning file.
//! - [`draw_path`]: the plain load-and-store counter the draw path bumps.
//! - [`probes`]: the capture probes, the one table the capture crate above fills at install, read
//!   with one relaxed load while nothing has asked for a capture or a benchmark.
//! - [`register`]: what the crate registers with the layers below it (its steps at hook removal,
//!   the debug tools' taps, and the VR teardown's flip release), which the payload's composition
//!   calls once.
//!
//! The layers above reach these modules by path.

pub mod controllers;
pub mod debug;
pub(crate) mod draw_path;
pub mod fsr;
pub mod hooks;
pub mod image_writer;
pub mod probes;
pub mod profiler;
pub mod readback;
mod register;
pub mod screenshot;
pub mod stereo;
pub mod tuning;

pub use register::register;

#[cfg(test)]
mod test_config;
