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
    /// Draw a line from each hand to every grip target within [`Self::radius_m`], coloured by how
    /// the hand stands to it.
    pub targets: bool,
    /// How far from a hand a grip target out of its reach still gets a line, metres.
    pub radius_m: f32,
    /// Mark each hand's wrist with the palms' state for that hand.
    pub wrist: bool,
    /// Flash the target of a grab that was refused.
    pub refusals: bool,
    /// Draw the boxes of the weapons lying in the world, the loose objects, and the vehicles near
    /// the hands, whether or not the reach finds them.
    pub candidates: bool,
    /// How far from a hand a candidate's box may be and still be drawn, metres.
    pub candidate_radius_m: f32,
    /// The most candidates drawn, the nearest first.
    pub max_candidates: u32,
}

impl GrabViewConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            rays: true,
            targets: true,
            radius_m: 2.0,
            wrist: true,
            refusals: true,
            candidates: true,
            candidate_radius_m: 3.0,
            max_candidates: 24,
        }
    }
}

impl Default for GrabViewConfig {
    fn default() -> Self {
        Self::new()
    }
}
