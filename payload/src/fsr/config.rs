//! FSR configuration. See `docs/mod/rendering/fsr.md`.

use serde::{Deserialize, Serialize};

/// FSR anti-aliasing / upscaling settings. When `enabled`, FSR runs in place of the engine's SMAA
/// (which is suppressed); off restores the engine AA. See `docs/mod/rendering/fsr.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FsrConfig {
    /// Master switch: run FSR and suppress the engine AA. Off = engine SMAA as normal, FSR idle.
    pub enabled: bool,
    /// Run the RCAS sharpening pass after the upscale, at [`sharpness`](Self::sharpness). Off by
    /// default: with the jitter, FSR's own resolve is already sharper than the game's SMAA against a
    /// supersampled reference, and the pass only moved the image further from it, added spurious
    /// gradient, and made the still image change more from frame to frame (see "Sharpening" in
    /// `docs/mod/rendering/fsr.md`).
    pub sharpen: bool,
    /// The RCAS sharpening strength (0..1) while [`sharpen`](Self::sharpen) is on. A separate
    /// switch rather than an `Option`, so turning sharpening off keeps the strength, and the
    /// settings file (which cannot write a missing value) can save either state.
    pub sharpness: f32,
    /// Add each eye's parallax from the centre camera to the decoded motion vectors, for a velocity
    /// buffer encoded against the centre camera's previous view-projection (see
    /// `docs/mod/rendering/fsr.md`). Every pixel the velocity pass does not draw holds zero motion,
    /// which the correction turns into that parallax.
    pub stereo_mv_correction: bool,
    /// Give every pixel the velocity pass does not draw (and every pixel whose encoded motion hit
    /// the encoding's clamp) the camera's motion, computed from its depth through each eye's own
    /// previous view-projection, keeping the engine's object motion where it wrote some. Without
    /// it most of the frame reaches FSR with no camera motion at all.
    #[serde(default = "default_hybrid_velocity")]
    pub hybrid_velocity: bool,
    /// Jitter each eye's projection by a sub-pixel offset of FSR's Halton sequence every VR frame
    /// and tell FSR the offset, so its accumulation gathers sub-pixel samples rather than filtering
    /// one sample position (see "Sub-pixel jitter" in `docs/mod/rendering/fsr.md`). Only while FSR
    /// runs.
    #[serde(default = "default_jitter")]
    pub jitter: bool,
}
impl FsrConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            sharpen: false,
            sharpness: 0.2,
            stereo_mv_correction: true,
            hybrid_velocity: true,
            jitter: true,
        }
    }
}

fn default_hybrid_velocity() -> bool {
    FsrConfig::new().hybrid_velocity
}

fn default_jitter() -> bool {
    FsrConfig::new().jitter
}
