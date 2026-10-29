//! The palms' fixed tuning: the palm box, the push off surfaces, and the knock of a fast hit.

use glam::Vec3;

/// The palms' tuning, fixed at [`PALMS`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PalmTuning {
    /// Half the palm box's size, metres of the player's hand: across the palm, through it, and
    /// along it.
    pub palm_half_extents_m: Vec3,
    /// The palm box's centre in the wrist frame, metres (-Z toward the fingers, +Y out of the back
    /// of the hand).
    pub palm_offset_m: Vec3,
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
    /// The slowest hand speed that knocks a dynamic object (a prop, a vehicle, a ragdoll) with an
    /// extra impulse on the first tick of contact, on top of the shove, metres per second.
    pub min_object_speed_mps: f32,
    /// The impulse a full-speed hit gives an object, as a share of the player's melee impulse.
    pub object_impulse: f32,
}

/// The palms' tuning.
pub const PALMS: PalmTuning = PalmTuning {
    palm_half_extents_m: Vec3::new(0.045, 0.02, 0.05),
    palm_offset_m: Vec3::new(0.0, -0.01, -0.06),
    push_deadband_m: 0.02,
    push_stiffness_per_s: 10.0,
    max_push_speed_mps: 2.5,
    min_push_horizontal: 0.3,
    full_hit_speed_mps: 7.0,
    min_object_speed_mps: 1.0,
    object_impulse: 1.0,
};
