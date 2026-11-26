//! The payload's render layer: the per-eye rendering and the engine-pipeline hooks that put the
//! game on the headset, and the diagnostics that measure them, naming nothing above it.
//!
//! The crate sits above the core, input, and view crates (`docs/architecture.md`, "Crate
//! layering"). The view crate below it holds the OpenXR runtime, the head pose, the camera, and the
//! frame state these modules read. This crate owns:
//!
//! - [`stereo`], [`fsr`], and [`far_field`]: the per-eye rendering (multipass and single-pass),
//!   the upscaler and anti-aliasing chain, and the far-field share.
//! - [`hooks`]: the engine-pipeline detours (the graphics engine and its passes, the clock, the
//!   draw counts, and, with the `profiler` feature, the engine-phase scopes).
//! - [`controllers`]: the render-side pieces of the layers above: the overlay and scope renderers
//!   and passes, and their registration in the post-draw.
//! - [`debug`], [`screenshot`], [`image_writer`], [`testing`], and [`profiler`]: the render
//!   diagnostics, the capture harness, the GPU dispatch records, and, with the `profiler` feature,
//!   the GPU timestamp lane.
//!
//! The layers above reach these modules by path.

mod capture_arming;
pub mod controllers;
pub mod debug;
pub mod far_field;
pub mod fsr;
pub mod hooks;
pub mod image_writer;
pub mod profiler;
pub mod screenshot;
pub mod stereo;
pub mod testing;

#[cfg(test)]
mod test_config;
