//! The finger-curl settings, held in
//! [`ControllersConfig::fingers`](crate::controllers::config::ControllersConfig::fingers).

use serde::{Deserialize, Serialize};

/// Finger curls on the rendered hands. They act only on a hand the controllers drive, so they
/// default on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FingersConfig {
    /// Curl the fingers from hand tracking when available, and otherwise from the inputs the
    /// fingers rest on (the trigger, the grip, and the touch sensors), and wrap them round what the
    /// hand holds.
    pub enabled: bool,
    /// Where the curls come from.
    pub source: FingerSource,
}

impl FingersConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            source: FingerSource::Auto,
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
