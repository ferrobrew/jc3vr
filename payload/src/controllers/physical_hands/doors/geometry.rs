//! A vehicle door's shape, pure: where its box is, whether a palm reaches it and its handle area,
//! where its handle is, which way is out, and how far a gripping hand has pulled it out.
//!
//! A door is a part of the vehicle whose mesh is authored in its own space, whose origin is taken
//! to be its hinge pivot (`docs/engine/gameplay/vehicle-parts.md`). Its box is the model's box in
//! that space, carried into the world by the part's root-space transform and the vehicle's world
//! transform. The handle area is the part of the box away from the hinge: a point's distance from
//! the pivot, as a share of the way from the box's nearest point to the pivot to its farthest
//! corner.
//!
//! The handle itself ([`DoorBox::handle`]) is found from the box alone, so that it does not rest on
//! the pivot being the part's origin: the hinge runs along one edge of the box, and the handle is
//! on the outer face at the opposite edge, halfway along it (at mid-height for a side door). Which
//! edge is the hinge's is read from the pivot when the pivot lies near one, and otherwise from the
//! kind of door: a side door hinges at its front, a hood at its back, and a trunk or a rear door at
//! its top.

use glam::{Mat4, Vec3};

/// A door's box in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DoorBox {
    /// The door's own space to the world: the vehicle's world transform times the part's
    /// root-space transform.
    pub to_world: Mat4,
    /// The door's own space to the vehicle's: the part's root-space transform.
    pub to_vehicle: Mat4,
    /// The model's box in the door's own space.
    pub min: Vec3,
    pub max: Vec3,
}

/// How a palm reaches a door.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DoorReach {
    /// The point of the door's box nearest the palm, world space.
    pub point: Vec3,
    /// How far the palm is from the box, metres; zero inside it.
    pub distance: f32,
    /// Where the point lies between the hinge and the door's far edge, `0..=1`.
    pub handle: f32,
}

impl DoorBox {
    /// The box of a part whose root-space transform is `root`, on a vehicle at `vehicle`, with the
    /// model box `min`..`max`. `None` for a box or transform that is not finite or is degenerate.
    pub fn new(vehicle: Mat4, root: Mat4, min: Vec3, max: Vec3) -> Option<Self> {
        let to_world = vehicle * root;
        let valid = to_world.is_finite()
            && to_world.determinant().abs() > 1e-9
            && min.is_finite()
            && max.is_finite()
            && (max - min).min_element() >= 0.0;
        valid.then_some(Self {
            to_world,
            to_vehicle: root,
            min,
            max,
        })
    }

    /// Where the door's handle is, world space, and which way its outer face looks, for a door of
    /// `kind`: on the outer face, at the edge across the box from the hinge's, halfway along it,
    /// and a little in from the edge. `None` for a box with no extent to find the faces by.
    pub fn handle(&self, kind: DoorKind) -> Option<Handle> {
        let size = self.max - self.min;
        if size.min_element() < 0.0 || size.max_element() <= 1e-4 {
            return None;
        }
        let centre = (self.min + self.max) * 0.5;
        // The vehicle's axes in the door's own space.
        let from_vehicle = self.to_vehicle.inverse();
        let up = from_vehicle.transform_vector3(Vec3::Y).normalize_or_zero();
        let side = from_vehicle.transform_vector3(Vec3::X).normalize_or_zero();
        // Out of the vehicle: from the vehicle's middle toward the door's.
        let out = (centre - from_vehicle.transform_point3(Vec3::ZERO)).normalize_or_zero();
        if up == Vec3::ZERO || side == Vec3::ZERO || out == Vec3::ZERO {
            return None;
        }
        // A door is a panel: its thinnest extent is through it.
        let thickness = (0..3)
            .min_by(|&a, &b| size[a].total_cmp(&size[b]))
            .unwrap_or(0);
        let (along, across) = match kind {
            // A side door hinges on an upright edge: it runs along the length, its handle at
            // mid-height.
            DoorKind::Side => {
                let height = axis_along(up, &[thickness]);
                (third_axis(thickness, height), height)
            }
            // A lid hinges on an edge across the vehicle: it runs along the vehicle, its handle
            // midway across.
            DoorKind::Front | DoorKind::Rear => {
                let width = axis_along(side, &[thickness]);
                (third_axis(thickness, width), width)
            }
        };
        // The hinge's end of the door's run: the end the pivot is near, or the kind's own.
        let length = size[along];
        let pivot = (0.0 - self.min[along]) / length.max(1e-4);
        let hinge_at_min = if pivot <= PIVOT_NEAR_END {
            true
        } else if pivot >= 1.0 - PIVOT_NEAR_END {
            false
        } else {
            let toward = match kind {
                // The vehicle's front is its -Z.
                DoorKind::Side => from_vehicle.transform_vector3(Vec3::NEG_Z),
                DoorKind::Front => from_vehicle.transform_vector3(Vec3::Z),
                // At the top of an upright hatch, at the front of a lid lying flat.
                DoorKind::Rear => up + from_vehicle.transform_vector3(Vec3::NEG_Z),
            };
            toward[along] < 0.0
        };
        let mut local = centre;
        local[along] = if hinge_at_min {
            self.max[along] - length * HANDLE_INSET
        } else {
            self.min[along] + length * HANDLE_INSET
        };
        local[across] = centre[across];
        let outward_sign = if out[thickness] >= 0.0 { 1.0 } else { -1.0 };
        local[thickness] = if outward_sign > 0.0 {
            self.max[thickness]
        } else {
            self.min[thickness]
        };
        let mut normal_local = Vec3::ZERO;
        normal_local[thickness] = outward_sign;
        let point = self.to_world.transform_point3(local);
        let normal = self
            .to_world
            .transform_vector3(normal_local)
            .normalize_or_zero();
        (point.is_finite() && normal != Vec3::ZERO).then_some(Handle {
            point,
            normal,
            hinge_from_pivot: pivot <= PIVOT_NEAR_END || pivot >= 1.0 - PIVOT_NEAR_END,
        })
    }

    /// The box's centre, world space.
    pub fn centre(&self) -> Vec3 {
        self.to_world.transform_point3((self.min + self.max) * 0.5)
    }

    /// How `palm` (world space) reaches the box, or `None` when it is farther than `reach_m`.
    pub fn reach(&self, palm: Vec3, reach_m: f32) -> Option<DoorReach> {
        let local = self.to_world.inverse().transform_point3(palm);
        if !local.is_finite() {
            return None;
        }
        let nearest = local.clamp(self.min, self.max);
        let point = self.to_world.transform_point3(nearest);
        let distance = point.distance(palm);
        (distance <= reach_m.max(0.0)).then(|| DoorReach {
            point,
            distance,
            handle: self.handle_share(nearest),
        })
    }

    /// Where the door-space point `local` lies between the hinge pivot and the box's far corner,
    /// `0..=1`.
    fn handle_share(&self, local: Vec3) -> f32 {
        let near = Vec3::ZERO.clamp(self.min, self.max).length();
        let far = [
            Vec3::new(self.min.x, self.min.y, self.min.z),
            Vec3::new(self.max.x, self.min.y, self.min.z),
            Vec3::new(self.min.x, self.max.y, self.min.z),
            Vec3::new(self.max.x, self.max.y, self.min.z),
            Vec3::new(self.min.x, self.min.y, self.max.z),
            Vec3::new(self.max.x, self.min.y, self.max.z),
            Vec3::new(self.min.x, self.max.y, self.max.z),
            Vec3::new(self.max.x, self.max.y, self.max.z),
        ]
        .iter()
        .map(|c| c.length())
        .fold(0.0, f32::max);
        if far - near <= 1e-4 {
            return 1.0;
        }
        ((local.length() - near) / (far - near)).clamp(0.0, 1.0)
    }

    /// Which way is out of the vehicle at this door, world space: from the vehicle's origin
    /// `vehicle_origin` toward the box's centre. A side door's is to the side, a hood's forward and
    /// up, a trunk's back.
    pub fn outward(&self, vehicle_origin: Vec3) -> Vec3 {
        (self.centre() - vehicle_origin).normalize_or_zero()
    }
}

/// Which way a door opens, by its slot, for finding its hinge.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DoorKind {
    /// A side door, hinged on an upright edge at its front.
    Side,
    /// A hood, hinged across the vehicle at its back.
    Front,
    /// A trunk, a rear door, or a hatch, hinged across the vehicle at its top.
    Rear,
}

/// A door's handle.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Handle {
    /// The handle's place on the outer face, world space.
    pub point: Vec3,
    /// The outer face's normal, world space.
    pub normal: Vec3,
    /// Whether the hinge's edge was read from the part's pivot, rather than from the door's kind.
    pub hinge_from_pivot: bool,
}

/// The share of the door's run within which the pivot counts as at one end, where the hinge is.
const PIVOT_NEAR_END: f32 = 0.25;

/// How far in from the edge across from the hinge the handle sits, as a share of the door's run.
const HANDLE_INSET: f32 = 0.1;

/// The box axis (0, 1, or 2) most nearly along `direction`, among those not in `taken`.
fn axis_along(direction: Vec3, taken: &[usize]) -> usize {
    (0..3)
        .filter(|a| !taken.contains(a))
        .max_by(|&a, &b| direction[a].abs().total_cmp(&direction[b].abs()))
        .unwrap_or(0)
}

/// The box axis that is neither `a` nor `b`.
fn third_axis(a: usize, b: usize) -> usize {
    (0..3).find(|x| *x != a && *x != b).unwrap_or(0)
}

/// How far a hand gripping a latched door at `grab` has pulled outward along `outward` with its
/// palm at `palm`, world space; negative for a push inward.
pub fn pull(grab: Vec3, palm: Vec3, outward: Vec3) -> f32 {
    let pulled = (palm - grab).dot(outward);
    if pulled.is_finite() { pulled } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    /// A door hinged at its front edge on the vehicle's left side: it runs back along +Z from the
    /// pivot, a metre long, a metre tall, and five centimetres thick.
    fn left_door() -> DoorBox {
        let root = Mat4::from_translation(Vec3::new(-0.9, 0.5, -0.8));
        DoorBox::new(
            Mat4::IDENTITY,
            root,
            Vec3::new(-0.05, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 1.0),
        )
        .unwrap()
    }

    #[test]
    fn a_palm_beside_the_door_reaches_it() {
        let door = left_door();
        let palm = Vec3::new(-1.0, 1.0, 0.0);
        let reach = door.reach(palm, 0.1).unwrap();
        assert!((reach.distance - 0.05).abs() < 1e-5);
        assert!(reach.point.abs_diff_eq(Vec3::new(-0.95, 1.0, 0.0), 1e-5));
        assert!(door.reach(Vec3::new(-1.2, 1.0, 0.0), 0.1).is_none());
    }

    #[test]
    fn the_handle_area_is_away_from_the_hinge() {
        let door = left_door();
        let near_hinge = door.reach(Vec3::new(-1.0, 0.6, -0.75), 0.1).unwrap();
        let far_edge = door.reach(Vec3::new(-1.0, 1.0, 0.15), 0.1).unwrap();
        assert!(near_hinge.handle < 0.2, "{near_hinge:?}");
        assert!(far_edge.handle > 0.7, "{far_edge:?}");
    }

    #[test]
    fn the_door_box_follows_the_vehicle() {
        let vehicle = Mat4::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Vec3::new(10.0, 0.0, 5.0),
        );
        let root = Mat4::from_translation(Vec3::new(-0.9, 0.5, -0.8));
        let door = DoorBox::new(
            vehicle,
            root,
            Vec3::new(-0.05, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 1.0),
        )
        .unwrap();
        let outward = door.outward(Vec3::new(10.0, 0.0, 5.0));
        // The vehicle's left side faces +Z once it has turned a quarter about +Y.
        assert!(outward.z > 0.5, "{outward:?}");
        let centre = door.centre();
        assert!(door.reach(centre + outward * 0.05, 0.1).is_some());
    }

    #[test]
    fn a_pull_is_measured_outward() {
        let grab = Vec3::new(-0.95, 1.0, 0.0);
        let out = Vec3::NEG_X;
        assert!((pull(grab, grab + Vec3::new(-0.06, 0.02, 0.01), out) - 0.06).abs() < 1e-6);
        assert!(pull(grab, grab + Vec3::X * 0.03, out) < 0.0);
        assert_eq!(pull(grab, Vec3::NAN, out), 0.0);
    }

    #[test]
    fn a_side_doors_handle_is_on_its_outer_face_across_from_the_hinge_at_mid_height() {
        // The left door above: hinged at its front (its own origin, the pivot), running back.
        let handle = left_door().handle(DoorKind::Side).unwrap();
        assert!(handle.hinge_from_pivot);
        assert!(
            handle.point.abs_diff_eq(Vec3::new(-0.95, 1.0, 0.1), 1e-5),
            "{handle:?}"
        );
        assert!(handle.normal.abs_diff_eq(Vec3::NEG_X, 1e-5));
    }

    #[test]
    fn a_door_whose_pivot_is_not_at_an_edge_hinges_at_its_front() {
        // The same door with its origin in the middle of the box: the pivot says nothing, and a side
        // door hinges at its front (the vehicle's -Z), so the handle is at the back.
        let root = Mat4::from_translation(Vec3::new(-0.9, 1.0, -0.3));
        let door = DoorBox::new(
            Mat4::IDENTITY,
            root,
            Vec3::new(-0.05, -0.5, -0.5),
            Vec3::new(0.0, 0.5, 0.5),
        )
        .unwrap();
        let handle = door.handle(DoorKind::Side).unwrap();
        assert!(!handle.hinge_from_pivot);
        assert!(
            handle.point.abs_diff_eq(Vec3::new(-0.95, 1.0, 0.1), 1e-5),
            "{handle:?}"
        );
    }

    #[test]
    fn a_hoods_handle_is_at_its_front_and_a_trunks_at_its_back() {
        // A hood lying ahead of the vehicle's middle, its pivot in the middle of the box.
        let hood = DoorBox::new(
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::new(0.0, 1.0, -1.5)),
            Vec3::new(-0.7, 0.0, -0.5),
            Vec3::new(0.7, 0.05, 0.5),
        )
        .unwrap();
        let handle = hood.handle(DoorKind::Front).unwrap();
        assert!(handle.point.z < -1.8, "{handle:?}");
        assert!(handle.point.x.abs() < 1e-5);
        let trunk = DoorBox::new(
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::new(0.0, 1.0, 1.8)),
            Vec3::new(-0.7, 0.0, -0.4),
            Vec3::new(0.7, 0.05, 0.4),
        )
        .unwrap();
        let handle = trunk.handle(DoorKind::Rear).unwrap();
        assert!(handle.point.x.abs() < 1e-5, "{handle:?}");
        assert!(
            handle.point.z > 2.1,
            "a lid lying flat opens at its back: {handle:?}"
        );
        // An upright hatch at the back opens at its bottom.
        let hatch = DoorBox::new(
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::new(0.0, 1.2, 2.2)),
            Vec3::new(-0.6, -0.4, 0.0),
            Vec3::new(0.6, 0.4, 0.05),
        )
        .unwrap();
        let handle = hatch.handle(DoorKind::Rear).unwrap();
        assert!(handle.point.y < 0.9, "{handle:?}");
        assert!(
            handle.normal.z > 0.9,
            "its outer face looks back: {handle:?}"
        );
    }

    #[test]
    fn a_degenerate_box_is_none() {
        assert!(DoorBox::new(Mat4::ZERO, Mat4::IDENTITY, Vec3::ZERO, Vec3::ONE).is_none());
        assert!(DoorBox::new(Mat4::IDENTITY, Mat4::IDENTITY, Vec3::ONE, Vec3::ZERO).is_none());
    }
}
