//! The physical hands' settings.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::controllers::weapons::config::PulseSpec;

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
    /// Strike characters with the palms: a hand that hits a character fast enough deals melee
    /// damage and a hit reaction.
    pub strikes: bool,
    /// While a hand's palm acts, the physical-melee gesture does not start the game's melee attack
    /// for that hand: the palm's own strike replaces it.
    pub replace_melee_gesture: bool,
    /// The slowest hand speed that strikes, metres per second.
    pub min_strike_speed_mps: f32,
    /// The hand speed of a full-strength strike, metres per second.
    pub full_strike_speed_mps: f32,
    /// How slow a hand must get before it strikes again, metres per second.
    pub rearm_speed_mps: f32,
    /// The shortest time between two strikes of one hand, seconds.
    pub strike_cooldown_s: f32,
    /// The mean curl of the four fingers from which a hand is a fist, `0..=1`. A fist punches; an
    /// open hand slaps.
    pub fist_curl: f32,
    /// A closed fist's strike.
    pub punch: StrikeTuning,
    /// An open hand's strike.
    pub slap: StrikeTuning,
    /// Push the dynamic objects a palm strikes (props, vehicles, ragdolls).
    pub object_impulses: bool,
    /// The slowest hand speed that pushes an object, metres per second.
    pub min_object_speed_mps: f32,
    /// The impulse a full-speed hit gives an object, as a share of the player's melee impulse.
    pub object_impulse: f32,
    /// Pulse a hand when its palm meets a surface or strikes.
    pub haptics: bool,
    /// The pulse of a palm meeting a surface at full strike speed; slower contacts are weaker.
    pub contact_pulse: PulseSpec,
    /// The pulse of a full-strength strike; weaker strikes are weaker.
    pub strike_pulse: PulseSpec,
    /// Draw the palm boxes and their contacts in the world-space overlay.
    pub show_boxes: bool,
    /// Picking up and holding loose objects with an empty hand's grip.
    pub pickup: PickupConfig,
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
            strikes: true,
            replace_melee_gesture: true,
            min_strike_speed_mps: 2.0,
            full_strike_speed_mps: 7.0,
            rearm_speed_mps: 1.0,
            strike_cooldown_s: 0.35,
            fist_curl: 0.6,
            punch: StrikeTuning {
                damage: 1.0,
                impulse: 1.0,
                stumble_strength: 0.5,
            },
            slap: StrikeTuning {
                damage: 0.2,
                impulse: 0.6,
                stumble_strength: 0.85,
            },
            object_impulses: true,
            min_object_speed_mps: 1.0,
            object_impulse: 1.0,
            haptics: true,
            contact_pulse: PulseSpec {
                amplitude: 0.35,
                duration_s: 0.025,
            },
            strike_pulse: PulseSpec {
                amplitude: 0.9,
                duration_s: 0.06,
            },
            show_boxes: false,
            pickup: PickupConfig::new(),
        }
    }
}

impl Default for PhysicalHandsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// How one kind of strike scales with its strength (`0..=1`, from the hand's speed).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrikeTuning {
    /// The damage of a full-strength strike, as a share of the player's melee damage.
    pub damage: f32,
    /// The impulse of a full-strength strike, as a share of the player's melee impulse.
    pub impulse: f32,
    /// The least strength at which the struck character stumbles, `0..=1`; above one, never.
    pub stumble_strength: f32,
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
    /// The heaviest object one hand lifts, kilograms.
    pub one_hand_mass_kg: f32,
    /// The heaviest object two hands lift together, kilograms.
    pub two_hand_mass_kg: f32,
    /// The held object's speed toward the hand per metre it is off, per second.
    pub stiffness_per_s: f32,
    /// The fastest the held object is driven, metres per second.
    pub max_speed_mps: f32,
    /// The held object's turn rate toward the hand's orientation per radian it is off, per second.
    pub angular_stiffness_per_s: f32,
    /// The fastest the held object is turned, radians per second.
    pub max_angular_speed_rps: f32,
    /// The time an object at the one-hand mass limit takes to come up to the driven velocity,
    /// seconds; lighter objects in proportion. Zero follows at once.
    pub heavy_response_s: f32,
    /// How far the held object can lag behind the hand before it is let go, metres.
    pub break_distance_m: f32,
    /// The released object's velocity as a share of the hand's.
    pub throw_scale: f32,
    /// The fastest throw, metres per second.
    pub max_throw_speed_mps: f32,
    /// Mark the object each hand would take, and the held ones, in the world-space overlay.
    pub show_targets: bool,
    /// The pulse of taking hold of an object or picking up a weapon.
    pub grab_pulse: PulseSpec,
    /// The pulse of letting go.
    pub release_pulse: PulseSpec,
}

impl PickupConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            reach_m: 0.1,
            weapons: true,
            one_hand_mass_kg: 25.0,
            two_hand_mass_kg: 60.0,
            stiffness_per_s: 18.0,
            max_speed_mps: 20.0,
            angular_stiffness_per_s: 14.0,
            max_angular_speed_rps: 20.0,
            heavy_response_s: 0.12,
            break_distance_m: 0.6,
            throw_scale: 1.0,
            max_throw_speed_mps: 25.0,
            show_targets: true,
            grab_pulse: PulseSpec {
                amplitude: 0.5,
                duration_s: 0.04,
            },
            release_pulse: PulseSpec {
                amplitude: 0.25,
                duration_s: 0.02,
            },
        }
    }
}

impl Default for PickupConfig {
    fn default() -> Self {
        Self::new()
    }
}
