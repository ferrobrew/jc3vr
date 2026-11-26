//! Headset-like eye rigs for the visual harness.
//!
//! The harness runs against a simulated headset whose eyes are symmetric and parallel, which leaves
//! every path that only holds for those eyes unexercised: real headsets have asymmetric per-eye
//! frusta, and some cant each display outward about the vertical axis. A `headset` visual override
//! substitutes a named rig for the located views at the point the frame loop reads them, in every
//! mode, so everything downstream (the per-eye projections and offsets, the cull and shadow-fit
//! frusta, the single-pass rows) sees a headset-like rig.
//!
//! The presets are plausible approximations of the shape of each headset's views, not vendor data.
//! Each preset's tangent extents keep the aspect of the eye target it is meant for, so pixels stay
//! square there: the harness's default 320 by 360 for most, and a square one for the
//! Bigscreen-Beyond-like rig, which a session seeds with its own eye size. The per-eye resolution is
//! the session's, never the rig's.

use glam::{EulerRot, Quat};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::vr::{
    Fov, harness,
    harness::{EyeOffset, HeadsetSubstitute},
};

/// A named headset-like rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadsetRig {
    /// Asymmetric frusta, wider towards the outside, with each display canted outward by five
    /// degrees.
    Index,
    /// Asymmetric frusta, wider towards the outside and below, with parallel displays.
    Quest,
    /// Strongly asymmetric frusta canted outward by twelve degrees, with the eyes pitched and rolled
    /// against each other by a degree, to expose anything that holds only approximately.
    Stress,
    /// A square eye image with asymmetric frusta, wider towards the outside, and parallel displays
    /// (no measured cant is available for this headset, so none is assumed).
    Beyond,
}

/// One eye's view relative to the head, as a rig substitutes it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct EyeGeometry {
    /// The field-of-view half-angles in degrees, `[left, right, up, down]`, with left and down
    /// negative as in `XrFovf`.
    pub fov_deg: [f32; 4],
    /// The eye's turn about the head's vertical axis in degrees; positive turns it to the left.
    pub yaw_deg: f32,
    /// The eye's turn about its horizontal axis in degrees; positive turns it up.
    pub pitch_deg: f32,
    /// The eye's turn about its view axis in degrees.
    pub roll_deg: f32,
}

/// A rig's full geometry, recorded in the capture metadata as the rig the views were substituted
/// with.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct HeadsetGeometry {
    pub rig: HeadsetRig,
    /// The distance between the eyes, in metres, along the head's horizontal axis.
    pub ipd_m: f32,
    /// The left eye, then the right.
    pub eyes: [EyeGeometry; 2],
}

impl HeadsetRig {
    /// The rig's geometry.
    pub(crate) fn geometry(self) -> HeadsetGeometry {
        match self {
            Self::Index => mirrored(self, 0.064, [-54.0, 51.0, 55.8, -55.8], 5.0, 0.0, 0.0),
            Self::Quest => mirrored(self, 0.063, [-50.0, 43.0, 48.0, -52.0], 0.0, 0.0, 0.0),
            Self::Beyond => mirrored(self, 0.063, [-51.0, 45.0, 48.2, -48.2], 0.0, 0.0, 0.0),
            Self::Stress => {
                let mut geometry =
                    mirrored(self, 0.070, [-60.0, 35.0, 49.0, -58.0], 12.0, 1.0, 1.0);
                geometry.eyes[1].pitch_deg = -geometry.eyes[0].pitch_deg;
                geometry
            }
        }
    }
}

impl HeadsetGeometry {
    /// The rig as the frame loop substitutes it for the located views.
    pub(crate) fn view_substitute(&self) -> HeadsetSubstitute {
        HeadsetSubstitute {
            ipd_m: self.ipd_m,
            eyes: self.eyes.map(|geometry| {
                let [left, right, up, down] = geometry.fov_deg.map(f32::to_radians);
                EyeOffset {
                    orientation: Quat::from_euler(
                        EulerRot::YXZ,
                        geometry.yaw_deg.to_radians(),
                        geometry.pitch_deg.to_radians(),
                        geometry.roll_deg.to_radians(),
                    ),
                    fov: Fov {
                        left,
                        right,
                        up,
                        down,
                    },
                }
            }),
        }
    }
}

/// The rig in force, recorded in the capture metadata, or `None` when the views are left as located.
pub(crate) fn active() -> Option<HeadsetGeometry> {
    ACTIVE.lock().map(HeadsetRig::geometry)
}

/// Set the rig the frame loop substitutes, for the configuration that names it.
pub(super) fn set_active(rig: Option<HeadsetRig>) {
    *ACTIVE.lock() = rig;
    harness::set_headset_substitute(rig.map(|rig| rig.geometry().view_substitute()));
}

/// The rig in force, as the metadata records it.
static ACTIVE: Mutex<Option<HeadsetRig>> = Mutex::new(None);

/// A rig whose right eye is the mirror image of its left: the field of view swapped side for side,
/// and the yaw and roll negated.
fn mirrored(
    rig: HeadsetRig,
    ipd_m: f32,
    [left, right, up, down]: [f32; 4],
    yaw_deg: f32,
    pitch_deg: f32,
    roll_deg: f32,
) -> HeadsetGeometry {
    HeadsetGeometry {
        rig,
        ipd_m,
        eyes: [
            EyeGeometry {
                fov_deg: [left, right, up, down],
                yaw_deg,
                pitch_deg,
                roll_deg,
            },
            EyeGeometry {
                fov_deg: [-right, -left, up, down],
                yaw_deg: -yaw_deg,
                pitch_deg,
                roll_deg: -roll_deg,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use pretty_assertions::assert_eq;
    use test_case::test_case;

    use super::*;
    use crate::vr::{OffAxisProjection, harness::EyeSubstitute};

    const RIGS: [HeadsetRig; 4] = [
        HeadsetRig::Index,
        HeadsetRig::Quest,
        HeadsetRig::Stress,
        HeadsetRig::Beyond,
    ];

    fn tangents(eye: &EyeGeometry) -> [f32; 4] {
        eye.fov_deg.map(|angle| angle.to_radians().tan())
    }

    #[test]
    fn rigs_parse_by_name() {
        for (name, rig) in [
            ("index", HeadsetRig::Index),
            ("quest", HeadsetRig::Quest),
            ("stress", HeadsetRig::Stress),
            ("beyond", HeadsetRig::Beyond),
        ] {
            assert_eq!(
                serde_json::from_value::<HeadsetRig>(serde_json::json!(name)).unwrap(),
                rig
            );
        }
        assert!(serde_json::from_value::<HeadsetRig>(serde_json::json!("vive")).is_err());
    }

    /// Every preset keeps its eye target's aspect, so a substituted rig renders square pixels.
    #[test_case(HeadsetRig::Index, 320.0 / 360.0)]
    #[test_case(HeadsetRig::Quest, 320.0 / 360.0)]
    #[test_case(HeadsetRig::Stress, 320.0 / 360.0)]
    #[test_case(HeadsetRig::Beyond, 1.0)]
    fn presets_match_the_eye_target_aspect(rig: HeadsetRig, target: f32) {
        for eye in rig.geometry().eyes {
            let [left, right, up, down] = tangents(&eye);
            let aspect = (right - left) / (up - down);
            assert!((aspect - target).abs() < 0.01, "{rig:?}: aspect {aspect}");
        }
    }

    /// Each preset is asymmetric: the eye's frustum is off centre horizontally, wider outward.
    #[test]
    fn presets_are_asymmetric_and_wider_outward() {
        for rig in RIGS {
            let [left_eye, right_eye] = rig.geometry().eyes;
            let [l, r, ..] = tangents(&left_eye);
            assert!(-l > r, "{rig:?}: the left eye is wider to its left");
            let [l, r, ..] = tangents(&right_eye);
            assert!(r > -l, "{rig:?}: the right eye is wider to its right");
        }
    }

    #[test]
    fn eyes_sit_half_the_ipd_either_side_of_the_head() {
        let head_position = Vec3::new(1.0, 1.7, -2.0);
        let head_orientation = Quat::from_rotation_y(0.7);
        for rig in RIGS {
            let geometry = rig.geometry();
            let [left, right] = geometry
                .view_substitute()
                .substitute(head_position, head_orientation);
            assert!(((right.position - left.position).length() - geometry.ipd_m).abs() < 1e-6);
            assert!((0.5 * (left.position + right.position) - head_position).length() < 1e-6);
            let local = head_orientation.inverse() * (right.position - left.position);
            assert!(local.x > 0.0 && local.y.abs() < 1e-6 && local.z.abs() < 1e-6);
        }
    }

    /// The Index-like rig turns each eye outward: the left eye's forward axis points left of the
    /// head's, the right eye's right of it, by the cant.
    #[test]
    fn the_cant_turns_each_eye_outward() {
        let [left, right] = HeadsetRig::Index
            .geometry()
            .view_substitute()
            .substitute(Vec3::ZERO, Quat::IDENTITY);
        let forward = |orientation: Quat| orientation * Vec3::NEG_Z;
        let (l, r) = (forward(left.orientation), forward(right.orientation));
        assert!(l.x < 0.0 && r.x > 0.0);
        assert!((l.x.asin().to_degrees() + 5.0).abs() < 1e-3);
        assert!((r.x.asin().to_degrees() - 5.0).abs() < 1e-3);
        let [left, right] = HeadsetRig::Quest
            .geometry()
            .view_substitute()
            .substitute(Vec3::ZERO, Quat::IDENTITY);
        assert_eq!(left.orientation, Quat::IDENTITY);
        assert_eq!(right.orientation, Quat::IDENTITY);
    }

    /// The stress rig pitches the eyes against each other, so their forward axes are not coplanar
    /// with the head's horizontal axis.
    #[test]
    fn the_stress_rig_pitches_the_eyes_apart() {
        let [left, right] = HeadsetRig::Stress
            .geometry()
            .view_substitute()
            .substitute(Vec3::ZERO, Quat::IDENTITY);
        let up = |orientation: Quat| (orientation * Vec3::NEG_Z).y;
        assert!(up(left.orientation) > 0.0 && up(right.orientation) < 0.0);
        assert!((up(left.orientation) + up(right.orientation)).abs() < 1e-6);
    }

    /// The substituted field of view reaches the projection the camera is given: the off-axis terms
    /// carry the asymmetry, and the right eye's is the left eye's mirrored.
    #[test]
    fn the_substituted_projection_is_off_axis_and_mirrored() {
        for rig in RIGS {
            let [left, right] = rig
                .geometry()
                .view_substitute()
                .substitute(Vec3::ZERO, Quat::IDENTITY);
            let p =
                |eye: &EyeSubstitute| OffAxisProjection::new(eye.fov, 0.1, 1000.0).standard_depth;
            let (pl, pr) = (p(&left), p(&right));
            // Row 2, column 0 holds `(tl + tr) / (tr - tl)`: negative for a frustum wider to the left.
            assert!(pl.data[8] < 0.0 && pr.data[8] > 0.0, "{rig:?}");
            assert!((pl.data[8] + pr.data[8]).abs() < 1e-6);
            assert!((pl.data[0] - pr.data[0]).abs() < 1e-6);
            let expected = 2.0 / (left.fov.right.tan() - left.fov.left.tan());
            assert!((pl.data[0] - expected).abs() < 1e-5);
        }
    }

    #[test]
    fn the_rig_is_recorded_by_name_with_its_geometry() {
        let value = serde_json::to_value(HeadsetRig::Quest.geometry()).unwrap();
        assert_eq!(value["rig"], serde_json::json!("quest"));
        assert_eq!(
            value["eyes"][1]["fov_deg"],
            serde_json::json!([-43.0, 50.0, 48.0, -52.0])
        );
        assert!((value["ipd_m"].as_f64().unwrap() - 0.063).abs() < 1e-6);
    }
}
