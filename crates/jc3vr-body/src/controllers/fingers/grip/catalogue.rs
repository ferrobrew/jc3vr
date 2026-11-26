//! The grip poses, pure: each held thing's finger pose as a shape for every finger, and the part
//! each finger plays in it.
//!
//! A pose's curls are on the finger curls' scale (0 straight, 1 a fist), its hooks and opposition on
//! the [`HandShape`](crate::controllers::fingers::pose::HandShape) scale. The values are judged by
//! eye against a hand holding the thing, not measured, so the headset checklist in
//! `docs/mod/input/grip-poses.md` is where they are tuned; they live in the finger curls' tuning
//! ([`FINGERS_TUNING`]'s `poses`).

use serde::Deserialize;

use crate::controllers::fingers::tuning::FINGERS_TUNING;

/// A named finger pose for what a hand holds.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum GripPose {
    /// A pistol grip: the fingers round the grip, the thumb over them, and the trigger finger on
    /// the trigger, following its pull.
    PistolGrip,
    /// A rifle's foregrip or handguard: a wider wrap, the thumb along the side.
    Foregrip,
    /// A magazine or a round held between the thumb and the first fingers.
    MagazinePinch,
    /// A gun held by the magazine in its well: the palm under it and the fingers cupped round it.
    MagazineCup,
    /// A gun held by its slide or charging handle: pinched between the thumb and the bent index
    /// and middle fingers, the others tucked.
    SlidePinch,
    /// A grenade in the palm: every finger round a ball, spread a little.
    GrenadePalm,
    /// A C4 charge: a flat brick lying on the palm, the fingers bent at the knuckles and the middle
    /// joints over its far edge onto its outer face, the thumb along its side.
    ChargePalm,
    /// Fingers hooked over an edge, the knuckles straight and the outer joints bent, the thumb
    /// alongside.
    LedgeHook,
    /// A flat palm on a crate or a panel: the fingers nearly straight.
    FlatPalm,
    /// A loose object of unknown shape: a loose wrap, which the contact fit closes onto the
    /// object's surface.
    Wrap,
    /// A round bar of this diameter in metres: a steering wheel's rim, handlebars, a stick, a
    /// lever, a handle, or a rope. A thinner bar closes the fingers further.
    Cylinder { diameter_m: f32 },
    /// A door handle: the fingers round the handle and the thumb pressing on it.
    DoorHandle,
    /// A fistful of clothing: the fingers closed, the thumb pinching against the index finger.
    CollarGrab,
}

impl GripPose {
    pub fn label(self) -> &'static str {
        match self {
            GripPose::PistolGrip => "pistol grip",
            GripPose::Foregrip => "foregrip",
            GripPose::MagazinePinch => "magazine pinch",
            GripPose::MagazineCup => "magazine cup",
            GripPose::SlidePinch => "slide pinch",
            GripPose::GrenadePalm => "grenade palm",
            GripPose::ChargePalm => "charge palm",
            GripPose::LedgeHook => "ledge hook",
            GripPose::FlatPalm => "flat palm",
            GripPose::Wrap => "wrap",
            GripPose::Cylinder { .. } => "cylinder",
            GripPose::DoorHandle => "door handle",
            GripPose::CollarGrab => "collar grab",
        }
    }

    /// The pose's shape.
    pub fn shape(self) -> PoseShape {
        let poses = &FINGERS_TUNING.get().poses;
        match self {
            GripPose::PistolGrip => poses.pistol_grip,
            GripPose::Foregrip => poses.foregrip,
            GripPose::MagazinePinch => poses.magazine_pinch,
            GripPose::MagazineCup => poses.magazine_cup,
            GripPose::SlidePinch => poses.slide_pinch,
            GripPose::GrenadePalm => poses.grenade_palm,
            GripPose::ChargePalm => poses.charge_palm,
            GripPose::LedgeHook => poses.ledge_hook,
            GripPose::FlatPalm => poses.flat_palm,
            GripPose::Wrap => poses.wrap,
            GripPose::Cylinder { diameter_m } => {
                let cylinder = &poses.cylinder;
                let finger = cylinder_curl(diameter_m);
                PoseShape {
                    curl: [
                        (finger - cylinder.thumb_less).max(cylinder.thumb_min),
                        finger,
                        finger,
                        (finger + cylinder.ring_more).min(1.0),
                        (finger + cylinder.little_more).min(1.0),
                    ],
                    splay: cylinder.splay,
                    hook: [0.0; 5],
                    opposition: cylinder.opposition,
                    thumb_spread_deg: Some(cylinder.thumb_spread_deg),
                    roles: cylinder.roles,
                }
            }
            GripPose::DoorHandle => poses.door_handle,
            GripPose::CollarGrab => poses.collar_grab,
        }
    }
}

/// A pose's target for every finger, indexed by
/// [`Finger::index`](crate::controllers::fingers::curl::Finger::index).
#[derive(Debug, Copy, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoseShape {
    pub curl: [f32; 5],
    /// Radians toward the thumb side; the thumb's is unused.
    pub splay: [f32; 5],
    pub hook: [f32; 5],
    /// The thumb's opposition, `0..=1`.
    pub opposition: f32,
    /// The open thumb's spread, degrees, or `None` (the key left out of the tuning file) for the
    /// configured spread.
    pub thumb_spread_deg: Option<f32>,
    pub roles: [FingerRole; 5],
}

/// The part a finger plays in a pose.
#[derive(Debug, Copy, Clone, PartialEq, Deserialize)]
pub enum FingerRole {
    /// The finger takes the pose's curl, or the contact fit's where the fit found the held thing.
    Posed,
    /// The trigger finger: on the trigger it rests at `rest` and closes to `pulled` with the
    /// trigger's pull; lifted off the trigger, it keeps the measured curl, so it points.
    Trigger { rest: f32, pulled: f32 },
    /// The finger keeps the measured curl when hand tracking measures it (it is free to move on
    /// the thing, as a thumb on a wheel's rim is), and takes the pose's curl when the curl is only
    /// estimated from the inputs.
    LiveWhenTracked,
}

/// The fingers' curl round a bar of `diameter_m`: a closed fist round a rope, a firm wrap round a
/// handle, and an open hand round a thick beam. Interpolated between judged points, and monotonic:
/// a thicker bar never closes the fingers further.
pub fn cylinder_curl(diameter_m: f32) -> f32 {
    let curls = &FINGERS_TUNING.get().poses.cylinder.curls;
    let d = if diameter_m.is_finite() {
        diameter_m.max(0.0)
    } else {
        curls[2][0]
    };
    let first = curls[0];
    if d <= first[0] {
        return first[1];
    }
    for pair in curls.windows(2) {
        let ([d0, c0], [d1, c1]) = (pair[0], pair[1]);
        if d <= d1 {
            return c0 + (c1 - c0) * (d - d0) / (d1 - d0);
        }
    }
    curls[curls.len() - 1][1]
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [GripPose; 11] = [
        GripPose::PistolGrip,
        GripPose::Foregrip,
        GripPose::MagazinePinch,
        GripPose::GrenadePalm,
        GripPose::ChargePalm,
        GripPose::LedgeHook,
        GripPose::FlatPalm,
        GripPose::Wrap,
        GripPose::Cylinder { diameter_m: 0.035 },
        GripPose::DoorHandle,
        GripPose::CollarGrab,
    ];

    #[test]
    fn every_pose_stays_in_range() {
        for pose in ALL {
            let shape = pose.shape();
            for i in 0..5 {
                assert!((0.0..=1.0).contains(&shape.curl[i]), "{pose:?} curl {i}");
                assert!((-1.0..=1.0).contains(&shape.hook[i]), "{pose:?} hook {i}");
                assert!(shape.splay[i].abs() <= 0.35, "{pose:?} splay {i}");
                if let FingerRole::Trigger { rest, pulled } = shape.roles[i] {
                    assert!(rest < pulled && pulled <= 1.0, "{pose:?}");
                }
            }
            assert!((0.0..=1.0).contains(&shape.opposition), "{pose:?}");
            assert!(!pose.label().is_empty());
        }
    }

    #[test]
    fn only_the_pistol_grip_has_a_trigger_finger() {
        for pose in ALL {
            let triggers = pose
                .shape()
                .roles
                .iter()
                .filter(|r| matches!(r, FingerRole::Trigger { .. }))
                .count();
            let expected = usize::from(pose == GripPose::PistolGrip);
            assert_eq!(triggers, expected, "{pose:?}");
        }
        assert!(matches!(
            GripPose::PistolGrip.shape().roles[1],
            FingerRole::Trigger { .. }
        ));
    }

    #[test]
    fn a_thinner_bar_closes_the_fingers_further() {
        let curls = FINGERS_TUNING.get().poses.cylinder.curls;
        let mut previous = cylinder_curl(0.0);
        assert_eq!(previous, curls[0][1]);
        for step in 1..=60 {
            let curl = cylinder_curl(step as f32 * 0.003);
            assert!(curl <= previous + 1e-6, "{step}");
            previous = curl;
        }
        assert_eq!(cylinder_curl(1.0), curls[5][1]);
        assert!((cylinder_curl(curls[2][0]) - curls[2][1]).abs() < 1e-6);
        let midway = (curls[1][0] + curls[2][0]) / 2.0;
        assert!((cylinder_curl(midway) - (curls[1][1] + curls[2][1]) / 2.0).abs() < 1e-5);
        assert!(cylinder_curl(f32::NAN).is_finite());
        assert!(cylinder_curl(-1.0) <= 1.0);
    }

    #[test]
    fn a_ledge_hook_bends_at_the_outer_joints() {
        let shape = GripPose::LedgeHook.shape();
        assert!(shape.hook[1..].iter().all(|&h| h > 0.5));
        // The charge's fingers lead with the knuckles over the brick's edge instead.
        let charge = GripPose::ChargePalm.shape();
        assert!(charge.hook[1..].iter().all(|&h| h < 0.0));
    }
}
