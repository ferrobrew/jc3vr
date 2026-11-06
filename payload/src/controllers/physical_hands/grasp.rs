//! Holding a loose object, pure: which reachable object a hand would take, where on the object the
//! hand holds it, whether the hands holding it can lift it, and the palm's motion.
//!
//! A hand holds an object at the point it touched ([`Grip`]): the point is fixed in the body's
//! frame and in the palm's, and so is the body's orientation in the palm's frame. The hold then
//! asks the point to stay where the palm carries it and the body to turn with the palm
//! ([`crate::controllers::physical_hands::hold`]), so that a plank held by one end hangs and swings
//! from the hand rather than from its middle.

use std::num::NonZeroUsize;

use body_rig::{geometry::point_velocity, rotation::angular_velocity, smooth::follow_factor};
use glam::{Quat, Vec3};

use crate::controllers::{
    clock::TimeScale,
    frame::Pose,
    physical_hands::{
        characters::CharacterTarget, doors::DoorTarget, pickup::PickupTuning, rigid::BodyPose,
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

/// How far a hold's grab point strains from the hand across ticks, which decides when the hold is
/// torn away. A heavy object drags behind the hand, slowly, and stays held while it follows; a
/// hold is let go when its grab point is caught, past [`PickupTuning::break_distance_m`] and not
/// closing on the hand for [`PickupTuning::stuck_s`] (a crate snagged on a wall, a door at its
/// hinge's limit), or at once past [`PickupTuning::tear_distance_m`], further than the hand
/// could reach. Closing is judged against the hand's own motion, from both velocities, so a door
/// held while riding a fast car follows the hand however fast the two move together. An object
/// the hands cannot lift is measured across the ground only: its sag
/// below the hand is its weight resting on what supports it, not a snag.
///
/// The watch runs in real time, as the hold does: the palm moves in real time, so in slow motion
/// a grab point keeps up with it as at full speed and a snag is let go as soon.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Strain {
    /// How long the grab point has been past the break distance without following the hand,
    /// seconds of real time.
    stuck_s: f32,
}

/// One tick of a hold, as its [`Strain`] measures it, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StrainSample {
    /// The grab point on the body now.
    pub anchor: Vec3,
    /// Where the hand would have the grab point.
    pub target: Vec3,
    /// The grab point's velocity, metres per second of game time.
    pub velocity: Vec3,
    /// The velocity of where the hand would have the grab point, metres per second of game time.
    pub hand_velocity: Vec3,
    /// Whether the hands holding the body can lift it.
    pub liftable: bool,
}

impl Strain {
    /// Fold in one tick of `dt` game seconds at `time_scale` game seconds per real second, and
    /// return whether the hold is torn away.
    pub fn torn(
        &mut self,
        sample: &StrainSample,
        dt: f32,
        time_scale: f32,
        config: &PickupTuning,
    ) -> bool {
        let across = |v: Vec3| {
            if sample.liftable {
                v
            } else {
                Vec3::new(v.x, 0.0, v.z)
            }
        };
        let lag = across(sample.target - sample.anchor);
        let distance = lag.length();
        if !distance.is_finite() || distance > config.tear_distance_m.max(0.0) {
            return true;
        }
        let scale = TimeScale::new(time_scale);
        let finite = |v: Vec3| if v.is_finite() { v } else { Vec3::ZERO };
        let closing = finite(sample.velocity) - finite(sample.hand_velocity);
        let approach = across(closing).dot(lag.normalize_or_zero()) * scale.get();
        let caught = distance > config.break_distance_m.max(0.0)
            && approach < config.stuck_speed_mps.max(0.0);
        self.stuck_s = if caught {
            self.stuck_s + scale.real_dt(dt)
        } else {
            0.0
        };
        self.stuck_s >= config.stuck_s.max(0.0) && caught
    }
}

/// Whether `hands` hands can lift a body of `mass_kg` against gravity with their strength.
pub fn can_lift(mass_kg: f32, hands: usize, config: &PickupTuning) -> bool {
    let strength = hands as f32 * config.hand_force_n.max(0.0);
    mass_kg.is_finite() && mass_kg > 0.0 && mass_kg * config.gravity_mps2.max(0.0) <= strength
}

/// Whether a hand may take hold of a body of `mass_kg` at all.
pub fn can_hold(mass_kg: f32, config: &PickupTuning) -> bool {
    mass_kg.is_finite() && mass_kg > 0.0 && mass_kg <= config.max_mass_kg
}

/// Whether a hand may take hold of an articulated object of `mass_kg`, which its constraint
/// carries.
pub fn can_hold_articulated(mass_kg: f32, config: &PickupTuning) -> bool {
    mass_kg.is_finite() && mass_kg > 0.0 && mass_kg <= config.max_articulated_mass_kg
}

/// Whether a hand may take hold of a body of `mass_kg` reached as `kind`.
pub fn can_take(kind: &TargetKind, mass_kg: f32, config: &PickupTuning) -> bool {
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
pub fn throw_velocity(hand_velocity: Vec3, config: &PickupTuning) -> Vec3 {
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
        point_velocity(self.pose.position, self.velocity, self.angular_velocity, at)
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
                self.velocity = self.velocity.lerp(raw, follow_factor(dt, time_constant_s));
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
            && let Some(raw) = angular_velocity(last, at, dt)
        {
            self.velocity = self.velocity.lerp(raw, follow_factor(dt, time_constant_s));
        }
        self.last = Some(at);
        self.velocity
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;
    use crate::controllers::physical_hands::pickup::PICKUP;

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
        let config = PICKUP;
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

    const TICK: f32 = 1.0 / 30.0;

    fn sample(lag: Vec3, velocity: Vec3, liftable: bool) -> StrainSample {
        StrainSample {
            anchor: Vec3::ZERO,
            target: lag,
            velocity,
            hand_velocity: Vec3::ZERO,
            liftable,
        }
    }

    /// The ticks a hold lasts at `sample` and `time_scale`, up to `limit`.
    fn ticks_held(sample: &StrainSample, time_scale: f32, limit: usize) -> usize {
        let mut strain = Strain::default();
        (0..limit)
            .take_while(|_| !strain.torn(sample, TICK, time_scale, &PICKUP))
            .count()
    }

    #[test]
    fn a_heavy_object_dragged_behind_the_hand_stays_held() {
        // Past the break distance, but coming toward the hand at a walk's drag.
        let lag = Vec3::new(0.0, 0.0, -(PICKUP.break_distance_m + 0.2));
        let dragged = sample(lag, lag.normalize() * 0.8, true);
        assert_eq!(ticks_held(&dragged, 1.0, 300), 300);
    }

    #[test]
    fn a_caught_grab_point_is_torn_away_after_the_stuck_time() {
        let lag = Vec3::new(PICKUP.break_distance_m + 0.1, 0.0, 0.0);
        let caught = sample(lag, Vec3::ZERO, true);
        let ticks = ticks_held(&caught, 1.0, 300);
        let expected = (PICKUP.stuck_s / TICK).ceil() as usize;
        assert!(ticks.abs_diff(expected) <= 1, "{ticks} vs {expected}");
        // Moving away from the hand is no better than standing still.
        let pulled_away = sample(lag, -lag.normalize(), true);
        assert!(ticks_held(&pulled_away, 1.0, 300).abs_diff(expected) <= 1);
        // Inside the break distance nothing is caught, however still.
        let near = sample(
            lag.normalize() * PICKUP.break_distance_m * 0.9,
            Vec3::ZERO,
            true,
        );
        assert_eq!(ticks_held(&near, 1.0, 300), 300);
    }

    #[test]
    fn following_again_restarts_the_stuck_time() {
        let lag = Vec3::new(PICKUP.break_distance_m + 0.1, 0.0, 0.0);
        let caught = sample(lag, Vec3::ZERO, true);
        let following = sample(lag, lag.normalize(), true);
        let mut strain = Strain::default();
        for _ in 0..200 {
            for _ in 0..((PICKUP.stuck_s / TICK) as usize - 2) {
                assert!(!strain.torn(&caught, TICK, 1.0, &PICKUP));
            }
            assert!(!strain.torn(&following, TICK, 1.0, &PICKUP));
        }
    }

    #[test]
    fn following_is_judged_against_the_hands_motion() {
        // A door held while riding a car at 40 m/s: the hand and the door both move fast across
        // the lag's line, and the door closes on the hand at half a metre a second.
        let lag = Vec3::new(PICKUP.break_distance_m + 0.2, 0.0, 0.0);
        let ride = Vec3::new(-40.0, 0.0, 15.0);
        let riding = StrainSample {
            velocity: ride + lag.normalize() * 0.5,
            hand_velocity: ride,
            ..sample(lag, Vec3::ZERO, true)
        };
        assert_eq!(ticks_held(&riding, 1.0, 300), 300);
        // Moving with the hand at the same speed, it is not closing on it: caught.
        let caught = StrainSample {
            velocity: ride,
            ..riding
        };
        let expected = (PICKUP.stuck_s / TICK).ceil() as usize;
        assert!(ticks_held(&caught, 1.0, 300).abs_diff(expected) <= 1);
        // Coming toward a hand that pulls away faster still falls behind: caught.
        let outrun = StrainSample {
            velocity: lag.normalize() * 0.5,
            hand_velocity: lag.normalize() * 1.5,
            ..riding
        };
        assert!(ticks_held(&outrun, 1.0, 300).abs_diff(expected) <= 1);
    }

    #[test]
    fn past_the_tear_distance_the_hold_is_torn_at_once() {
        let far = Vec3::new(0.0, 0.0, PICKUP.tear_distance_m + 0.05);
        assert_eq!(ticks_held(&sample(far, far.normalize(), true), 1.0, 10), 0);
        assert_eq!(ticks_held(&sample(Vec3::NAN, Vec3::ZERO, true), 1.0, 10), 0);
    }

    #[test]
    fn an_object_too_heavy_to_lift_sags_without_being_torn() {
        // Hanging well below the hand, still, with its weight on the ground.
        let sag = Vec3::new(0.0, PICKUP.tear_distance_m + 0.3, 0.0);
        assert_eq!(ticks_held(&sample(sag, Vec3::ZERO, false), 1.0, 300), 300);
        // The same lag on one the hands can lift is a snag.
        assert_eq!(ticks_held(&sample(sag, Vec3::ZERO, true), 1.0, 300), 0);
        // Its drag across the ground still counts.
        let snagged = Vec3::new(PICKUP.break_distance_m + 0.1, 0.8, 0.0);
        assert!(ticks_held(&sample(snagged, Vec3::ZERO, false), 1.0, 300) < 300);
    }

    #[test]
    fn the_stuck_time_and_the_follow_run_in_real_time() {
        let lag = Vec3::new(PICKUP.break_distance_m + 0.1, 0.0, 0.0);
        let caught = sample(lag, Vec3::ZERO, true);
        let full = ticks_held(&caught, 1.0, 300);
        let slow = ticks_held(&caught, 0.5, 300);
        assert!(slow.abs_diff(full / 2) <= 1, "{slow} vs {full}");
        // A grab point following at the real-time threshold in game time is, at half speed,
        // following at half of it in real time.
        let creeping = sample(lag, lag.normalize() * PICKUP.stuck_speed_mps * 1.5, true);
        assert_eq!(ticks_held(&creeping, 1.0, 300), 300);
        assert!(ticks_held(&creeping, 0.5, 300) < 300);
    }

    #[test]
    fn a_throw_is_the_hand_velocity_scaled_and_capped() {
        let config = PickupTuning {
            throw_scale: 2.0,
            max_throw_speed_mps: 10.0,
            ..PICKUP
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
    }
}
