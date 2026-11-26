//! The payload's render layer: everything that puts the game on the headset, and the diagnostics
//! that measure it, naming nothing above it.
//!
//! The crate sits above the core and input crates (`docs/architecture.md`, "Crate layering") and
//! owns:
//!
//! - [`vr`]: the OpenXR runtime, its frame loop and deferred tail, the input sampling, the eye
//!   blit, the foveation, and the desktop mirror.
//! - [`headpose`]: the head pose the camera hooks read, and its anchors and follow model.
//! - [`stereo`], [`fsr`], and [`far_field`]: the per-eye rendering (multipass and single-pass),
//!   the upscaler and anti-aliasing chain, and the far-field share.
//! - [`hooks`]: the engine-pipeline detours (the graphics engine and its passes, the camera, the
//!   clock, the draw counts, the window focus, and, with the `profiler` feature, the engine-phase
//!   scopes).
//! - [`controllers`] and [`hud`]: the render-side pieces of the layers above, which those layers
//!   feed: the cockpit transform, the overlay and scope renderers and passes, the rig's origin
//!   provenance, the render camera's pose, the panel math, and the stage frame.
//! - [`recorder::tracking`]: the `.jc3trk` tracking recorder.
//! - [`debug`], [`screenshot`], [`image_writer`], [`testing`], and [`profiler`]: the render
//!   diagnostics, the capture harness, and, with the `profiler` feature, the GPU timestamp lane.
//!
//! The layers above reach these modules by path. [`CAPTURE_VS`] is the full-screen vertex shader
//! every capture pass draws with, exported for the passes that live above.

mod capture_arming;
pub mod controllers;
pub mod debug;
pub mod far_field;
pub mod fsr;
pub mod headpose;
pub mod hooks;
pub mod hud;
pub mod image_writer;
pub mod profiler;
pub mod recorder;
pub mod screenshot;
pub mod stereo;
pub mod testing;
pub mod vr;

#[cfg(test)]
mod test_config;

/// The full-screen triangle vertex shader the capture, blit, mirror, and composite passes draw
/// with (`shaders/capture_vs.hlsl`).
pub const CAPTURE_VS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/capture_vs.dxbc"
));
