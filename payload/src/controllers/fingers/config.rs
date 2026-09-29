//! The finger-curl settings, held in
//! [`ControllersConfig::fingers`](crate::controllers::config::ControllersConfig::fingers).

use serde::{Deserialize, Serialize};

/// Finger curls on the rendered hands. They act only on a hand the controllers drive, so they
/// default on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FingersConfig {
    /// Curl the fingers from hand tracking when available, and otherwise from the inputs the
    /// fingers rest on (the trigger, the grip, and the touch sensors).
    pub enabled: bool,
    /// Where the curls come from.
    pub source: FingerSource,
    /// Drive every finger of a hand that holds a weapon. Off, such a hand drives only its trigger
    /// finger, and the rest keep the game's grip, which wraps the weapon.
    pub drive_all_when_holding: bool,
    /// The bend applied at each finger's open and closed curl, indexed by
    /// [`Finger::index`](crate::controllers::fingers::Finger::index).
    pub ranges: [CurlRange; 5],
    /// The full bend of a finger's proximal, middle, and distal joints, degrees: the bend at a
    /// range value of one.
    pub finger_bend_deg: [f32; 3],
    /// The full bend of the thumb's base, middle, and distal joints, degrees.
    pub thumb_bend_deg: [f32; 3],
    /// The share of the tracked finger splay applied (hand tracking only).
    pub splay_scale: f32,
    /// The largest splay applied either way, degrees.
    pub max_splay_deg: f32,
    /// The time constant the curls follow their source with, seconds. The estimate jumps when a
    /// touch sensor changes, and the smoothing turns the jump into a quick motion.
    pub smoothing_s: f32,
}

impl FingersConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            source: FingerSource::Auto,
            drive_all_when_holding: false,
            ranges: [
                // The thumb.
                CurlRange {
                    open: -0.2,
                    closed: 0.9,
                },
                // The index finger.
                CurlRange {
                    open: -0.15,
                    closed: 0.85,
                },
                // The middle finger.
                CurlRange {
                    open: -0.15,
                    closed: 0.9,
                },
                // The ring finger.
                CurlRange {
                    open: -0.15,
                    closed: 0.9,
                },
                // The little finger.
                CurlRange {
                    open: -0.15,
                    closed: 0.95,
                },
            ],
            finger_bend_deg: [80.0, 95.0, 65.0],
            thumb_bend_deg: [20.0, 45.0, 60.0],
            splay_scale: 1.0,
            max_splay_deg: 20.0,
            smoothing_s: 0.04,
        }
    }
}

impl Default for FingersConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Where the finger curls come from.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FingerSource {
    /// The tracked joints for a hand the runtime reports them for, and the estimate otherwise.
    #[default]
    Auto,
    /// The tracked joints only; a hand without them keeps the animated fingers.
    Joints,
    /// The estimate from the inputs only.
    Estimate,
}

impl FingerSource {
    pub const ALL: [FingerSource; 3] = [
        FingerSource::Auto,
        FingerSource::Joints,
        FingerSource::Estimate,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            FingerSource::Auto => "auto",
            FingerSource::Joints => "joints",
            FingerSource::Estimate => "estimate",
        }
    }
}

/// The bend a finger takes at its open (curl 0) and closed (curl 1) ends, as a share of its full
/// joint bends, added to the animated pose. A negative value straightens a finger the animation
/// holds bent.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurlRange {
    pub open: f32,
    pub closed: f32,
}

impl CurlRange {
    /// The bend share at `curl`.
    pub fn at(self, curl: f32) -> f32 {
        self.open + (self.closed - self.open) * curl.clamp(0.0, 1.0)
    }
}
