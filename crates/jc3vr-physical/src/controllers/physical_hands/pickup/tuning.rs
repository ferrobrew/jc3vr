//! The pickups' tuning, read from the shipped `tuning/pickup.toml` ([`PICKUP`]): the reach, the
//! weight limits, the drive toward the hand, the tearing away, the carry along with the
//! character, and the floor scan. The hand's strength on what it holds is the hand bodies'
//! ([`HandStrength`]).

use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::physical_hands::physics::tuning::PHYSICS;

jc3vr_core::tuning_file! {
    /// The pickups' tuning file.
    pub static PICKUP: PickupTuning = "pickup.toml";
}

/// The pickups' tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickupTuning {
    /// How far from the palm an object can be taken, metres of the player's hand.
    pub reach_m: f32,
    /// The heaviest object a hand takes hold of at all, kilograms. A hand holds what it cannot lift
    /// and drags or pushes it within its strength.
    pub(crate) max_mass_kg: f32,
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
    /// The radius around the player's body a held object's centre is kept out of, metres; zero
    /// keeps nothing out.
    pub(crate) keep_out_radius_m: f32,
    /// How far the grab point can lag behind the hand before a hold that is stuck is let go,
    /// metres ([`grasp::Strain`](crate::controllers::physical_hands::grasp::Strain)). A heavy
    /// object that drags behind the hand while it still follows stays held past it.
    pub(crate) break_distance_m: f32,
    /// How far the grab point can lag behind the hand before the hold is let go however the
    /// object moves, metres: further than the hand could reach for it.
    pub(crate) tear_distance_m: f32,
    /// The slowest the grab point can come toward the hand while it is past
    /// [`break_distance_m`](Self::break_distance_m) and still count as following it, metres per
    /// second of real time.
    pub(crate) stuck_speed_mps: f32,
    /// How long a grab point past [`break_distance_m`](Self::break_distance_m) can fail to follow
    /// the hand before the hold is let go, seconds of real time.
    pub(crate) stuck_s: f32,
    /// The share of the character's own change of velocity a carried loose object the hands can
    /// lift takes on each tick, `0..=1`. The hands ride on the character, so a jump, a landing, or
    /// a launch moves what they carry with them rather than through the hands' strength, which
    /// covers only the hands' motion relative to the body. One carries the object along whatever
    /// its weight within the hands' lift; its weight still shows as the drive's sag.
    pub(crate) carrier_motion_share: f32,
    /// The heaviest articulated object a hand takes hold of, kilograms: a body joined to the world
    /// or another body by a constraint, such as a hinged gate, a hatch, or a lever, is taken
    /// whatever its weight up to this, since the constraint carries it.
    pub(crate) max_articulated_mass_kg: f32,
    /// How fast a held articulated object's motion within its assembly dies down, per second: the
    /// rate a held ragdoll's bones are damped at (the characters' tuning's
    /// `characters.held_ragdoll_damping_per_s`).
    pub(crate) assembly_damping_per_s: f32,
    /// How long a hand that undid a latch keeps its grip on the door while the door's body is not
    /// yet free to move, seconds.
    pub(crate) unlatch_settle_s: f32,
    pub transport: TransportTuning,
    pub reach: ReachLimits,
    pub(crate) floor_weapons: FloorWeaponsTuning,
}

/// The character's motion a carried object rides along with
/// ([`Transport`](crate::controllers::physical_hands::pickup::transport::Transport)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportTuning {
    /// The fastest turn of the character a carried object follows, radians per second.
    pub max_angular_speed_rps: f32,
    /// The fastest motion of the character a carried object follows, metres per second.
    pub(crate) max_linear_speed_mps: f32,
}

/// What one hand's reach keeps per tick.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReachLimits {
    /// The most notes one hand's reach keeps for the grip press's log.
    pub(crate) max_notes: usize,
    /// The most vehicles one hand's rays hand the door scan.
    pub(crate) max_hit_vehicles: usize,
}

/// The scan for dropped weapons and characters near the hands
/// ([`floor_weapons`](crate::controllers::physical_hands::pickup::floor_weapons)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FloorWeaponsTuning {
    /// How far past the hands' reach and spread a body is still remembered, metres.
    pub(crate) discovery_margin_m: f32,
    /// The most dynamic bodies the scan walks per tick.
    pub(crate) walk_budget: u32,
    /// The most bodies the scan remembers.
    pub max_bodies: usize,
}

/// One hand's strength on what it holds: the hand bodies' strongest pull and twist at the hands'
/// strength setting ([`HandBodyTuning::force_n`]).
///
/// [`HandBodyTuning::force_n`]: crate::controllers::physical_hands::physics::tuning::HandBodyTuning::force_n
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HandStrength {
    /// The strongest pull of one hand on what it holds, newtons. One hand lifts what weighs less;
    /// two hands pull twice as hard.
    pub(crate) force_n: f32,
    /// The strongest twist of one hand, newton metres: a long object held at one end droops when
    /// its weight's lever is longer.
    pub(crate) torque_nm: f32,
}

impl HandStrength {
    /// The strength of a hand at the strength setting `strength`.
    pub fn at(strength: f32) -> Self {
        let bodies = &PHYSICS.get().hand_bodies;
        Self {
            force_n: bodies.force_n(strength),
            torque_nm: bodies.torque_nm(strength),
        }
    }

    /// The strength of a hand at a strength of one.
    pub fn full() -> Self {
        let bodies = &PHYSICS.get().hand_bodies;
        Self {
            force_n: bodies.max_force_n,
            torque_nm: bodies.max_torque_nm,
        }
    }
}

impl Tuning for PickupTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("reach_m", self.reach_m, 2.0)?;
        positive("max_mass_kg", self.max_mass_kg, 1.0e6)?;
        nonnegative("stiffness_per_s", self.stiffness_per_s, 1000.0)?;
        nonnegative("max_speed_mps", self.max_speed_mps, 1000.0)?;
        nonnegative(
            "angular_stiffness_per_s",
            self.angular_stiffness_per_s,
            1000.0,
        )?;
        nonnegative("max_angular_speed_rps", self.max_angular_speed_rps, 1000.0)?;
        range(
            "two_hand_angular_share",
            self.two_hand_angular_share,
            0.0,
            1.0,
        )?;
        nonnegative("keep_out_radius_m", self.keep_out_radius_m, 5.0)?;
        positive("tear_distance_m", self.tear_distance_m, 10.0)?;
        range(
            "break_distance_m",
            self.break_distance_m,
            0.0,
            self.tear_distance_m,
        )?;
        nonnegative("stuck_speed_mps", self.stuck_speed_mps, 100.0)?;
        nonnegative("stuck_s", self.stuck_s, 60.0)?;
        range("carrier_motion_share", self.carrier_motion_share, 0.0, 1.0)?;
        positive(
            "max_articulated_mass_kg",
            self.max_articulated_mass_kg,
            1.0e7,
        )?;
        nonnegative(
            "assembly_damping_per_s",
            self.assembly_damping_per_s,
            1000.0,
        )?;
        nonnegative("unlatch_settle_s", self.unlatch_settle_s, 10.0)?;

        nonnegative(
            "transport.max_angular_speed_rps",
            self.transport.max_angular_speed_rps,
            1000.0,
        )?;
        nonnegative(
            "transport.max_linear_speed_mps",
            self.transport.max_linear_speed_mps,
            10_000.0,
        )?;

        count("reach.max_notes", self.reach.max_notes, 1024)?;
        count("reach.max_hit_vehicles", self.reach.max_hit_vehicles, 1024)?;

        let floor = &self.floor_weapons;
        nonnegative(
            "floor_weapons.discovery_margin_m",
            floor.discovery_margin_m,
            100.0,
        )?;
        count(
            "floor_weapons.walk_budget",
            floor.walk_budget as usize,
            1 << 20,
        )?;
        count("floor_weapons.max_bodies", floor.max_bodies, 1 << 16)
    }
}

/// Check that the count `value` named `name` is in `1..=max`.
fn count(name: &str, value: usize, max: usize) -> Result<(), Invalid> {
    if value == 0 || value > max {
        return Err(Invalid(format!(
            "{name} must be in [1, {max}], got {value}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(PICKUP.get().validate(), Ok(()));
    }
}
