//! The finger curls' tuning, read from the shipped `tuning/fingers.toml` ([`FINGERS_TUNING`]): the
//! curl-to-bend mapping, the splay, and the smoothing ([`FingerTuning`]), the curls measured from
//! tracked joints and estimated from the inputs, the grip poses' shapes, the contact fit, and the
//! bar diameters of the sources that hold one.

use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::{
    controllers::fingers::grip::catalogue::{FingerRole, PoseShape},
    tuning::Millis,
};

jc3vr_core::tuning_file! {
    /// The finger curls' tuning file.
    pub static FINGERS_TUNING: FingersTuning = "fingers.toml";
}

/// The finger curls' tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FingersTuning {
    pub curls: FingerTuning,
    pub tracked: TrackedCurlsTuning,
    pub estimate: EstimatedCurlsTuning,
    pub poses: GripPosesTuning,
    pub contact: ContactFitTuning,
    pub diameters: SourceDiametersTuning,
}

impl Tuning for FingersTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.curls.validate()?;
        self.tracked.validate()?;
        self.estimate.validate()?;
        self.poses.validate()?;
        self.contact.validate()?;
        self.diameters.validate()
    }
}

/// The curl-to-bend mapping, the splay, the smoothing, and the grip poses' blend.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FingerTuning {
    /// The bend each finger takes at its open and closed curl, as a share of its full joint
    /// bends, indexed by [`Finger::index`](crate::controllers::fingers::curl::Finger::index).
    pub curl_ranges: [CurlRange; 5],
    /// The full bend of a finger's proximal, middle, and distal joints from straight, degrees: the
    /// bend at a range share of one.
    pub finger_bend_deg: [f32; 3],
    /// The full bend of the thumb's base, middle, and distal joints across the palm, degrees.
    pub thumb_bend_deg: [f32; 3],
    /// Each joint's bend at a share of zero, degrees, indexed by finger and then from the root
    /// joint outward: a correction to the straight finger built from the animated hand, positive
    /// toward the palm.
    pub rest_offset_deg: [[f32; 3]; 5],
    /// How a full hook scales each joint's share of the bend, root outward: the knuckle nearly
    /// straight and the outer joints bent about half again as far.
    pub hook_shift: [f32; 3],
    /// The open thumb's turn from the fingers' direction toward the thumb side, in the palm's
    /// plane, degrees.
    pub thumb_spread_deg: f32,
    /// The share of the tracked finger splay applied (hand tracking only).
    pub splay_scale: f32,
    /// The largest splay applied either way, degrees.
    pub max_splay_deg: f32,
    /// The time constant the curls follow their source with, seconds. The estimate jumps when a
    /// touch sensor changes, and the smoothing turns the jump into a quick motion.
    pub smoothing_s: f32,
    /// The grip poses: the fingers wrap what the hand holds.
    pub grip: GripPoseTuning,
}

impl FingerTuning {
    fn validate(&self) -> Result<(), Invalid> {
        for (i, r) in self.curl_ranges.iter().enumerate() {
            range(&format!("curls.curl_ranges[{i}].open"), r.open, 0.0, 1.0)?;
            range(
                &format!("curls.curl_ranges[{i}].closed"),
                r.closed,
                0.0,
                1.0,
            )?;
        }
        for (k, &deg) in self.finger_bend_deg.iter().enumerate() {
            nonnegative(&format!("curls.finger_bend_deg[{k}]"), deg, 180.0)?;
        }
        for (k, &deg) in self.thumb_bend_deg.iter().enumerate() {
            nonnegative(&format!("curls.thumb_bend_deg[{k}]"), deg, 180.0)?;
        }
        for (i, joints) in self.rest_offset_deg.iter().enumerate() {
            for (k, &deg) in joints.iter().enumerate() {
                range(
                    &format!("curls.rest_offset_deg[{i}][{k}]"),
                    deg,
                    -90.0,
                    90.0,
                )?;
            }
        }
        for (k, &shift) in self.hook_shift.iter().enumerate() {
            range(&format!("curls.hook_shift[{k}]"), shift, -1.0, 2.0)?;
        }
        nonnegative("curls.thumb_spread_deg", self.thumb_spread_deg, 90.0)?;
        nonnegative("curls.splay_scale", self.splay_scale, 4.0)?;
        nonnegative("curls.max_splay_deg", self.max_splay_deg, 90.0)?;
        positive("curls.smoothing_s", self.smoothing_s, 2.0)?;
        let grip = &self.grip;
        range("curls.grip.light_share", grip.light_share, 0.0, 1.0)?;
        range("curls.grip.firm_squeeze", grip.firm_squeeze, 0.01, 1.0)?;
        positive("curls.grip.blend_s", grip.blend_s, 2.0)?;
        nonnegative(
            "curls.grip.max_opposition_deg",
            grip.max_opposition_deg,
            90.0,
        )
    }
}

/// The grip poses' tuning, held in [`FingerTuning::grip`].
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GripPoseTuning {
    /// The pose's share at the lightest grip that holds, `0..=1`; it rises to the full pose at
    /// [`firm_squeeze`](Self::firm_squeeze).
    pub light_share: f32,
    /// The grip's squeeze at which the pose is fully on, `0..=1`.
    pub firm_squeeze: f32,
    /// The time constant the pose's share follows the grip with, seconds, so that taking hold
    /// closes the fingers and letting go opens them over a short motion.
    pub blend_s: f32,
    /// The thumb's largest turn out of the palm's plane toward the palm side, at full opposition,
    /// degrees.
    pub max_opposition_deg: f32,
}

/// The bend a finger takes at its open (curl 0) and closed (curl 1) ends, as a share of its full
/// joint bends from straight. The bend replaces the animated one rather than adding to it.
#[derive(Debug, Copy, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurlRange {
    pub open: f32,
    pub closed: f32,
}

impl CurlRange {
    /// The bend share at `curl`.
    pub fn at(self, curl: f32) -> f32 {
        self.open + (self.closed - self.open) * curl.clamp(0.0, 1.0)
    }
}

/// The curls measured from a tracked hand skeleton
/// ([`from_joints`](crate::controllers::fingers::curl::from_joints)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackedCurlsTuning {
    /// A finger's knuckle, middle, and last joint flexions at a full fist, degrees: a finger's
    /// curl is its flexions' sum over theirs.
    pub finger_full_flex_deg: [f32; 3],
    /// The thumb's two outer flexions at a full bend, degrees.
    pub thumb_full_flex_deg: [f32; 2],
}

impl TrackedCurlsTuning {
    /// The sum of a finger's flexions at a full fist, radians.
    pub fn finger_full_flex_rad(&self) -> f32 {
        let [a, b, c] = self.finger_full_flex_deg;
        (a + b + c) * std::f32::consts::PI / 180.0
    }

    /// The sum of the thumb's flexions at a full bend, radians.
    pub fn thumb_full_flex_rad(&self) -> f32 {
        let [a, b] = self.thumb_full_flex_deg;
        (a + b) * std::f32::consts::PI / 180.0
    }

    fn validate(&self) -> Result<(), Invalid> {
        for (k, &deg) in self.finger_full_flex_deg.iter().enumerate() {
            positive(&format!("tracked.finger_full_flex_deg[{k}]"), deg, 180.0)?;
        }
        for (k, &deg) in self.thumb_full_flex_deg.iter().enumerate() {
            positive(&format!("tracked.thumb_full_flex_deg[{k}]"), deg, 180.0)?;
        }
        Ok(())
    }
}

/// The curls estimated from the inputs the fingers rest on
/// ([`estimate`](crate::controllers::fingers::curl::estimate)), on the finger curls' scale.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstimatedCurlsTuning {
    /// The thumb's curl resting on the thumbstick.
    pub thumb_on_stick: f32,
    /// The thumb's curl resting on the trackpad.
    pub thumb_on_trackpad: f32,
    /// The thumb's curl resting on the A button. The face buttons sit nearer the palm than the
    /// thumbstick, so the thumb bends further to reach them.
    pub thumb_on_a: f32,
    /// The thumb's curl resting on the B button.
    pub thumb_on_b: f32,
    /// The extra curl of a thumb pressing down.
    pub thumb_press_extra: f32,
    /// The trackpad force past which the thumb counts as pressing it.
    pub trackpad_pressed_force: f32,
    /// The index finger's curl resting on the trigger.
    pub trigger_rest: f32,
    /// The curl of fingers resting on a Touch controller's grip without pulling it.
    pub touch_grip_rest: f32,
}

impl EstimatedCurlsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range("estimate.thumb_on_stick", self.thumb_on_stick, 0.0, 1.0)?;
        range(
            "estimate.thumb_on_trackpad",
            self.thumb_on_trackpad,
            0.0,
            1.0,
        )?;
        range("estimate.thumb_on_a", self.thumb_on_a, 0.0, 1.0)?;
        range("estimate.thumb_on_b", self.thumb_on_b, 0.0, 1.0)?;
        range(
            "estimate.thumb_press_extra",
            self.thumb_press_extra,
            0.0,
            1.0,
        )?;
        range(
            "estimate.trackpad_pressed_force",
            self.trackpad_pressed_force,
            0.0,
            1.0,
        )?;
        range("estimate.trigger_rest", self.trigger_rest, 0.0, 1.0)?;
        range("estimate.touch_grip_rest", self.touch_grip_rest, 0.0, 1.0)
    }
}

/// The named grip poses' shapes
/// ([`GripPose::shape`](crate::controllers::fingers::grip::catalogue::GripPose::shape)). The
/// values are judged by eye against a hand holding the thing, not measured.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GripPosesTuning {
    pub pistol_grip: PoseShape,
    pub foregrip: PoseShape,
    pub magazine_pinch: PoseShape,
    pub magazine_cup: PoseShape,
    pub slide_pinch: PoseShape,
    pub grenade_palm: PoseShape,
    pub charge_palm: PoseShape,
    pub ledge_hook: PoseShape,
    pub flat_palm: PoseShape,
    pub wrap: PoseShape,
    pub cylinder: CylinderPoseTuning,
    pub door_handle: PoseShape,
    pub collar_grab: PoseShape,
}

impl GripPosesTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let shapes = [
            ("pistol_grip", &self.pistol_grip),
            ("foregrip", &self.foregrip),
            ("magazine_pinch", &self.magazine_pinch),
            ("magazine_cup", &self.magazine_cup),
            ("slide_pinch", &self.slide_pinch),
            ("grenade_palm", &self.grenade_palm),
            ("charge_palm", &self.charge_palm),
            ("ledge_hook", &self.ledge_hook),
            ("flat_palm", &self.flat_palm),
            ("wrap", &self.wrap),
            ("door_handle", &self.door_handle),
            ("collar_grab", &self.collar_grab),
        ];
        for (name, shape) in shapes {
            validate_shape(&format!("poses.{name}"), shape)?;
        }
        self.cylinder.validate()
    }
}

fn validate_shape(name: &str, shape: &PoseShape) -> Result<(), Invalid> {
    for i in 0..5 {
        range(&format!("{name}.curl[{i}]"), shape.curl[i], 0.0, 1.0)?;
        range(&format!("{name}.splay[{i}]"), shape.splay[i], -0.5, 0.5)?;
        range(&format!("{name}.hook[{i}]"), shape.hook[i], -1.0, 1.0)?;
    }
    range(&format!("{name}.opposition"), shape.opposition, 0.0, 1.0)?;
    if let Some(spread) = shape.thumb_spread_deg {
        nonnegative(&format!("{name}.thumb_spread_deg"), spread, 90.0)?;
    }
    validate_roles(name, &shape.roles)
}

fn validate_roles(name: &str, roles: &[FingerRole; 5]) -> Result<(), Invalid> {
    for (i, role) in roles.iter().enumerate() {
        if let FingerRole::Trigger { rest, pulled } = *role {
            range(&format!("{name}.roles[{i}].rest"), rest, 0.0, 1.0)?;
            range(&format!("{name}.roles[{i}].pulled"), pulled, 0.0, 1.0)?;
        }
    }
    Ok(())
}

/// The round bar's pose, whose curls follow the bar's diameter
/// ([`cylinder_curl`](crate::controllers::fingers::grip::catalogue::cylinder_curl)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CylinderPoseTuning {
    /// The fingers' curl at judged bar diameters, as `[diameter in metres, curl]`, thinnest first.
    /// The curl is interpolated between the points, and a thicker bar never closes the fingers
    /// further.
    pub curls: [[f32; 2]; 6],
    /// How much less the thumb curls than the fingers.
    pub thumb_less: f32,
    /// The least the thumb curls.
    pub thumb_min: f32,
    /// How much more the ring finger curls than the index and middle fingers.
    pub ring_more: f32,
    /// How much more the little finger curls than the index and middle fingers.
    pub little_more: f32,
    pub splay: [f32; 5],
    pub opposition: f32,
    pub thumb_spread_deg: f32,
    pub roles: [FingerRole; 5],
}

impl CylinderPoseTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let mut previous: Option<[f32; 2]> = None;
        for (i, &[diameter, curl]) in self.curls.iter().enumerate() {
            positive(&format!("poses.cylinder.curls[{i}][0]"), diameter, 1.0)?;
            range(&format!("poses.cylinder.curls[{i}][1]"), curl, 0.0, 1.0)?;
            if let Some([d0, c0]) = previous
                && (diameter <= d0 || curl > c0)
            {
                return Err(Invalid(format!(
                    "poses.cylinder.curls[{i}] must be thicker than the point before it and curl \
                     no further, got [{diameter}, {curl}] after [{d0}, {c0}]"
                )));
            }
            previous = Some([diameter, curl]);
        }
        range("poses.cylinder.thumb_less", self.thumb_less, 0.0, 1.0)?;
        range("poses.cylinder.thumb_min", self.thumb_min, 0.0, 1.0)?;
        range("poses.cylinder.ring_more", self.ring_more, 0.0, 1.0)?;
        range("poses.cylinder.little_more", self.little_more, 0.0, 1.0)?;
        for (i, &splay) in self.splay.iter().enumerate() {
            range(&format!("poses.cylinder.splay[{i}]"), splay, -0.5, 0.5)?;
        }
        range("poses.cylinder.opposition", self.opposition, 0.0, 1.0)?;
        nonnegative(
            "poses.cylinder.thumb_spread_deg",
            self.thumb_spread_deg,
            90.0,
        )?;
        validate_roles("poses.cylinder", &self.roles)
    }
}

/// The contact fit ([`contact`](crate::controllers::fingers::grip::contact)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactFitTuning {
    /// The distal bone's length as a share of the middle bone's: the distal bone has no joint
    /// beyond it to measure.
    pub distal_share: f32,
    /// The steps a sweep takes from straight to a fist.
    pub sweep_steps: u32,
    /// The fingers of a hand swept per tick.
    pub fingers_per_tick: usize,
    /// How long a fitted hold keeps its fit before it is swept again.
    pub refit_ms: Millis,
}

impl ContactFitTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("contact.distal_share", self.distal_share, 2.0)?;
        if !(1..=32).contains(&self.sweep_steps) {
            return Err(Invalid(format!(
                "contact.sweep_steps must be in [1, 32], got {}",
                self.sweep_steps
            )));
        }
        if !(1..=5).contains(&self.fingers_per_tick) {
            return Err(Invalid(format!(
                "contact.fingers_per_tick must be in [1, 5], got {}",
                self.fingers_per_tick
            )));
        }
        self.refit_ms.validate("contact.refit_ms", 10_000)
    }
}

/// The bar diameter, metres, of each grip source whose hold is a round bar
/// ([`diameters`](crate::controllers::fingers::grip::diameters)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDiametersTuning {
    pub articulated_m: f32,
    pub stunt_m: f32,
    pub wheel_m: f32,
    pub handlebars_m: f32,
    pub stick_m: f32,
    pub lever_m: f32,
    pub parachute_handle_m: f32,
    pub mounted_gun_m: f32,
    pub grapple_line_m: f32,
}

impl SourceDiametersTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let diameters = [
            ("diameters.articulated_m", self.articulated_m),
            ("diameters.stunt_m", self.stunt_m),
            ("diameters.wheel_m", self.wheel_m),
            ("diameters.handlebars_m", self.handlebars_m),
            ("diameters.stick_m", self.stick_m),
            ("diameters.lever_m", self.lever_m),
            ("diameters.parachute_handle_m", self.parachute_handle_m),
            ("diameters.mounted_gun_m", self.mounted_gun_m),
            ("diameters.grapple_line_m", self.grapple_line_m),
        ];
        for (name, diameter) in diameters {
            positive(name, diameter, 1.0)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(FINGERS_TUNING.get().validate(), Ok(()));
    }
}
