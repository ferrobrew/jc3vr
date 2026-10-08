//! The character grabs' settings.

use serde::{Deserialize, Serialize};

/// Taking hold of characters with an empty hand (see
/// [`crate::controllers::physical_hands::characters`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharactersConfig {
    /// Take hold of characters on foot: hold on, shove, yank them off their feet, and drag and
    /// throw them by their ragdoll.
    pub grab: bool,
    /// How hard a yank throws a character the player is hostile to, as a share of the knockdown
    /// blow ([`HandHitsConfig::knockdown_impulse_ns`]); one and above knocks the character down.
    ///
    /// [`HandHitsConfig::knockdown_impulse_ns`]: crate::controllers::physical_hands::physics::HandHitsConfig::knockdown_impulse_ns
    pub yank_strength: f32,
    /// How long after a yank the hand looks for the fallen character's ragdoll to hold, seconds.
    pub regrip_s: f32,
    /// The slowest hand that shoves a character it lets go of, metres per second.
    pub shove_min_speed_mps: f32,
    /// Pull a character out of a vehicle seat through an open door: grip them, then yank.
    pub hijack: bool,
    /// How far open a door beside the hand must stand for a hand to reach in, degrees.
    pub hijack_min_open_deg: f32,
    /// How far from the palm an open door may be for the hand to reach in through it, metres of
    /// the player's hand.
    pub hijack_door_reach_m: f32,
    /// How far a hand gripping a character must pull away from its grab point to yank, metres of
    /// the player's hand.
    pub yank_m: f32,
}

impl CharactersConfig {
    pub const fn new() -> Self {
        Self {
            grab: true,
            yank_strength: 1.3,
            regrip_s: 1.0,
            shove_min_speed_mps: 1.5,
            hijack: true,
            hijack_min_open_deg: 25.0,
            hijack_door_reach_m: 1.2,
            yank_m: 0.2,
        }
    }
}

impl Default for CharactersConfig {
    fn default() -> Self {
        Self::new()
    }
}
