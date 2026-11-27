//! The physical hands' tuning, read from the shipped `tuning/hands.toml` ([`PHYSICAL_HANDS`]): the palm box,
//! the push off surfaces and the knock of a fast hit, how long a tick's results stay current, the
//! soft contact with loose bodies, the impact sounds, the hold's solver, the strain's acquisition,
//! the pressure haptics, the carried bodies' collision, the snatch's side, the articulated
//! assemblies, and the held marker.

use glam::Vec3;
use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range, vec3},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::physical_hands::{shove::ShoveTuning, sounds::SoundsTuning};

jc3vr_core::tuning_file! {
    /// The physical hands' tuning file.
    pub static PHYSICAL_HANDS: HandsTuning = "hands.toml";
}

/// The physical hands' tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandsTuning {
    pub palms: PalmTuning,
    pub state: StateTuning,
    pub shove: ShoveTuning,
    pub sounds: SoundsTuning,
    pub hold: HoldSolverTuning,
    pub grasp: GraspTuning,
    pub haptics: PressureHapticsTuning,
    pub(crate) carry_collision: CarryCollisionTuning,
    pub remote: RemoteTuning,
    pub articulated: ArticulatedTuning,
    pub targets: TargetsTuning,
}

impl Tuning for HandsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.palms.validate()?;
        self.state.validate()?;
        self.shove.validate()?;
        self.sounds.validate()?;
        self.hold.validate()?;
        self.grasp.validate()?;
        self.haptics.validate()?;
        self.carry_collision.validate()?;
        self.remote.validate()?;
        positive(
            "articulated.max_assembly_bodies",
            self.articulated.max_assembly_bodies as f32,
            4096.0,
        )?;
        positive("targets.held_mark_m", self.targets.held_mark_m, 1.0)
    }
}

/// The palms' tuning: the palm box, the push off surfaces, and the knock of a fast hit. The palm
/// box is centred on the hand's palm centre
/// ([`palm_centre_m()`](jc3vr_body::controllers::hands::pose::palm_centre_m)) in the wrist frame.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PalmTuning {
    /// Half the palm box's size, metres of the player's hand: across the palm, through it, and
    /// along it.
    pub(crate) palm_half_extents_m: Vec3,
    /// The press depth left alone before the body is pushed, metres, so that resting a hand on a
    /// wall does not drift the body.
    pub(crate) push_deadband_m: f32,
    /// The push speed per metre of press depth past the deadband, per second.
    pub(crate) push_stiffness_per_s: f32,
    /// The fastest push, metres per second.
    pub(crate) max_push_speed_mps: f32,
    /// The least horizontal share of a surface's normal for the press to push the body, `0..=1`:
    /// pressing down on a table or the floor does not push.
    pub(crate) min_push_horizontal: f32,
    /// The hand speed of a full-strength hit, metres per second: a palm meeting a surface this
    /// fast pulses its hand at full strength, and knocks an object it meets with its full share of
    /// the player's melee impulse.
    pub(crate) full_hit_speed_mps: f32,
    /// The slowest hand speed that knocks a dynamic object (a prop, a vehicle, a ragdoll) with an
    /// extra impulse on the first tick of contact, on top of the shove, metres per second.
    pub(crate) min_object_speed_mps: f32,
    /// The impulse a full-speed hit gives an object, as a share of the player's melee impulse.
    pub(crate) object_impulse: f32,
    /// The least strength a knock is given, `0..=1`, so that a hit just past
    /// [`min_object_speed_mps`](Self::min_object_speed_mps) still moves the object.
    pub(crate) min_object_strength: f32,
    /// The lag a carried loose object leaves alone before it pushes the player, metres: past what a
    /// carry near the hands' strength trails by, and well short of the break distance
    /// ([`resist_deadband_m`](crate::controllers::physical_hands::hold::resist_deadband_m)).
    pub(crate) carry_resist_deadband_m: f32,
    /// The load from which a hold that cannot move its body starts pushing the player, `0..1`
    /// ([`resist_push`](crate::controllers::physical_hands::hold::resist_push)).
    pub(crate) resist_onset_load: f32,
}

impl PalmTuning {
    fn validate(&self) -> Result<(), Invalid> {
        vec3("palms.palm_half_extents_m", self.palm_half_extents_m, 0.5)?;
        nonnegative("palms.push_deadband_m", self.push_deadband_m, 0.5)?;
        nonnegative(
            "palms.push_stiffness_per_s",
            self.push_stiffness_per_s,
            1000.0,
        )?;
        nonnegative("palms.max_push_speed_mps", self.max_push_speed_mps, 50.0)?;
        range(
            "palms.min_push_horizontal",
            self.min_push_horizontal,
            0.0,
            1.0,
        )?;
        positive("palms.full_hit_speed_mps", self.full_hit_speed_mps, 100.0)?;
        range(
            "palms.min_object_speed_mps",
            self.min_object_speed_mps,
            0.0,
            self.full_hit_speed_mps,
        )?;
        nonnegative("palms.object_impulse", self.object_impulse, 100.0)?;
        range(
            "palms.min_object_strength",
            self.min_object_strength,
            0.0,
            1.0,
        )?;
        nonnegative(
            "palms.carry_resist_deadband_m",
            self.carry_resist_deadband_m,
            2.0,
        )?;
        // The onset divides the load's span above it, so it must stay below one.
        range("palms.resist_onset_load", self.resist_onset_load, 0.0, 0.99)
    }
}

/// How long a palms' tick's results stay current, and how the palms' motion is smoothed.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateTuning {
    /// How long a tick's results stay current, milliseconds: the aim update runs every simulation
    /// tick while the palms act, so an older result means they have stopped. The carried bodies'
    /// collision, the remove-queue pause, the climb's native claim, and the pickups' input go stale
    /// with it.
    pub stale_after_ms: u64,
    /// How long the palms' velocities are smoothed over, seconds: short, so that a throw keeps the
    /// flick. The aim update's trackers and the hand bodies' trackers both smooth over it.
    pub velocity_smoothing_s: f32,
}

impl StateTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("state.stale_after_ms", self.stale_after_ms as f32, 10_000.0)?;
        nonnegative("state.velocity_smoothing_s", self.velocity_smoothing_s, 2.0)
    }
}

impl ShoveTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("shove.stiffness_per_s", self.stiffness_per_s, 1000.0)?;
        nonnegative("shove.deadband_m", self.deadband_m, 0.5)?;
        nonnegative("shove.max_speed_mps", self.max_speed_mps, 50.0)?;
        nonnegative("shove.max_force_n", self.max_force_n, 100_000.0)?;
        nonnegative("shove.friction", self.friction, 10.0)?;
        nonnegative("shove.give_smoothing_s", self.give_smoothing_s, 5.0)?;
        nonnegative("shove.min_asked_mps", self.min_asked_mps, 10.0)
    }
}

impl SoundsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative(
            "sounds.min_touch_speed_mps",
            self.min_touch_speed_mps,
            100.0,
        )?;
        range("sounds.min_strike_share", self.min_strike_share, 0.0, 1.0)?;
        nonnegative("sounds.hand_cooldown_s", self.hand_cooldown_s, 10.0)?;
        nonnegative("sounds.min_interval_s", self.min_interval_s, 10.0)
    }
}

/// The hold's solver
/// ([`solve_in_frame`](crate::controllers::physical_hands::hold::solve_in_frame)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldSolverTuning {
    /// The most of its error a constraint closes in one tick: below one, so that a stiff hold
    /// settles rather than overshoots at a slow tick.
    pub(crate) max_correction: f32,
    /// The solver's passes over the hands.
    pub(crate) iterations: usize,
}

impl HoldSolverTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("hold.max_correction", self.max_correction, 1.0)?;
        positive("hold.iterations", self.iterations as f32, 256.0)
    }
}

/// The hold's strain as a grip is taken
/// ([`Strain::acquired`](crate::controllers::physical_hands::grasp::Strain::acquired)): a body
/// still moving when the hand takes it is not judged caught while the hand's strength stops it.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraspTuning {
    /// How many times the body's stopping time at the hand's strength the strain waits.
    pub(crate) acquisition_stopping_scale: f32,
    /// The longest the strain waits, seconds.
    pub(crate) max_acquisition_s: f32,
}

impl GraspTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative(
            "grasp.acquisition_stopping_scale",
            self.acquisition_stopping_scale,
            100.0,
        )?;
        nonnegative("grasp.max_acquisition_s", self.max_acquisition_s, 10.0)
    }
}

/// The vibration of a hand that touches, presses, or pulls (the palms' `touch` module).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PressureHapticsTuning {
    /// The least load that pulses a hand, `0..=1` of its strength.
    pub(crate) min_pressure_load: f32,
    /// The weight of a shoved body's resistance in its palm's pressure, `0..=1`.
    pub(crate) pressure_resistance_share: f32,
    /// The strength of the gentlest palm touch's pulse, `0..=1`; a touch at
    /// [`PalmTuning::full_hit_speed_mps`] pulses at full strength.
    pub(crate) min_touch_scale: f32,
}

impl PressureHapticsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range(
            "haptics.min_pressure_load",
            self.min_pressure_load,
            0.0,
            1.0,
        )?;
        range(
            "haptics.pressure_resistance_share",
            self.pressure_resistance_share,
            0.0,
            1.0,
        )?;
        range("haptics.min_touch_scale", self.min_touch_scale, 0.0, 1.0)
    }
}

/// How a carried body's collision with the player is given back once the hand lets go.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CarryCollisionTuning {
    /// The gap two bodies' boxes must have on one axis to count as clear of each other, metres.
    pub(crate) release_margin_m: f32,
    /// The longest a let-go body stays kept off the player while it still overlaps, seconds.
    pub(crate) release_grace_s: f32,
}

impl CarryCollisionTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative(
            "carry_collision.release_margin_m",
            self.release_margin_m,
            1.0,
        )?;
        nonnegative(
            "carry_collision.release_grace_s",
            self.release_grace_s,
            60.0,
        )
    }
}

/// The physical hands' side of the grapple snatch
/// ([`crate::controllers::physical_hands::remote`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteTuning {
    /// The share of the line's pull on a ragdoll the hooked body takes at the hook, `0..=1`.
    pub(crate) ragdoll_lead_share: f32,
    /// The most boxes
    /// [`dynamic_near`](crate::controllers::physical_hands::remote::dynamic_near) considers: its
    /// filter keeps only boxes near the aim, so this bounds a pathological pile of debris, not an
    /// ordinary scene.
    pub(crate) max_near_bodies: usize,
}

impl RemoteTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range(
            "remote.ragdoll_lead_share",
            self.ragdoll_lead_share,
            0.0,
            1.0,
        )?;
        positive(
            "remote.max_near_bodies",
            self.max_near_bodies as f32,
            4096.0,
        )
    }
}

/// The damping of a held articulated object's assembly
/// ([`crate::controllers::physical_hands::articulated`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArticulatedTuning {
    /// The most bodies one assembly's damping takes: a rack or a hinged contraption has a handful,
    /// so this bounds a walk into something far larger, not a real assembly.
    pub(crate) max_assembly_bodies: usize,
}

/// The physical hands' grip targets (the palms' `targets` module).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetsTuning {
    /// The marker on the grab point of what a hand holds, metres.
    pub(crate) held_mark_m: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(PHYSICAL_HANDS.get().validate(), Ok(()));
    }
}
