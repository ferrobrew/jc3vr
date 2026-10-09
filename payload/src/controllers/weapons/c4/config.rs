//! The settings of sticking a held charge onto a surface by hand (see
//! [`crate::controllers::weapons::c4::stick`]), held in
//! [`C4Config::stick`](crate::controllers::weapons::config::C4Config::stick).

use serde::{Deserialize, Serialize};

/// Pressing a held charge onto a surface sticks it there. Acts only under the controller scheme,
/// so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct C4StickConfig {
    /// Stick a held charge to the surface it is pressed onto.
    pub enabled: bool,
    /// How far from the held charge's centre a surface counts as touching it, metres: about half
    /// the charge's thickness.
    pub contact_m: f32,
    /// How far off the surface the charge is placed before it is pushed onto it, metres.
    pub standoff_m: f32,
    /// How fast the charge is pushed onto the surface, metres per second, so that the charge's own
    /// collision sticks it there.
    pub push_mps: f32,
    /// How long after a charge comes out of the pouch before it sticks, seconds, so that the body
    /// the pouch is on is not mistaken for a surface as the hand leaves it.
    pub after_prime_s: f32,
    /// How far from the held charge a surface is previewed, where the charge would stick if pressed
    /// on, metres.
    pub preview_m: f32,
}

impl C4StickConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            contact_m: 0.04,
            standoff_m: 0.03,
            push_mps: 1.5,
            after_prime_s: 0.3,
            preview_m: 0.3,
        }
    }
}

impl Default for C4StickConfig {
    fn default() -> Self {
        Self::new()
    }
}
