//! The physical hands' settings.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::controllers::physical_hands::{
    characters::CharactersConfig, climb::ClimbConfig, doors::DoorsConfig,
    physics::HandBodiesConfig, stunt::StuntConfig, vault::VaultConfig,
};

/// The physical hands (see [`crate::controllers::physical_hands`]). They act only under the
/// controller scheme, on foot on the ground, and for a hand that holds nothing, so they default on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicalHandsConfig {
    /// Master switch: give each empty hand a palm that collides with the world.
    pub enabled: bool,
    /// Half the palm box's size, metres of the player's hand: across the palm, through it, and
    /// along it.
    pub palm_half_extents_m: Vec3,
    /// The palm box's centre in the wrist frame, metres (-Z toward the fingers, +Y out of the back
    /// of the hand).
    pub palm_offset_m: Vec3,
    /// Hold the rendered hand at the surface it presses into, instead of letting it pass through.
    pub clamp_hand: bool,
    /// Push the body away from a surface a palm presses into.
    pub push: bool,
    /// The press depth left alone before the body is pushed, metres, so that resting a hand on a
    /// wall does not drift the body.
    pub push_deadband_m: f32,
    /// The push speed per metre of press depth past the deadband, per second.
    pub push_stiffness_per_s: f32,
    /// The fastest push, metres per second.
    pub max_push_speed_mps: f32,
    /// The least horizontal share of a surface's normal for the press to push the body, `0..=1`:
    /// pressing down on a table or the floor does not push.
    pub min_push_horizontal: f32,
    /// The hand speed of a full-strength hit, metres per second: a palm meeting a surface this
    /// fast pulses its hand at full strength, and knocks an object it meets with its full share of
    /// the player's melee impulse.
    pub full_hit_speed_mps: f32,
    /// Shove the loose objects a palm presses into, every tick it presses, however slowly.
    pub shove: ShoveConfig,
    /// Knock the dynamic objects a palm hits fast (props, vehicles, ragdolls) with an extra impulse
    /// on the first tick of contact, on top of the shove.
    pub object_impulses: bool,
    /// The slowest hand speed that pushes an object, metres per second.
    pub min_object_speed_mps: f32,
    /// The impulse a full-speed hit gives an object, as a share of the player's melee impulse.
    pub object_impulse: f32,
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
}

impl PhysicalHandsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            palm_half_extents_m: Vec3::new(0.045, 0.02, 0.05),
            palm_offset_m: Vec3::new(0.0, -0.01, -0.06),
            clamp_hand: true,
            push: true,
            push_deadband_m: 0.02,
            push_stiffness_per_s: 10.0,
            max_push_speed_mps: 2.5,
            min_push_horizontal: 0.3,
            full_hit_speed_mps: 7.0,
            shove: ShoveConfig::new(),
            object_impulses: true,
            min_object_speed_mps: 1.0,
            object_impulse: 1.0,
            show_boxes: false,
            pickup: PickupConfig::new(),
            bodies: HandBodiesConfig::new(),
            climb: ClimbConfig::new(),
            doors: DoorsConfig::new(),
            characters: CharactersConfig::new(),
            vault: VaultConfig::new(),
            stunt: StuntConfig::new(),
        }
    }
}

impl Default for PhysicalHandsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The palms' soft contact with loose objects (see
/// [`crate::controllers::physical_hands::shove`]). A palm pressed into a dynamic body pushes the
/// point it touches along with the hand, within the hand's strength; what the body does not give
/// pushes the player away, as a wall does.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShoveConfig {
    pub enabled: bool,
    /// The speed into the body per metre of press past the deadband, per second.
    pub stiffness_per_s: f32,
    /// The press depth left alone, metres, so that a hand resting on a crate does not creep it.
    pub deadband_m: f32,
    /// The fastest the contact pushes the touched point, metres per second.
    pub max_speed_mps: f32,
    /// The strongest push of one palm, newtons.
    pub max_force_n: f32,
    /// The friction coefficient between the palm and a surface it slides along.
    pub friction: f32,
    /// How long the measure of how much a body gives is smoothed over, seconds.
    pub give_smoothing_s: f32,
}

impl ShoveConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            stiffness_per_s: 12.0,
            deadband_m: 0.005,
            max_speed_mps: 4.0,
            max_force_n: 600.0,
            friction: 0.6,
            give_smoothing_s: 0.08,
        }
    }
}

impl Default for ShoveConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Picking up with the physical hands (see [`crate::controllers::physical_hands::pickup`]). A grip
/// press with an empty hand near a loose object takes hold of it; near a weapon lying in the world
/// it picks the weapon up the game's own way.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PickupConfig {
    pub enabled: bool,
    /// How far from the palm an object can be taken, metres of the player's hand.
    pub reach_m: f32,
    /// Collect weapons lying in the world by grabbing them.
    pub weapons: bool,
    /// The heaviest object a hand takes hold of at all, kilograms. A hand holds what it cannot lift
    /// and drags or pushes it within its strength.
    pub max_mass_kg: f32,
    /// The strongest pull of one hand on what it holds, newtons. One hand lifts what weighs less;
    /// two hands pull twice as hard.
    pub hand_force_n: f32,
    /// The strongest twist of one hand, newton metres: a long object held at one end droops when
    /// its weight's lever is longer.
    pub hand_torque_nm: f32,
    /// The grab point's speed toward the hand per metre it is off, per second.
    pub stiffness_per_s: f32,
    /// The fastest the grab point is driven, metres per second.
    pub max_speed_mps: f32,
    /// The held object's turn rate toward the hand's orientation per radian it is off, per second.
    pub angular_stiffness_per_s: f32,
    /// The fastest the held object is turned, radians per second.
    pub max_angular_speed_rps: f32,
    /// The share of each hand's twist while both hands hold one object, `0..=1`: the two grab
    /// points, not the two wrists, set the object's line.
    pub two_hand_angular_share: f32,
    /// Gravity's acceleration the hold supports a body against, metres per second squared.
    pub gravity_mps2: f32,
    /// Push the player toward the grab point of what a hand holds but cannot move, as a wall
    /// pushes the player off it.
    pub resist: bool,
    /// The radius around the player's body a held object's centre is kept out of, metres; zero
    /// keeps nothing out.
    pub keep_out_radius_m: f32,
    /// How far the grab point can lag behind the hand before it is let go, metres.
    pub break_distance_m: f32,
    /// The released object's velocity as a share of the hand's.
    pub throw_scale: f32,
    /// The fastest throw, metres per second.
    pub max_throw_speed_mps: f32,
    /// Take hold of articulated objects: bodies joined to the world or another body by a
    /// constraint, such as hinged gates, hatches, and levers, whatever their weight up to
    /// [`max_articulated_mass_kg`](Self::max_articulated_mass_kg), since the constraint carries
    /// them.
    pub articulated: bool,
    /// The heaviest articulated object a hand takes hold of, kilograms.
    pub max_articulated_mass_kg: f32,
    /// Mark the object each hand would take, and the held ones, in the world-space overlay.
    pub show_targets: bool,
}

impl PickupConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            reach_m: 0.1,
            weapons: true,
            max_mass_kg: 1500.0,
            hand_force_n: 350.0,
            hand_torque_nm: 30.0,
            stiffness_per_s: 18.0,
            max_speed_mps: 20.0,
            angular_stiffness_per_s: 14.0,
            max_angular_speed_rps: 20.0,
            two_hand_angular_share: 0.25,
            gravity_mps2: 9.81,
            resist: true,
            keep_out_radius_m: 0.3,
            break_distance_m: 0.6,
            throw_scale: 1.0,
            max_throw_speed_mps: 25.0,
            articulated: true,
            max_articulated_mass_kg: 20_000.0,
            show_targets: false,
        }
    }
}

impl Default for PickupConfig {
    fn default() -> Self {
        Self::new()
    }
}
