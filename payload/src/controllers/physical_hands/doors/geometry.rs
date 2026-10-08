//! A vehicle door's shape, pure: where its box is, whether a palm reaches it and its handle area,
//! which way is out, and how far a gripping hand has pulled it out.
//!
//! A door is a part of the vehicle whose mesh is authored in its own space, whose origin is its
//! hinge pivot (`docs/engine/gameplay/vehicle-parts.md`). Its box is the model's box in that space,
//! carried into the world by the part's root-space transform and the vehicle's world transform. The
//! handle area is the part of the box away from the hinge: a point's distance from the pivot, as a
//! share of the way from the box's nearest point to the pivot to its farthest corner.

use glam::{Mat4, Vec3};

/// A door's box in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DoorBox {
    /// The door's own space to the world: the vehicle's world transform times the part's
    /// root-space transform.
    pub to_world: Mat4,
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
        valid.then_some(Self { to_world, min, max })
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
    fn a_degenerate_box_is_none() {
        assert!(DoorBox::new(Mat4::ZERO, Mat4::IDENTITY, Vec3::ZERO, Vec3::ONE).is_none());
        assert!(DoorBox::new(Mat4::IDENTITY, Mat4::IDENTITY, Vec3::ONE, Vec3::ZERO).is_none());
    }
}
