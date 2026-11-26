//! FSR configuration. See `docs/mod/rendering/fsr.md`.

use serde::{Deserialize, Serialize};

use crate::config::engine::ConfigSlot;

/// Which anti-aliasing resolves the eye images at the engine's anti-aliasing stage.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AntiAliasing {
    /// FSR 2 at 1:1, in place of the engine's SMAA, which is suppressed.
    Fsr,
    /// The mod's own temporal resolve (`fsr::taa`), in place of the engine's SMAA, which is
    /// suppressed. See `docs/mod/rendering/taa.md`.
    Taa,
    /// The engine's own anti-aliasing (SMAA 1x in stereo), with no jitter.
    Engine,
}

impl AntiAliasing {
    /// Whether a temporal resolve of the mod's runs, so the projections take the sub-pixel jitter.
    pub fn temporal(self) -> bool {
        matches!(self, Self::Fsr | Self::Taa)
    }
}

/// The upscaling and anti-aliasing settings, at `fsr` in the settings file; the payload binds the
/// slot at startup.
pub(crate) static FSR_CONFIG: ConfigSlot<FsrConfig> = ConfigSlot::new("fsr");

/// Anti-aliasing settings: the resolve in use, and FSR's and the shared inputs' switches. See
/// `docs/mod/rendering/fsr.md` and `docs/mod/rendering/taa.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FsrConfig {
    /// Which resolve runs at the anti-aliasing stage. A settings file that holds the older
    /// `enabled` switch instead is migrated on load (`config::format`).
    #[serde(default = "default_antialiasing")]
    pub antialiasing: AntiAliasing,
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
    /// or the mod's own temporal resolve runs, which reads the same offset.
    #[serde(default = "default_jitter")]
    pub jitter: bool,
    /// The mod's own temporal resolve's tuning, while [`antialiasing`](Self::antialiasing) is
    /// [`AntiAliasing::Taa`].
    #[serde(default)]
    pub taa: TaaConfig,
}
impl FsrConfig {
    pub const fn new() -> Self {
        Self {
            antialiasing: AntiAliasing::Fsr,
            sharpen: false,
            sharpness: 0.2,
            stereo_mv_correction: true,
            hybrid_velocity: true,
            jitter: true,
            taa: TaaConfig::new(),
        }
    }
}

fn default_antialiasing() -> AntiAliasing {
    FsrConfig::new().antialiasing
}

fn default_hybrid_velocity() -> bool {
    FsrConfig::new().hybrid_velocity
}

fn default_jitter() -> bool {
    FsrConfig::new().jitter
}

/// The tuning of the mod's own temporal resolve (`fsr::taa`). See `docs/mod/rendering/taa.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TaaConfig {
    /// The history's share of each resolved pixel at rest (0..1): the exponential blend's
    /// feedback, which sets how many frames the accumulation averages.
    pub history_weight: f32,
    /// The history's share once the pixel moves by 16 pixels a frame or more, interpolated with
    /// the motion between the two.
    pub motion_history_weight: f32,
    /// The half-size, in standard deviations, of the current neighbourhood's colour box the history
    /// is clipped to while the pixel moves.
    pub clip_gamma: f32,
    /// The same at rest, where a wider box keeps sub-pixel detail the current frame's
    /// neighbourhood misses at some jitter offsets.
    pub clip_gamma_rest: f32,
    /// The falloff of the kernel that reconstructs the current colour at the pixel's unjittered
    /// centre from its neighbourhood: a neighbour `d` pixels from that centre weighs
    /// `exp(-current_sharpness * d^2)`.
    pub current_sharpness: f32,
    /// The relative slack of the depth test that rejects a disoccluded history.
    pub depth_tolerance: f32,
}

impl TaaConfig {
    pub const fn new() -> Self {
        Self {
            history_weight: 0.9,
            motion_history_weight: 0.4,
            clip_gamma: 1.0,
            clip_gamma_rest: 1.5,
            current_sharpness: 4.0,
            depth_tolerance: 0.1,
        }
    }
}

impl Default for TaaConfig {
    fn default() -> Self {
        Self::new()
    }
}
