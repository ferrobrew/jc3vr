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
