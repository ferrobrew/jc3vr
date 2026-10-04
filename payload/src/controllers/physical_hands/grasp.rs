//! Holding a loose object, pure: which reachable object a hand would take, where a held body
//! should be, the velocities that drive it there, whether the hands holding it can lift it, and
//! the velocity it leaves the hand with.
//!
//! A held body stays dynamic and is driven by its velocity: each tick it is given the velocity that
//! carries it toward where the hand holds it ([`drive`]), so that it still collides, is blocked by
//! walls, and needs nothing restored when it is let go.

use std::num::NonZeroUsize;

use glam::{Quat, Vec3};

use crate::controllers::{frame::Pose, physical_hands::config::PickupConfig};

/// What a reachable object is to a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TargetKind {
    /// A loose rigid object, held in the hand, of this mass in kilograms.
    Prop { mass_kg: f32 },
    /// A weapon lying in the world, collected through the game's own pickup.
    Weapon,
}

/// An object within a hand's reach.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Reach {
    /// The object's address.
    pub object: NonZeroUsize,
    /// The reached body's Havok id.
    pub body: u32,
    /// Where the hand reached the object, world space.
    pub point: Vec3,
    /// How far the point is from the palm, metres.
    pub distance: f32,
    pub kind: TargetKind,
}

/// The nearest of `reaches`.
pub fn nearest(reaches: impl IntoIterator<Item = Reach>) -> Option<Reach> {
    reaches
        .into_iter()
        .filter(|r| r.distance.is_finite())
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

/// A body's place: its centre of mass and its orientation, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyPose {
    pub centre: Vec3,
    pub orientation: Quat,
}

/// Where a hand holds a body: the body's place in the palm's frame, fixed at the grab.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Grip {
    offset: Vec3,
    rotation: Quat,
}

impl Grip {
    /// The grip of a palm at `palm` on a body at `body`.
    pub fn of(palm: Pose, body: BodyPose) -> Self {
        let inverse = palm.orientation.inverse();
        Self {
            offset: inverse * (body.centre - palm.position),
            rotation: (inverse * body.orientation).normalize(),
        }
    }

    /// Where the body should be with the palm at `palm`.
    pub fn target(&self, palm: Pose) -> BodyPose {
        BodyPose {
            centre: palm.position + palm.orientation * self.offset,
            orientation: (palm.orientation * self.rotation).normalize(),
        }
    }
}

/// The place between two hands' targets for one body: the midpoint, and the halfway rotation.
pub fn between(a: BodyPose, b: BodyPose) -> BodyPose {
    // The two quaternions may name one rotation with opposite signs; blend along the short way.
    let b_orientation = if a.orientation.dot(b.orientation) < 0.0 {
        -b.orientation
    } else {
        b.orientation
    };
    BodyPose {
        centre: (a.centre + b.centre) * 0.5,
        orientation: a.orientation.slerp(b_orientation, 0.5).normalize(),
    }
}

/// The follow's tuning for one tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Follow {
    /// The speed per metre the body is off its target, per second.
    pub stiffness_per_s: f32,
    pub max_speed_mps: f32,
    /// The turn rate per radian the body is off its target orientation, per second.
    pub angular_stiffness_per_s: f32,
    pub max_angular_speed_rps: f32,
    /// The share of the way from the body's current velocities to the driven ones taken this tick,
    /// `0..=1`.
    pub response: f32,
}

impl Follow {
    /// The follow for a body of `mass_kg`, over a tick of `dt` seconds: a heavier body takes
    /// longer to come up to the driven velocities.
    pub fn for_mass(mass_kg: f32, dt: f32, config: &PickupConfig) -> Self {
        let share = if config.one_hand_mass_kg > 0.0 {
            (mass_kg / config.one_hand_mass_kg).max(0.0)
        } else {
            1.0
        };
        let time_constant = config.heavy_response_s.max(0.0) * share;
        let response = if time_constant > 0.0 && dt.is_finite() && dt > 0.0 {
            1.0 - (-dt / time_constant).exp()
        } else {
            1.0
        };
        Self {
            stiffness_per_s: config.stiffness_per_s.max(0.0),
            max_speed_mps: config.max_speed_mps.max(0.0),
            angular_stiffness_per_s: config.angular_stiffness_per_s.max(0.0),
            max_angular_speed_rps: config.max_angular_speed_rps.max(0.0),
            response: response.clamp(0.0, 1.0),
        }
    }
}

/// A body's velocities, world space: linear in metres per second, angular in radians per second.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Velocities {
    pub linear: Vec3,
    pub angular: Vec3,
}

/// The velocities that carry a body at `body`, moving at `current`, toward `target`, which moves
/// at `target_velocity`.
pub fn drive(
    target: BodyPose,
    target_velocity: Vec3,
    body: BodyPose,
    current: Velocities,
    follow: &Follow,
) -> Velocities {
    let target_velocity = if target_velocity.is_finite() {
        target_velocity
    } else {
        Vec3::ZERO
    };
    let linear = (target_velocity + (target.centre - body.centre) * follow.stiffness_per_s)
        .clamp_length_max(follow.max_speed_mps);
    let mut turn = (target.orientation * body.orientation.inverse()).normalize();
    if turn.w < 0.0 {
        turn = -turn;
    }
    let (axis, angle) = turn.to_axis_angle();
    let angular = if angle.is_finite() && axis.is_finite() {
        (axis * angle * follow.angular_stiffness_per_s)
            .clamp_length_max(follow.max_angular_speed_rps)
    } else {
        Vec3::ZERO
    };
    let blend = |from: Vec3, to: Vec3| {
        if from.is_finite() {
            from.lerp(to, follow.response)
        } else {
            to
        }
    };
    Velocities {
        linear: blend(current.linear, linear),
        angular: blend(current.angular, angular),
    }
}

/// Whether the body has been torn from the hand: it lies farther than `break_distance_m` from its
/// target, as when it is caught on something the hand moved past.
pub fn torn_away(target: BodyPose, body: BodyPose, break_distance_m: f32) -> bool {
    let distance = target.centre.distance(body.centre);
    !distance.is_finite() || distance > break_distance_m.max(0.0)
}

/// Whether `hands` hands can lift a body of `mass_kg`.
pub fn can_lift(mass_kg: f32, hands: usize, config: &PickupConfig) -> bool {
    let limit = match hands {
        0 => return false,
        1 => config.one_hand_mass_kg,
        _ => config.two_hand_mass_kg.max(config.one_hand_mass_kg),
    };
    mass_kg.is_finite() && mass_kg > 0.0 && mass_kg <= limit
}

/// The mass of a body whose inverse mass is `inverse_mass`, or `None` for an infinite mass.
pub fn mass_of(inverse_mass: f32) -> Option<f32> {
    (inverse_mass.is_finite() && inverse_mass > 0.0).then(|| 1.0 / inverse_mass)
}

/// The velocity a body leaves the hand with: the hand's own, scaled and capped.
pub fn throw_velocity(hand_velocity: Vec3, config: &PickupConfig) -> Vec3 {
    if !hand_velocity.is_finite() {
        return Vec3::ZERO;
    }
    (hand_velocity * config.throw_scale.max(0.0)).clamp_length_max(config.max_throw_speed_mps)
}

/// A position's velocity, smoothed across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct VelocityTracker {
    last: Option<Vec3>,
    velocity: Vec3,
}

impl VelocityTracker {
    pub const fn new() -> Self {
        Self {
            last: None,
            velocity: Vec3::ZERO,
        }
    }

    /// Forget the motion: after a teleport, or when the hand stops being tracked.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Take the position `at` after `dt` seconds, smoothing over `time_constant_s`, and return the
    /// velocity.
    pub fn update(&mut self, at: Vec3, dt: f32, time_constant_s: f32) -> Vec3 {
        if !at.is_finite() {
            return self.velocity;
        }
        if let Some(last) = self.last
            && dt.is_finite()
            && dt > 0.0
        {
            let raw = (at - last) / dt;
            let share = if time_constant_s > 0.0 {
                1.0 - (-dt / time_constant_s).exp()
            } else {
                1.0
            };
            if raw.is_finite() {
                self.velocity = self.velocity.lerp(raw, share);
            }
        }
        self.last = Some(at);
        self.velocity
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    fn pose(position: Vec3, orientation: Quat) -> Pose {
        Pose {
            position,
            orientation,
        }
    }

    fn body(centre: Vec3, orientation: Quat) -> BodyPose {
        BodyPose {
            centre,
            orientation,
        }
    }

    fn reach(distance: f32, kind: TargetKind) -> Reach {
        Reach {
            object: NonZeroUsize::new(0x1000).unwrap(),
            body: 1,
            point: Vec3::ZERO,
            distance,
            kind,
        }
    }

    #[test]
    fn the_nearest_reach_wins() {
        let near = reach(0.03, TargetKind::Weapon);
        let far = reach(0.08, TargetKind::Prop { mass_kg: 2.0 });
        assert_eq!(nearest([far, near]), Some(near));
        assert_eq!(nearest([reach(f32::NAN, TargetKind::Weapon)]), None);
        assert_eq!(nearest([]), None);
    }

    #[test]
    fn a_grip_keeps_the_body_where_the_hand_took_it() {
        let palm = pose(Vec3::new(1.0, 1.0, 0.0), Quat::from_rotation_y(0.3));
        let held = body(Vec3::new(1.1, 0.9, -0.2), Quat::from_rotation_x(0.5));
        let grip = Grip::of(palm, held);
        let same = grip.target(palm);
        assert!(same.centre.distance(held.centre) < 1e-5);
        assert!(same.orientation.angle_between(held.orientation) < 1e-3);

        // Turning the hand a quarter about +Y swings the body around with it.
        let turned = pose(palm.position, Quat::from_rotation_y(0.3 + FRAC_PI_2));
        let moved = grip.target(turned);
        let before = held.centre - palm.position;
        let after = moved.centre - palm.position;
        assert!((before.length() - after.length()).abs() < 1e-5);
        assert!((before.y - after.y).abs() < 1e-5);
        assert!((before.x * after.x + before.z * after.z).abs() < 1e-4);
        assert!(
            (moved.orientation.angle_between(held.orientation) - FRAC_PI_2).abs() < 1e-3,
            "the body turns with the hand"
        );
    }

    #[test]
    fn two_hands_hold_the_body_between_them() {
        let a = body(Vec3::new(-0.2, 1.0, 0.0), Quat::IDENTITY);
        let b = body(Vec3::new(0.2, 1.2, 0.0), -Quat::from_rotation_z(0.4));
        let mid = between(a, b);
        assert!(mid.centre.distance(Vec3::new(0.0, 1.1, 0.0)) < 1e-5);
        // The sign flip of `b` does not send the blend the long way round.
        assert!((mid.orientation.angle_between(Quat::IDENTITY) - 0.2).abs() < 1e-3);
    }

    fn follow() -> Follow {
        Follow {
            stiffness_per_s: 10.0,
            max_speed_mps: 5.0,
            angular_stiffness_per_s: 8.0,
            max_angular_speed_rps: 10.0,
            response: 1.0,
        }
    }

    #[test]
    fn the_drive_closes_on_the_target_and_carries_its_motion() {
        let target = body(Vec3::new(0.0, 1.0, 0.0), Quat::IDENTITY);
        let at = body(Vec3::new(0.0, 0.9, 0.0), Quat::IDENTITY);
        let v = drive(
            target,
            Vec3::new(1.0, 0.0, 0.0),
            at,
            Velocities::default(),
            &follow(),
        );
        assert!((v.linear - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-5);
        assert!(v.angular.length() < 1e-5);
        // On target and still, nothing moves.
        let still = drive(target, Vec3::ZERO, target, Velocities::default(), &follow());
        assert!(still.linear.length() < 1e-6 && still.angular.length() < 1e-6);
    }

    #[test]
    fn the_drive_turns_the_short_way_and_is_capped() {
        let target = body(Vec3::ZERO, Quat::from_rotation_y(0.5));
        let at = body(Vec3::new(0.0, -10.0, 0.0), Quat::IDENTITY);
        let v = drive(target, Vec3::ZERO, at, Velocities::default(), &follow());
        assert!(
            (v.linear.length() - 5.0).abs() < 1e-4,
            "the speed is capped"
        );
        assert!((v.angular - Vec3::new(0.0, 4.0, 0.0)).length() < 1e-4);
        // A target given with the opposite sign is the same rotation.
        let flipped = body(Vec3::ZERO, -Quat::from_rotation_y(0.5));
        let w = drive(flipped, Vec3::ZERO, at, Velocities::default(), &follow());
        assert!((w.angular - v.angular).length() < 1e-4);
    }

    #[test]
    fn a_slow_response_eases_from_the_current_velocities() {
        let target = body(Vec3::new(1.0, 0.0, 0.0), Quat::IDENTITY);
        let at = body(Vec3::ZERO, Quat::IDENTITY);
        let half = Follow {
            response: 0.5,
            ..follow()
        };
        let v = drive(target, Vec3::ZERO, at, Velocities::default(), &half);
        assert!((v.linear - Vec3::new(2.5, 0.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn heavier_props_respond_more_slowly() {
        let config = PickupConfig::new();
        let light = Follow::for_mass(1.0, 1.0 / 60.0, &config);
        let heavy = Follow::for_mass(config.one_hand_mass_kg, 1.0 / 60.0, &config);
        assert!(light.response > heavy.response);
        assert!(heavy.response > 0.0 && light.response <= 1.0);
        let instant = PickupConfig {
            heavy_response_s: 0.0,
            ..config
        };
        assert_eq!(Follow::for_mass(10.0, 1.0 / 60.0, &instant).response, 1.0);
    }

    #[test]
    fn the_mass_limits_need_two_hands_for_heavy_props() {
        let config = PickupConfig::new();
        let light = config.one_hand_mass_kg * 0.5;
        let heavy = (config.one_hand_mass_kg + config.two_hand_mass_kg) * 0.5;
        assert!(can_lift(light, 1, &config));
        assert!(!can_lift(heavy, 1, &config));
        assert!(can_lift(heavy, 2, &config));
        assert!(!can_lift(config.two_hand_mass_kg * 2.0, 2, &config));
        assert!(!can_lift(light, 0, &config));
        assert!(!can_lift(f32::INFINITY, 2, &config));
        assert_eq!(mass_of(0.5), Some(2.0));
        assert_eq!(mass_of(0.0), None);
    }

    #[test]
    fn a_body_far_from_its_target_is_torn_away() {
        let target = body(Vec3::ZERO, Quat::IDENTITY);
        assert!(!torn_away(
            target,
            body(Vec3::new(0.2, 0.0, 0.0), Quat::IDENTITY),
            0.5
        ));
        assert!(torn_away(
            target,
            body(Vec3::new(0.8, 0.0, 0.0), Quat::IDENTITY),
            0.5
        ));
        assert!(torn_away(target, body(Vec3::NAN, Quat::IDENTITY), 0.5));
    }

    #[test]
    fn a_throw_is_the_hand_velocity_scaled_and_capped() {
        let config = PickupConfig {
            throw_scale: 2.0,
            max_throw_speed_mps: 10.0,
            ..PickupConfig::new()
        };
        assert!(
            (throw_velocity(Vec3::new(1.0, 2.0, 0.0), &config) - Vec3::new(2.0, 4.0, 0.0)).length()
                < 1e-5
        );
        assert!((throw_velocity(Vec3::new(0.0, 0.0, 9.0), &config).length() - 10.0).abs() < 1e-4);
        assert_eq!(throw_velocity(Vec3::NAN, &config), Vec3::ZERO);
    }

    #[test]
    fn the_tracker_follows_a_steady_motion() {
        let mut tracker = VelocityTracker::new();
        let dt = 1.0 / 60.0;
        let mut velocity = Vec3::ZERO;
        for i in 0..120 {
            velocity = tracker.update(Vec3::new(2.0 * dt * i as f32, 0.0, 0.0), dt, 0.03);
        }
        assert!((velocity - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-3);
        tracker.reset();
        assert_eq!(tracker, VelocityTracker::new());
        // The first sample after a reset has nothing to difference against.
        assert_eq!(tracker.update(Vec3::splat(5.0), dt, 0.03), Vec3::ZERO);
    }
}
