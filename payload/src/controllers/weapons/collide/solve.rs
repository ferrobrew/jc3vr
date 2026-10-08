//! A held long gun against a wall. Pure.
//!
//! The gun is a segment from its holding grip to just past its muzzle. A wall is a plane met by
//! the segment, with its normal facing the player. A segment whose tip is behind the plane while
//! its grip is in front is pushed out: first turned about the grip, away from the wall, until the
//! tip lies on the plane or the turn reaches its limit, and then pulled back along its new
//! direction until the tip is on the plane or the pull reaches its limit. The gun slides along the
//! wall as the hands push it in, and is pushed back into the hands when it meets the wall head-on.

use glam::{Quat, Vec3};

use crate::controllers::weapons::ads::solve::RigidMove;

/// The gun as a segment, in one frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Barrel {
    pub grip: Vec3,
    /// A unit vector toward the muzzle.
    pub direction: Vec3,
    /// From the grip to the tip, metres.
    pub length: f32,
}

/// A wall the barrel met, in the barrel's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Wall {
    pub point: Vec3,
    /// Facing the side the grip is on.
    pub normal: Vec3,
}

/// The push out of a wall.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Push {
    pub moved: RigidMove,
    pub turn_deg: f32,
    pub retract_m: f32,
}

/// The push that takes `barrel` out of `wall`, turning at most `max_turn_deg` and pulling back at
/// most `max_retract_m`, or `None` when the barrel is not in the wall (or its grip is through it).
pub fn solve(barrel: Barrel, wall: Wall, max_turn_deg: f32, max_retract_m: f32) -> Option<Push> {
    let n = wall.normal.try_normalize()?;
    let d = barrel.direction.try_normalize()?;
    let length = barrel.length;
    if !(length > 0.0 && barrel.grip.is_finite() && wall.point.is_finite()) {
        return None;
    }
    let grip_side = (barrel.grip - wall.point).dot(n);
    let tip_side = (barrel.grip + d * length - wall.point).dot(n);
    if tip_side >= 0.0 || grip_side <= 0.0 {
        return None;
    }
    // The barrel's angle from the normal now, and where its tip would just touch the plane.
    let now = d.dot(n).clamp(-1.0, 1.0).acos();
    let touching = (-grip_side / length).clamp(-1.0, 1.0).acos();
    let axis = d.cross(n).try_normalize();
    let turn = match axis {
        Some(_) => (now - touching).clamp(0.0, max_turn_deg.max(0.0).to_radians()),
        None => 0.0,
    };
    let rotation = axis.map_or(Quat::IDENTITY, |axis| Quat::from_axis_angle(axis, turn));
    let turned = rotation * d;
    let tip = barrel.grip + turned * length;
    let depth = -(tip - wall.point).dot(n);
    let into = -turned.dot(n);
    let retract = if depth > 0.0 && into > 1e-4 {
        (depth / into).min(max_retract_m.max(0.0))
    } else {
        0.0
    };
    let moved = RigidMove::about(barrel.grip, rotation).then(RigidMove {
        rotation: Quat::IDENTITY,
        translation: -turned * retract,
    });
    Some(Push {
        moved,
        turn_deg: turn.to_degrees(),
        retract_m: retract,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A barrel 0.8 m long from the origin along -Z, and a wall facing +Z.
    fn barrel(direction: Vec3) -> Barrel {
        Barrel {
            grip: Vec3::ZERO,
            direction: direction.normalize(),
            length: 0.8,
        }
    }

    fn wall(z: f32) -> Wall {
        Wall {
            point: Vec3::new(0.0, 0.0, z),
            normal: Vec3::Z,
        }
    }

    fn tip_side(b: Barrel, push: &Push, w: Wall) -> f32 {
        let tip = push.moved.point(b.grip + b.direction * b.length);
        (tip - w.point).dot(w.normal)
    }

    #[test]
    fn a_barrel_short_of_the_wall_is_left_alone() {
        assert!(solve(barrel(Vec3::NEG_Z), wall(-1.0), 70.0, 0.35).is_none());
    }

    #[test]
    fn a_slanted_barrel_slides_along_the_wall() {
        let b = barrel(Vec3::new(1.0, 0.0, -1.0));
        let w = wall(-0.4);
        let push = solve(b, w, 70.0, 0.35).unwrap();
        assert!(push.turn_deg > 0.0);
        assert!(push.retract_m < 1e-4);
        assert!(tip_side(b, &push, w).abs() < 1e-4);
        // The grip stays.
        assert!(push.moved.point(b.grip).length() < 1e-6);
    }

    #[test]
    fn a_barrel_straight_into_the_wall_is_pulled_back() {
        let b = barrel(Vec3::NEG_Z);
        let w = wall(-0.6);
        let push = solve(b, w, 70.0, 0.35).unwrap();
        assert_eq!(push.turn_deg, 0.0);
        assert!((push.retract_m - 0.2).abs() < 1e-4);
        assert!(tip_side(b, &push, w).abs() < 1e-4);
    }

    #[test]
    fn a_turn_past_its_limit_pulls_back_the_rest() {
        let b = barrel(Vec3::new(0.2, 0.0, -1.0));
        let w = wall(-0.3);
        let push = solve(b, w, 20.0, 1.0).unwrap();
        assert!((push.turn_deg - 20.0).abs() < 1e-3);
        assert!(push.retract_m > 0.0);
        assert!(tip_side(b, &push, w).abs() < 1e-4);
    }

    #[test]
    fn the_pull_back_stops_at_its_limit() {
        let push = solve(barrel(Vec3::NEG_Z), wall(-0.1), 0.0, 0.35).unwrap();
        assert_eq!(push.retract_m, 0.35);
    }

    #[test]
    fn a_grip_through_the_wall_is_not_fought() {
        assert!(solve(barrel(Vec3::NEG_Z), wall(0.1), 70.0, 0.35).is_none());
    }
}
