//! Character hook configuration: headset-driven upper-body IK.

use serde::{Deserialize, Serialize};

/// Headset-driven upper-body IK: drive the player's spine and head toward the headpose target by
/// feeding the engine's own HumanIK `MAIN` pass an effector target for the head bone, so the body
/// leans, ducks, and turns to follow where the player looks. Queued pre-solve in
/// [`crate::hooks::character`] (see `docs/engine/character/humanik.md`); the `UpdatePropEffects` head-bone override
/// still sets the exact head orientation on top of the HIK-bent spine. The weights are fixed in
/// [`crate::hooks::character::body_ik::tuning`].
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct BodyIkConfig {
    /// Master switch: queue the head effector target each frame for the local player.
    pub enabled: bool,
}
impl BodyIkConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}
