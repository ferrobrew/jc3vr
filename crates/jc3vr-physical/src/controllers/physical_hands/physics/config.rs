//! The hand bodies' setting.

use serde::{Deserialize, Serialize};

/// The hand bodies (see [`crate::controllers::physical_hands::physics`]): each empty hand is a body
/// in the game's physics world, driven toward the controller within the hand's strength, and a
/// character struck by a hand reacts to the blow. Their tuning is the tuning file's `hand_bodies`
/// ([`PHYSICS`](crate::controllers::physical_hands::physics::PHYSICS)).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HandBodiesConfig {
    /// The hands' strength, a scale on their force and twist: one is the default "Rico strength".
    pub strength: f32,
}

impl HandBodiesConfig {
    pub const fn new() -> Self {
        Self { strength: 1.0 }
    }
}

impl Default for HandBodiesConfig {
    fn default() -> Self {
        Self::new()
    }
}
