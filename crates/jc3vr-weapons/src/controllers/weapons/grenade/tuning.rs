//! The grenade throw's and the catch's tuning, read from the shipped `tuning/grenade.toml`
//! ([`GRENADE`]): the hold, the fuse warning, the catch, the pin, and the locker's reach.

use glam::Vec3;
use jc3vr_body::tuning::Millis;
use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range, vec3},
    tuning::Tuning,
};
use jc3vr_view::controllers::overlay::primitive::Rgba;
use serde::Deserialize;

use crate::controllers::weapons::in_hand::HoldPlacement;

jc3vr_core::tuning_file! {
    /// The grenades' tuning file.
    pub static GRENADE: GrenadeTuning = "grenade.toml";
}

/// The grenades' tuning file's root: the physical grenade throw, its hold, its release, its fuse
/// warning, its pin, and the catch.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrenadeTuning {
    /// Where the held grenade's model sits in the hand ([`HoldPlacement`]), derived from its model
    /// (the grenade weapon's projectile model): the origin is the middle of the round body, the
    /// body runs along the model's +Y up to the fuse head, and the lever lies down the body's +Z
    /// side. Upright in the fist, the model's +Y runs up the finger tube (the grip's -Z) and its
    /// +Z, the lever, into the palm (the right grip's +X). The body's centre is a body radius
    /// ([`GRENADE_BODY_RADIUS_M`]) and a little out of the palm, level with the middle of the palm
    /// along the hand, and a little toward the little finger so that the fuse head clears the top
    /// of the fist.
    pub hold: HoldPlacement,
    /// The fuse warning on the hand holding a live grenade.
    pub warning: WarningTuning,
    /// Catching live grenades.
    pub catch: CatchTuning,
    /// The held grenade's pin.
    pub pin: PinTuning,
    /// Taking grenades from a weapon locker.
    pub locker: LockerTuning,
    /// The render-time placement of the explosives a hand holds, the grenades' and the C4
    /// charge's ([`crate::controllers::weapons::in_hand`]).
    pub held: HeldTuning,
}

impl Tuning for GrenadeTuning {
    fn validate(&self) -> Result<(), Invalid> {
        hold_placement("hold", &self.hold)?;
        self.warning.validate()?;
        self.catch.validate()?;
        self.pin.validate()?;
        positive("locker.reach_m", self.locker.reach_m, 1.0)?;
        self.held
            .registration_ms
            .validate("held.registration_ms", 10_000)
    }
}

/// The render-time placement of the explosives a hand holds
/// ([`crate::controllers::weapons::in_hand`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldTuning {
    /// How long a held explosive's registration stays current without a refresh: a little past a
    /// tick.
    pub(crate) registration_ms: Millis,
}

/// The fuse warning: a ring at the hand holding a live grenade, turning from calm to urgent as the
/// fuse runs down, and ticks on that hand from the pin on, faster as it runs down. Each value that
/// changes with the fuse is given when the fuse is lit and grows by its growth as the fuse runs
/// out ([`crate::controllers::weapons::grenade::fuse_urgency`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarningTuning {
    /// The gap between warning ticks when the fuse is lit, and when it is about to run out,
    /// seconds.
    pub interval_s: [f32; 2],
    /// The ring's size in the world when the fuse is lit, metres.
    pub(crate) ring_size_m: f32,
    /// How much the ring grows by the time the fuse runs out, metres.
    pub(crate) ring_size_growth_m: f32,
    /// The ring's thickness when the fuse is lit, as a share of its radius.
    pub ring_thickness: f32,
    /// How much the ring thickens by the time the fuse runs out, as a share of its radius.
    pub(crate) ring_thickness_growth: f32,
    /// The ring's colour when the fuse is lit.
    pub(crate) calm_rgba: Rgba,
    /// The ring's colour when the fuse is about to run out.
    pub(crate) urgent_rgba: Rgba,
    /// The ring's opacity where something is in front of it, as a share of its colour's.
    pub occluded_alpha: f32,
    /// How long one tick's ring lasts when it is not drawn again, milliseconds.
    pub ttl_ms: u64,
}

impl WarningTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let [lit, out] = self.interval_s;
        positive("warning.interval_s[0]", lit, 10.0)?;
        positive("warning.interval_s[1]", out, 10.0)?;
        positive("warning.ring_size_m", self.ring_size_m, 1.0)?;
        nonnegative("warning.ring_size_growth_m", self.ring_size_growth_m, 1.0)?;
        range("warning.ring_thickness", self.ring_thickness, 0.0, 1.0)?;
        range(
            "warning.ring_thickness_growth",
            self.ring_thickness_growth,
            0.0,
            1.0 - self.ring_thickness,
        )?;
        for (key, colour) in [
            ("warning.calm_rgba", self.calm_rgba),
            ("warning.urgent_rgba", self.urgent_rgba),
        ] {
            for (channel, value) in ["r", "g", "b", "a"].iter().zip(colour) {
                range(&format!("{key}.{channel}"), value, 0.0, 1.0)?;
            }
        }
        range("warning.occluded_alpha", self.occluded_alpha, 0.0, 1.0)?;
        if self.ttl_ms == 0 {
            return Err(Invalid("warning.ttl_ms must be positive, got 0".to_owned()));
        }
        Ok(())
    }
}

/// Catching a live grenade near the player and throwing it on.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatchTuning {
    /// The radius of a live grenade's grab volume, metres. A grenade in flight moves several
    /// centimetres between two ticks, so it is larger than a pouch's.
    pub radius_m: f32,
    /// How far from the player a live grenade may be to be caught, metres.
    pub range_m: f32,
    /// How long after the player's own throw the grenade cannot be caught, seconds, so that the
    /// hand that threw it does not take it back as it leaves.
    pub(crate) own_grace_s: f32,
    /// The fastest a caught grenade is moved toward the hand, metres per second.
    pub(crate) max_carry_mps: f32,
}

impl CatchTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("catch.radius_m", self.radius_m, 1.0)?;
        positive("catch.range_m", self.range_m, 50.0)?;
        nonnegative("catch.own_grace_s", self.own_grace_s, 10.0)?;
        positive("catch.max_carry_mps", self.max_carry_mps, 200.0)
    }
}

/// The held grenade's pin, pulled by the other hand or by the teeth (see the grenade's `pin`
/// module).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinTuning {
    /// The pin's reach, metres at the player's scale: larger than the ring, so that a hand reaching
    /// for the top of the grenade takes the pin without aiming for it, and smaller than the
    /// grenade's own reach (the hold's
    /// [`handle_radius_m`](crate::controllers::weapons::hold::tuning::HoldTuning::handle_radius_m)),
    /// so that a hand closing on the grenade's body still takes the grenade.
    pub radius_m: f32,
    /// How long the pin stays at the mouth before the teeth pull it, seconds: longer than a grenade
    /// passing the face takes.
    pub(crate) teeth_dwell_s: f32,
    /// The mouth in the head's frame (the eye midpoint, -Z forward, +Y up), metres.
    pub(crate) mouth_in_head_m: Vec3,
    /// How near the mouth the pin must come for the teeth to take it, metres.
    pub(crate) mouth_radius_m: f32,
}

impl PinTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("pin.radius_m", self.radius_m, 1.0)?;
        positive("pin.teeth_dwell_s", self.teeth_dwell_s, 10.0)?;
        vec3("pin.mouth_in_head_m", self.mouth_in_head_m, 1.0)?;
        positive("pin.mouth_radius_m", self.mouth_radius_m, 1.0)
    }
}

/// Taking grenades from a weapon locker (see the grenade's `locker` module).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockerTuning {
    /// How near a locker's grenade a hand must come to take it, metres.
    pub reach_m: f32,
}

/// The radius of the grenade model's round body, metres.
pub const GRENADE_BODY_RADIUS_M: f32 = 0.034;

/// Check a hold placement read from a tuning file at `name`: its offset within a metre of the
/// grip, and its axes the columns of a rotation (unit length, at right angles, and right-handed).
pub(crate) fn hold_placement(name: &str, hold: &HoldPlacement) -> Result<(), Invalid> {
    vec3(&format!("{name}.offset_m"), hold.offset_m, 1.0)?;
    let [x, y, z] = hold.axes;
    for (index, axis) in hold.axes.iter().enumerate() {
        vec3(&format!("{name}.axes[{index}]"), *axis, 1.0)?;
    }
    let rotation = (x.length() - 1.0).abs() < AXIS_TOLERANCE
        && (y.length() - 1.0).abs() < AXIS_TOLERANCE
        && x.dot(y).abs() < AXIS_TOLERANCE
        && x.cross(y).distance(z) < AXIS_TOLERANCE;
    if !rotation {
        return Err(Invalid(format!(
            "{name}.axes must be the columns of a rotation (unit, at right angles, right-handed), \
             got {x}, {y}, {z}"
        )));
    }
    Ok(())
}

/// How far a hold's axes may be from a rotation's, for the rounding of a hand-written file.
const AXIS_TOLERANCE: f32 = 1e-3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(GRENADE.get().validate(), Ok(()));
    }

    #[test]
    fn a_hold_whose_axes_are_not_a_rotation_is_refused() {
        let mirrored = HoldPlacement {
            axes: [Vec3::X, Vec3::Y, Vec3::NEG_Z],
            ..GRENADE.get().hold
        };
        assert!(hold_placement("hold", &mirrored).is_err());
        assert_eq!(hold_placement("hold", &GRENADE.get().hold), Ok(()));
    }
}
