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
//! - [`debug`], [`screenshot`], [`image_writer`], and [`profiler`]: the render diagnostics, the
//!   F12 screenshot, the image writer, the GPU dispatch records, and, with the `profiler` feature,
//!   the GPU timestamp lane.
//! - [`tuning`]: the render layer's tuning file.
//! - [`capture_arming`] and the probe tables in [`stereo::single_pass::probes`] and
//!   [`hooks::graphics_engine::probes`]: the seams the capture crate above fills at install, read
//!   with one relaxed load while nothing has asked for a capture or a benchmark.
//!
//! The layers above reach these modules by path.

pub mod capture_arming;
pub mod controllers;
pub mod debug;
pub mod fsr;
pub mod hooks;
pub mod image_writer;
pub mod profiler;
pub mod screenshot;
pub mod stereo;
pub mod tuning;

#[cfg(test)]
mod test_config;
