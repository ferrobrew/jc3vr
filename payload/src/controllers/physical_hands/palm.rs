//! The palm box, pure: where a hand's palm is in the world, and the points its contacts are probed
//! from.

use glam::{Quat, Vec3};

use crate::controllers::{frame::Pose, physical_hands::config::PhysicalHandsConfig};

/// A palm box in the world: an oriented box in the wrist frame's axes.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PalmBox {
    pub centre: Vec3,
    /// The wrist frame's orientation: -Z toward the fingers, +Y out of the back of the hand.
    pub orientation: Quat,
    /// Half the box's size along its axes, world metres.
    pub half_extents: Vec3,
}

/// How many points a palm's contacts are probed from: [`PalmBox::probe_points`].
pub const PROBE_POINTS: usize = 6;

impl PalmBox {
    /// The palm box of a hand whose wrist frame is at `wrist` in the world, for a VR world scale
    /// of `world_scale` (world metres per metre of the player's hand).
    pub fn from_wrist(wrist: Pose, world_scale: f32, config: &PhysicalHandsConfig) -> Self {
        let scale = if world_scale.is_finite() && world_scale > 0.0 {
            world_scale
        } else {
            1.0
        };
        Self {
            centre: wrist.position + wrist.orientation * (config.palm_offset_m * scale),
            orientation: wrist.orientation,
            half_extents: config.palm_half_extents_m.abs() * scale,
        }
    }

    /// A point in the box's frame, `local` in units of its half extents.
    pub fn point(&self, local: Vec3) -> Vec3 {
        self.centre + self.orientation * (local * self.half_extents)
    }

    /// The points a palm's contacts are probed from: the centre, the middle of the palm's face,
    /// and the four corners of the face toward the fingers, which lead a punch and a slap's
    /// follow-through.
    pub fn probe_points(&self) -> [Vec3; PROBE_POINTS] {
        [
            self.centre,
            self.point(Vec3::new(0.0, -1.0, 0.0)),
            self.point(Vec3::new(-1.0, -1.0, -1.0)),
            self.point(Vec3::new(1.0, -1.0, -1.0)),
            self.point(Vec3::new(-1.0, 1.0, -1.0)),
            self.point(Vec3::new(1.0, 1.0, -1.0)),
        ]
    }

    /// The box's eight corners, for drawing: the four of the -Z face, then the four of the +Z face,
    /// each face in winding order.
    pub fn corners(&self) -> [Vec3; 8] {
        [
            (-1.0, -1.0, -1.0),
            (1.0, -1.0, -1.0),
            (1.0, 1.0, -1.0),
            (-1.0, 1.0, -1.0),
            (-1.0, -1.0, 1.0),
            (1.0, -1.0, 1.0),
            (1.0, 1.0, 1.0),
            (-1.0, 1.0, 1.0),
        ]
        .map(|(x, y, z)| self.point(Vec3::new(x, y, z)))
    }
}

/// The edges of a box drawn from [`PalmBox::corners`], as corner index pairs.
pub const EDGES: [(usize, usize); 12] = [
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 0),
    (4, 5),
    (5, 6),
    (6, 7),
    (7, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn wrist(position: Vec3, orientation: Quat) -> Pose {
        Pose {
            position,
            orientation,
        }
    }

    #[test]
    fn the_palm_sits_toward_the_fingers_in_the_wrist_frame() {
        let config = PhysicalHandsConfig::new();
        let palm = PalmBox::from_wrist(wrist(Vec3::ZERO, Quat::IDENTITY), 1.0, &config);
        assert_eq!(palm.centre, config.palm_offset_m);
        // Turned a quarter about +Y, the fingers (-Z) point along -X.
        let turned = PalmBox::from_wrist(
            wrist(
                Vec3::ZERO,
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            ),
            1.0,
            &config,
        );
        assert!(turned.centre.x < -0.05 && turned.centre.z.abs() < 1e-5);
    }

    #[test]
    fn the_world_scale_scales_the_box() {
        let config = PhysicalHandsConfig::new();
        let small = PalmBox::from_wrist(wrist(Vec3::ONE, Quat::IDENTITY), 1.0, &config);
        let big = PalmBox::from_wrist(wrist(Vec3::ONE, Quat::IDENTITY), 2.0, &config);
        assert!((big.half_extents - small.half_extents * 2.0).length() < 1e-6);
        assert!(((big.centre - Vec3::ONE) - (small.centre - Vec3::ONE) * 2.0).length() < 1e-6);
        // A degenerate scale falls back to the hand's own size.
        let bad = PalmBox::from_wrist(wrist(Vec3::ONE, Quat::IDENTITY), f32::NAN, &config);
        assert_eq!(bad, small);
    }

    #[test]
    fn every_probe_point_and_corner_is_on_the_box() {
        let config = PhysicalHandsConfig::new();
        let palm = PalmBox::from_wrist(
            wrist(Vec3::new(1.0, 2.0, 3.0), Quat::from_rotation_x(0.4)),
            1.0,
            &config,
        );
        let inside = |p: Vec3| {
            let local = palm.orientation.inverse() * (p - palm.centre);
            (local.abs() - palm.half_extents).max_element() < 1e-5
        };
        assert!(palm.probe_points().into_iter().all(inside));
        assert!(palm.corners().into_iter().all(inside));
        for (a, b) in EDGES {
            // Every edge joins corners that differ along exactly one axis.
            let local = palm.orientation.inverse() * (palm.corners()[a] - palm.corners()[b]);
            let axes = local.to_array().iter().filter(|v| v.abs() > 1e-5).count();
            assert_eq!(axes, 1);
        }
    }
}
