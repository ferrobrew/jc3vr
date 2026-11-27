//! The C4 throw's, the press's, and the detonator's tuning, read from the shipped `tuning/c4.toml`
//! ([`C4`]).

use glam::Vec3;
use jc3vr_core::{
    config::{Invalid, nonnegative, positive},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::weapons::{grenade::tuning::hold_placement, in_hand::HoldPlacement};

jc3vr_core::tuning_file! {
    /// The C4's tuning file.
    pub static C4: C4Tuning = "c4.toml";
}

/// The C4's tuning file's root: the throw of a charge from the C4 pouch (see
/// [`crate::controllers::weapons::c4`]), the press onto a surface, the placed charges, the
/// detonation, and the detonator. The pouch itself is placed with the others
/// ([`PouchTuning`](crate::controllers::weapons::volumes::PouchTuning)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct C4Tuning {
    /// Where the held charge's in-hand model sits in the hand ([`HoldPlacement`]), derived from the
    /// C4 weapon's in-hand model: a flat square brick lying in the model's XZ plane, centred on its
    /// origin there, whose origin is at the sticking (bottom) face and whose top face is
    /// [`CHARGE_TOP_M`] up the model's +Y. The top face lies on the palm, a little out of it, so
    /// the model's +Y points into the palm (the right grip's +X) and the sticking face looks out of
    /// the hand, at a surface the charge is pressed onto. The brick's sides run up the finger tube
    /// (the model's +Z along the grip's -Z) and along the hand; it is centred on the middle of the
    /// palm.
    pub hold: HoldPlacement,
    /// Where the detonator's model sits in the hand ([`HoldPlacement`]), derived from the trigger
    /// model's bounds: an upright cylinder about 3.3 cm across and
    /// [`DETONATOR_LENGTH_M`](crate::controllers::weapons::c4::detonator::DETONATOR_LENGTH_M) long,
    /// standing on its origin along the model's +Y. It stands up the fist as a pistol grip does,
    /// its +Y along the grip's -Z, toward the thumb, centred on the grip's origin, the middle of
    /// the closed hand.
    pub(crate) detonator_hold: HoldPlacement,
    /// Pressing a held charge onto a surface sticks it there.
    pub stick: StickTuning,
    /// Taking a placed charge back into a hand.
    pub placed: PlacedTuning,
    /// A trigger's request to detonate the placed charges.
    pub(crate) detonation: DetonationTuning,
    /// A charge chosen on the weapon wheel.
    pub(crate) wheel: WheelTuning,
}

impl Tuning for C4Tuning {
    fn validate(&self) -> Result<(), Invalid> {
        hold_placement("hold", &self.hold)?;
        hold_placement("detonator_hold", &self.detonator_hold)?;
        self.stick.validate()?;
        self.placed.validate()?;
        self.detonation.validate()?;
        self.wheel.validate()
    }
}

/// How far the in-hand model's top face is from its origin along its +Y, metres.
pub(crate) const CHARGE_TOP_M: f32 = 0.029;

/// The middle of the charge in its in-hand model's frame, metres: where the surface probes start.
pub(crate) const CHARGE_CENTRE_M: Vec3 = Vec3::new(0.0, CHARGE_TOP_M / 2.0, 0.0);

/// Pressing a held charge onto a surface sticks it there.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StickTuning {
    /// How far from the held charge's centre a surface counts as touching it, metres: about half
    /// the charge's thickness.
    pub(crate) contact_m: f32,
    /// How far off the surface the charge is placed before it is pushed onto it, metres.
    pub(crate) standoff_m: f32,
    /// How fast the charge is pushed onto the surface, metres per second, so that the charge's own
    /// collision sticks it there.
    pub(crate) push_mps: f32,
    /// How long after a charge comes out of the pouch before it sticks, seconds, so that the body
    /// the pouch is on is not mistaken for a surface as the hand leaves it.
    pub(crate) after_prime_s: f32,
    /// How far from the held charge a surface is previewed, where the charge would stick if pressed
    /// on, metres.
    pub(crate) preview_m: f32,
}

impl StickTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("stick.contact_m", self.contact_m, 1.0)?;
        nonnegative("stick.standoff_m", self.standoff_m, 1.0)?;
        positive("stick.push_mps", self.push_mps, 50.0)?;
        nonnegative("stick.after_prime_s", self.after_prime_s, 10.0)?;
        positive("stick.preview_m", self.preview_m, 5.0)
    }
}

/// Taking a placed charge back into a hand (see [`placed`]).
///
/// [`placed`]: mod@crate::controllers::weapons::c4::placed
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedTuning {
    /// How near a placed charge a hand must come to take it, metres: about the charge's size, with
    /// the slack the drawn hand needs.
    pub radius_m: f32,
    /// How far from the player a placed charge is offered, metres: beyond any reach, so that only
    /// the few charges a hand might take are carried into the cockpit frame.
    pub(crate) offer_range_m: f32,
}

impl PlacedTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("placed.radius_m", self.radius_m, 1.0)?;
        positive("placed.offer_range_m", self.offer_range_m, 50.0)
    }
}

/// A trigger's request to detonate the placed charges: the detonator's, or that of the hand
/// holding a charge.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DetonationTuning {
    /// How long a detonation request waits for the weapon to start the detonation, seconds: long
    /// enough for the character to finish an act that keeps the detonate act out.
    pub timeout_s: f32,
    /// The longest a started detonation's charges count as on their way to going off, seconds: the
    /// detonate act's lead-in and the charges' own timers. A charge still live after this did not
    /// go off, and a press may detonate it again.
    pub(crate) in_flight_s: f32,
}

impl DetonationTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("detonation.timeout_s", self.timeout_s, 10.0)?;
        positive("detonation.in_flight_s", self.in_flight_s, 30.0)
    }
}

/// A charge chosen on the weapon wheel (see [`crate::controllers::weapons::c4::wheel`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WheelTuning {
    /// How long the game's planting mode has to end once it is left, seconds, before the pending
    /// charge is forgotten.
    pub(crate) leave_timeout_s: f32,
}

impl WheelTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("wheel.leave_timeout_s", self.leave_timeout_s, 30.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(C4.get().validate(), Ok(()));
    }
}
