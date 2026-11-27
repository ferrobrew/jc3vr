//! The physical hands' settings.

use jc3vr_core::config::ConfigSlot;
use serde::{Deserialize, Serialize};

use crate::controllers::physical_hands::{
    collider_view::ColliderViewConfig, grab_view::GrabViewConfig, physics::HandBodiesConfig,
};

/// The physical hands' settings, at `controllers.physical_hands` in the settings file; the payload
/// binds the slot at startup.
pub static PHYSICAL_HANDS_CONFIG: ConfigSlot<PhysicalHandsConfig> =
    ConfigSlot::new("controllers.physical_hands");

/// The physical hands (see [`crate::controllers::physical_hands`]). They act only under the
/// controller scheme, on foot on the ground or falling, and for a hand that holds nothing. Their
/// tuning is the shipped
/// [`PHYSICAL_HANDS`](crate::controllers::physical_hands::tuning::PHYSICAL_HANDS).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicalHandsConfig {
    /// The hand bodies' strength.
    pub bodies: HandBodiesConfig,
    /// The collider view: the physics world's bodies near the player drawn in the overlay, a
    /// diagnostic.
    pub collider_view: ColliderViewConfig,
    /// The grab view: what the grip and the reach saw drawn in the overlay around the hands, a
    /// diagnostic.
    pub grab_view: GrabViewConfig,
}

impl PhysicalHandsConfig {
    pub const fn new() -> Self {
        Self {
            bodies: HandBodiesConfig::new(),
            collider_view: ColliderViewConfig::new(),
            grab_view: GrabViewConfig::new(),
        }
    }
}

impl Default for PhysicalHandsConfig {
    fn default() -> Self {
        Self::new()
    }
}
