//! The physical hands' settings.

use serde::{Deserialize, Serialize};

use crate::controllers::physical_hands::{
    characters::CharactersConfig, climb::ClimbConfig, collider_view::ColliderViewConfig,
    doors::DoorsConfig, grab_view::GrabViewConfig, physics::HandBodiesConfig,
    sounds::ImpactSoundsConfig, stunt::StuntConfig, vault::VaultConfig,
};

/// The physical hands (see [`crate::controllers::physical_hands`]). They act only under the
/// controller scheme, on foot on the ground, and for a hand that holds nothing, so they default on.
/// Their fixed tuning is [`PALMS`](crate::controllers::physical_hands::tuning::PALMS).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicalHandsConfig {
    /// Master switch: give each empty hand a palm that collides with the world.
    pub enabled: bool,
    /// Push the body away from a surface a palm or a hand body presses into, and toward what a
    /// hand holds but cannot move.
    pub push: bool,
    /// Draw the palm boxes and their contacts in the world-space overlay.
    pub show_boxes: bool,
    /// Picking up and holding loose objects with an empty hand's grip.
    pub pickup: PickupConfig,
    /// The hand bodies, which replace the palm's probes for the hands they drive.
    pub bodies: HandBodiesConfig,
    /// Climbing by gripping the static world with a hand its body drives.
    pub climb: ClimbConfig,
    /// Opening vehicle doors by hand.
    pub doors: DoorsConfig,
    /// Taking hold of characters.
    pub characters: CharactersConfig,
    /// Pushing the body up with the hands pressed down.
    pub vault: VaultConfig,
    /// Stunt positions on vehicles, entered and let go of with the grip.
    pub stunt: StuntConfig,
    /// The game's melee impact sound where a hand meets something hard enough.
    pub sounds: ImpactSoundsConfig,
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
            enabled: true,
            push: true,
            show_boxes: false,
            pickup: PickupConfig::new(),
            bodies: HandBodiesConfig::new(),
            climb: ClimbConfig::new(),
            doors: DoorsConfig::new(),
            characters: CharactersConfig::new(),
            vault: VaultConfig::new(),
            stunt: StuntConfig::new(),
            sounds: ImpactSoundsConfig::new(),
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

/// Picking up with the physical hands (see [`crate::controllers::physical_hands::pickup`]). A grip
/// press with an empty hand near a loose object takes hold of it; near a weapon lying in the world
/// it picks the weapon up the game's own way. The rest of the hold's tuning is
/// [`PICKUP`](crate::controllers::physical_hands::pickup::PICKUP).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PickupConfig {
    pub enabled: bool,
    /// The released object's velocity as a share of the hand's.
    pub throw_scale: f32,
}

impl PickupConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            throw_scale: 1.0,
        }
    }
}

impl Default for PickupConfig {
    fn default() -> Self {
        Self::new()
    }
}
