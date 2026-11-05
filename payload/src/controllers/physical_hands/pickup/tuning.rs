//! The pickups' fixed tuning: the reach, the weight limits, the hand's strength on what it holds,
//! the drive toward the hand, and the throw.

use crate::{controllers::physical_hands::config::PickupConfig, grapple::rope::GRAVITY_MPS2};

/// The pickups' tuning, fixed at [`PICKUP`] apart from what [`PickupConfig`] sets from the debug UI
/// ([`PickupTuning::configured`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PickupTuning {
    /// How far from the palm an object can be taken, metres of the player's hand.
    pub reach_m: f32,
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
    /// The radius around the player's body a held object's centre is kept out of, metres; zero
    /// keeps nothing out.
    pub keep_out_radius_m: f32,
    /// How far the grab point can lag behind the hand before it is let go, metres.
    pub break_distance_m: f32,
    /// The share of the character's own change of velocity a carried loose object the hands can
    /// lift takes on each tick, `0..=1`. The hands ride on the character, so a jump, a landing, or
    /// a launch moves what they carry with them rather than through the hands' strength, which
    /// covers only the hands' motion relative to the body. One carries the object along whatever
    /// its weight within the hands' lift; its weight still shows as the drive's sag.
    pub carrier_motion_share: f32,
    /// The released object's velocity as a share of the hand's.
    pub throw_scale: f32,
    /// The fastest throw, metres per second.
    pub max_throw_speed_mps: f32,
    /// The heaviest articulated object a hand takes hold of, kilograms: a body joined to the world
    /// or another body by a constraint, such as a hinged gate, a hatch, or a lever, is taken
    /// whatever its weight up to this, since the constraint carries it.
    pub max_articulated_mass_kg: f32,
}

impl PickupTuning {
    /// The tuning with the debug UI's settings from `config` applied.
    pub const fn configured(self, config: &PickupConfig) -> Self {
        Self {
            throw_scale: config.throw_scale,
            carrier_motion_share: config.carrier_motion_share,
            ..self
        }
    }
}

/// The pickups' tuning.
pub const PICKUP: PickupTuning = PickupTuning {
    reach_m: 0.18,
    max_mass_kg: 1500.0,
    hand_force_n: 350.0,
    hand_torque_nm: 30.0,
    stiffness_per_s: 18.0,
    max_speed_mps: 20.0,
    angular_stiffness_per_s: 14.0,
    max_angular_speed_rps: 20.0,
    two_hand_angular_share: 0.25,
    gravity_mps2: GRAVITY_MPS2,
    keep_out_radius_m: 0.3,
    break_distance_m: 0.6,
    carrier_motion_share: 1.0,
    throw_scale: 1.0,
    max_throw_speed_mps: 25.0,
    max_articulated_mass_kg: 20_000.0,
};
