//! FSR configuration. See `docs/mod/rendering/fsr.md`.

use serde::{Deserialize, Serialize};

/// FSR anti-aliasing / upscaling settings. When `enabled`, FSR runs in place of the engine's SMAA
/// (which is suppressed); off restores the engine AA. See `docs/mod/rendering/fsr.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FsrConfig {
    /// Master switch: run FSR and suppress the engine AA. Off = engine SMAA as normal, FSR idle.
    pub enabled: bool,
    /// Run the RCAS sharpening pass after the upscale, at [`sharpness`](Self::sharpness).
    pub sharpen: bool,
    /// The RCAS sharpening strength (0..1) while [`sharpen`](Self::sharpen) is on. A separate
    /// switch rather than an `Option`, so turning sharpening off keeps the strength, and the
    /// settings file (which cannot write a missing value) can save either state.
    pub sharpness: f32,
}
impl FsrConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            sharpen: true,
            sharpness: 0.2,
        }
    }
}
