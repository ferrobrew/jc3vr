//! The grab view's settings.

use serde::{Deserialize, Serialize};

/// The grab view: what the grip and the palms' reach saw drawn in the world-space overlay, around
/// each hand. A diagnostic, on by default.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrabViewConfig {
    /// Draw the view.
    pub enabled: bool,
    /// Draw each palm's reach rays as the last tick cast them, coloured by what they hit.
    pub rays: bool,
    /// Draw a line from each hand to every grip target near it, coloured by how the hand stands to
    /// it.
    pub targets: bool,
    /// Mark each hand's wrist with the palms' state for that hand. Off by default: a coloured disc
    /// on the wrist reads to a player as part of the wrist detonator, and the state it shows is in
    /// the readout and the grip press log.
    pub wrist: bool,
    /// Flash the target of a grab that was refused.
    pub refusals: bool,
    /// Draw the boxes of the weapons lying in the world, the loose objects, and the vehicles near
    /// the hands, whether or not the reach finds them.
    pub candidates: bool,
}

impl GrabViewConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            rays: true,
            targets: true,
            wrist: false,
            refusals: true,
            candidates: true,
        }
    }
}

impl Default for GrabViewConfig {
    fn default() -> Self {
        Self::new()
    }
}
