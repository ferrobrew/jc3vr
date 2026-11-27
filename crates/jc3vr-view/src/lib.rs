//! The payload's view layer: the OpenXR runtime, the head pose, the engine camera, and the frame
//! state the render layer reads per draw, naming nothing above it.
//!
//! The crate sits above the core and input crates and below the render and body crates and
//! everything above them (`docs/architecture.md`, "Crate layering"). It owns:
//!
//! - [`vr`]: the OpenXR runtime, its frame loop and deferred tail, the input sampling, the eye
//!   blit, the foveation, the desktop mirror, and the per-eye render parameters.
//! - [`headpose`]: the head pose the camera hooks read, its two sources, its anchors and follow
//!   model, and the gameplay participants and body frame filter the layers above register.
//! - [`hooks`]: the camera and window-focus detours, and the dependency-free registries the render
//!   crate's graphics-engine hooks read and the layers above fill (the eye captures, the post-draw,
//!   the palette edit, and the player's draw inputs).
//! - [`stereo`]: the frame's stereo state, the dispatch, the engine context, the layout, and the
//!   stereo settings; [`fsr`]: the upscaler's settings and jitter.
//! - [`controllers`] and [`hud`]: the data the layers above hand the render passes: the cockpit
//!   transform, the overlay's data model and inputs, the scope's lens frame, the rig's origin
//!   provenance, the render camera's pose, the panel math, and the stage frame.
//! - [`recorder`]: the `.jc3trk` tracking recorder.
//! - [`gpu_passes`]: the benchmark's per-pass GPU totals.
//! - [`tuning`]: the crate's tuning files, and the input crate's haptics file it loads.
//!
//! The layers above reach these modules by path. [`CAPTURE_VS`] is the full-screen vertex shader
//! every capture pass draws with, exported for the passes that live above.

pub mod controllers;
pub mod fsr;
pub mod gpu_passes;
pub mod headpose;
pub mod hooks;
pub mod hud;
pub mod recorder;
pub mod stereo;
pub mod tuning;
pub mod vr;

#[cfg(test)]
mod test_config;

/// The full-screen triangle vertex shader the capture, blit, mirror, and composite passes draw
/// with (`shaders/capture_vs.hlsl`).
pub const CAPTURE_VS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/capture_vs.dxbc"
));
