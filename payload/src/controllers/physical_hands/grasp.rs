//! Holding a loose object, pure: which reachable object a hand would take, where on the object the
//! hand holds it, whether the hands holding it can lift it, and the palm's motion.
//!
//! A hand holds an object at the point it touched ([`Grip`]): the point is fixed in the body's
//! frame and in the palm's, and so is the body's orientation in the palm's frame. The hold then
//! asks the point to stay where the palm carries it and the body to turn with the palm
//! ([`crate::controllers::physical_hands::hold`]), so that a plank held by one end hangs and swings
//! from the hand rather than from its middle.

use std::num::NonZeroUsize;

use glam::{Quat, Vec3};

use crate::controllers::{
    frame::Pose,
    physical_hands::{
        characters::CharacterTarget, config::PickupConfig, doors::DoorTarget, rigid::BodyPose,
    },
};

/// What a reachable object is to a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TargetKind {
    /// A loose rigid object, held in the hand, of this mass in kilograms.
    Prop { mass_kg: f32 },
    /// An object joined to the world or another body by a constraint, such as a hinged gate or a
    /// lever, of this mass in kilograms, which its constraint carries.
    Articulated { mass_kg: f32 },
    /// A weapon lying in the world, collected through the game's own pickup.
    Weapon,
    /// A vehicle's door, hood, trunk, or hatch: an open one held as a loose object on its hinge, a
    /// closed one gripped at its handle area until the latch gives.
    Door(DoorTarget),
    /// A character, held on to until a yank.
    Character(CharacterTarget),
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
    /// The surface's normal at the point, toward the palm; zero where none is known.
    pub normal: Vec3,
    pub kind: TargetKind,
}

/// The nearest of `reaches`.
pub fn nearest(reaches: impl IntoIterator<Item = Reach>) -> Option<Reach> {
    reaches
        .into_iter()
        .filter(|r| r.distance.is_finite())
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

/// Where a hand holds a body: the grab point in the body's frame and in the palm's, and the body's
/// orientation in the palm's frame, all fixed at the grab.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Grip {
    /// The grab point in the body's frame, about its centre of mass.
    on_body: Vec3,
    /// The grab point in the palm's frame.
    in_palm: Vec3,
    /// The body's orientation in the palm's frame.
    rotation: Quat,
}

/// Where a grip would have its body: the grab point and the body's orientation, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GripTarget {
    pub point: Vec3,
    pub orientation: Quat,
}

impl Grip {
    /// The grip of a palm at `palm` on a body at `body`, taken at the world point `point`.
    pub fn of(palm: Pose, body: BodyPose, point: Vec3) -> Self {
        let inverse = palm.orientation.inverse();
        Self {
            on_body: body.local(point),
            in_palm: inverse * (point - palm.position),
            rotation: (inverse * body.orientation).normalize(),
        }
    }

    /// The grab point on the body at `body`, world space.
    pub fn anchor(&self, body: BodyPose) -> Vec3 {
        body.point(self.on_body)
    }

    /// Where the body should be with the palm at `palm`.
    pub fn target(&self, palm: Pose) -> GripTarget {
        GripTarget {
            point: palm.position + palm.orientation * self.in_palm,
            orientation: (palm.orientation * self.rotation).normalize(),
        }
    }

    /// The centre of mass of a body at `target`.
    pub fn centre_at(&self, target: GripTarget) -> Vec3 {
        target.point - target.orientation * self.on_body
    }
}

/// Whether the grab point has been torn from the hand: it lies farther than `break_distance_m`
/// from where the hand holds it, as when the body is caught on something the hand moved past.
pub fn torn_away(anchor: Vec3, target: Vec3, break_distance_m: f32) -> bool {
    let distance = anchor.distance(target);
    !distance.is_finite() || distance > break_distance_m.max(0.0)
}

/// Whether `hands` hands can lift a body of `mass_kg` against gravity with their strength.
pub fn can_lift(mass_kg: f32, hands: usize, config: &PickupConfig) -> bool {
    let strength = hands as f32 * config.hand_force_n.max(0.0);
    mass_kg.is_finite() && mass_kg > 0.0 && mass_kg * config.gravity_mps2.max(0.0) <= strength
}

/// Whether a hand may take hold of a body of `mass_kg` at all.
pub fn can_hold(mass_kg: f32, config: &PickupConfig) -> bool {
    mass_kg.is_finite() && mass_kg > 0.0 && mass_kg <= config.max_mass_kg
}

/// Whether a hand may take hold of an articulated object of `mass_kg`, which its constraint
/// carries.
pub fn can_hold_articulated(mass_kg: f32, config: &PickupConfig) -> bool {
    mass_kg.is_finite() && mass_kg > 0.0 && mass_kg <= config.max_articulated_mass_kg
}

/// Whether a hand may take hold of a body of `mass_kg` reached as `kind`.
pub fn can_take(kind: &TargetKind, mass_kg: f32, config: &PickupConfig) -> bool {
    match kind {
        TargetKind::Articulated { .. } => can_hold_articulated(mass_kg, config),
        _ => can_hold(mass_kg, config),
    }
}

/// The mass of a body whose inverse mass is `inverse_mass`, or `None` for an infinite mass.
pub fn mass_of(inverse_mass: f32) -> Option<f32> {
    (inverse_mass.is_finite() && inverse_mass > 0.0).then(|| 1.0 / inverse_mass)
}

/// The velocity a body's grab point leaves the hand with: the hand's own, scaled and capped.
pub fn throw_velocity(hand_velocity: Vec3, config: &PickupConfig) -> Vec3 {
    if !hand_velocity.is_finite() {
        return Vec3::ZERO;
    }
    (hand_velocity * config.throw_scale.max(0.0)).clamp_length_max(config.max_throw_speed_mps)
}

/// A palm's motion on a tick: its place, and its linear and angular velocity, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PalmMotion {
    pub pose: Pose,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
}

impl PalmMotion {
    /// The velocity of the point `at` carried rigidly with the palm.
    pub fn point_velocity(&self, at: Vec3) -> Vec3 {
        self.velocity + self.angular_velocity.cross(at - self.pose.position)
    }
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
            if raw.is_finite() {
                self.velocity = self.velocity.lerp(raw, smoothing(dt, time_constant_s));
            }
        }
        self.last = Some(at);
        self.velocity
    }
}

/// An orientation's angular velocity, world space, smoothed across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct AngularTracker {
    last: Option<Quat>,
    velocity: Vec3,
}

impl AngularTracker {
    pub const fn new() -> Self {
        Self {
            last: None,
            velocity: Vec3::ZERO,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Take the orientation `at` after `dt` seconds, smoothing over `time_constant_s`, and return
    /// the angular velocity.
    pub fn update(&mut self, at: Quat, dt: f32, time_constant_s: f32) -> Vec3 {
        if !at.is_finite() {
            return self.velocity;
        }
        let at = at.normalize();
        if let Some(last) = self.last
            && dt.is_finite()
            && dt > 0.0
        {
            let raw = rotation_vector(at * last.inverse()) / dt;
            if raw.is_finite() {
                self.velocity = self.velocity.lerp(raw, smoothing(dt, time_constant_s));
            }
        }
        self.last = Some(at);
        self.velocity
    }
}

/// The rotation `q` as an axis times its angle, radians, the short way round.
pub fn rotation_vector(q: Quat) -> Vec3 {
    let q = if q.w < 0.0 { -q } else { q }.normalize();
    let (axis, angle) = q.to_axis_angle();
    let v = axis * angle;
    if v.is_finite() { v } else { Vec3::ZERO }
}

fn smoothing(dt: f32, time_constant_s: f32) -> f32 {
    if time_constant_s > 0.0 {
        1.0 - (-dt / time_constant_s).exp()
    } else {
        1.0
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
            normal: Vec3::ZERO,
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
    fn a_grip_holds_the_touched_point_where_the_hand_took_it() {
        let palm = pose(Vec3::new(1.0, 1.0, 0.0), Quat::from_rotation_y(0.3));
        let held = body(Vec3::new(1.6, 0.9, -0.2), Quat::from_rotation_x(0.5));
        let point = Vec3::new(1.05, 0.95, -0.02);
        let grip = Grip::of(palm, held, point);
        let same = grip.target(palm);
        assert!(same.point.distance(point) < 1e-5);
        assert!(grip.anchor(held).distance(point) < 1e-5);
        assert!(same.orientation.angle_between(held.orientation) < 1e-3);
        assert!(grip.centre_at(same).distance(held.centre) < 1e-5);
    }

    #[test]
    fn turning_the_wrist_swings_the_far_end_like_a_lever() {
        // A plank 1 m long held at its near end, its centre half a metre out along -Z.
        let palm = pose(Vec3::ZERO, Quat::IDENTITY);
        let plank = body(Vec3::new(0.0, 0.0, -0.5), Quat::IDENTITY);
        let grip = Grip::of(palm, plank, Vec3::new(0.0, 0.0, -0.02));
        let turned = grip.target(pose(Vec3::ZERO, Quat::from_rotation_y(FRAC_PI_2)));
        // The grab point barely moves; the centre swings round to -X.
        assert!(turned.point.length() < 0.03);
        let centre = grip.centre_at(turned);
        assert!((centre - Vec3::new(-0.5, 0.0, 0.0)).length() < 1e-4);
        assert!(
            (turned.orientation.angle_between(plank.orientation) - FRAC_PI_2).abs() < 1e-3,
            "the body turns with the hand"
        );
    }

    #[test]
    fn the_lift_follows_the_hands_strength() {
        let config = PickupConfig::new();
        let one = config.hand_force_n / config.gravity_mps2;
        assert!(can_lift(one * 0.9, 1, &config));
        assert!(!can_lift(one * 1.1, 1, &config));
        assert!(can_lift(one * 1.1, 2, &config));
        assert!(!can_lift(one, 0, &config));
        assert!(!can_lift(f32::INFINITY, 2, &config));
        assert!(can_hold(config.max_mass_kg, &config));
        assert!(!can_hold(config.max_mass_kg * 1.01, &config));
        // A hinged object is taken far heavier, since its hinge carries it.
        let hinged = TargetKind::Articulated { mass_kg: 0.0 };
        assert!(can_take(&hinged, config.max_mass_kg * 2.0, &config));
        assert!(!can_take(
            &hinged,
            config.max_articulated_mass_kg * 1.01,
            &config
        ));
        assert!(!can_take(
            &TargetKind::Weapon,
            config.max_mass_kg * 2.0,
            &config
        ));
        assert_eq!(mass_of(0.5), Some(2.0));
        assert_eq!(mass_of(0.0), None);
    }

    #[test]
    fn a_grab_point_far_from_the_hand_is_torn_away() {
        assert!(!torn_away(Vec3::ZERO, Vec3::new(0.2, 0.0, 0.0), 0.5));
        assert!(torn_away(Vec3::ZERO, Vec3::new(0.8, 0.0, 0.0), 0.5));
        assert!(torn_away(Vec3::NAN, Vec3::ZERO, 0.5));
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
    fn the_trackers_follow_a_steady_motion() {
        let mut tracker = VelocityTracker::new();
        let mut spin = AngularTracker::new();
        let dt = 1.0 / 60.0;
        let mut velocity = Vec3::ZERO;
        let mut angular = Vec3::ZERO;
        for i in 0..120 {
            let t = dt * i as f32;
            velocity = tracker.update(Vec3::new(2.0 * t, 0.0, 0.0), dt, 0.03);
            angular = spin.update(Quat::from_rotation_y(1.5 * t), dt, 0.03);
        }
        assert!((velocity - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-3);
        assert!((angular - Vec3::new(0.0, 1.5, 0.0)).length() < 1e-3);
        tracker.reset();
        assert_eq!(tracker, VelocityTracker::new());
        // The first sample after a reset has nothing to difference against.
        assert_eq!(tracker.update(Vec3::splat(5.0), dt, 0.03), Vec3::ZERO);
    }

    #[test]
    fn a_palm_carries_its_points_round() {
        let palm = PalmMotion {
            pose: pose(Vec3::ZERO, Quat::IDENTITY),
            velocity: Vec3::X,
            angular_velocity: Vec3::new(0.0, 2.0, 0.0),
        };
        // A point 0.5 m along -Z is swung toward -X by the turn, cancelling the palm's own +X.
        let v = palm.point_velocity(Vec3::new(0.0, 0.0, -0.5));
        assert!(v.length() < 1e-5);
        let short = rotation_vector(Quat::from_rotation_x(0.2));
        assert!((rotation_vector(-Quat::from_rotation_x(0.2)) - short).length() < 1e-6);
        assert!((short - Vec3::new(0.2, 0.0, 0.0)).length() < 1e-5);
    }
}
