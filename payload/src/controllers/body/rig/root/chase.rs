//! The chase's arithmetic: the point the root walks toward, the gap to it, the velocity that
//! closes it, and the warp test. Pure.
//!
//! The target is the ground point under the player's upright eyes ([`target`]): the neck pivot,
//! less the arm from the pivot to the eyes turned by the torso's heading. Nodding and turning the
//! head swing the eyes about the pivot, so they move neither the pivot nor the target. The view's
//! anchor stands the eyes of a neutral head directly over the root, so in the cockpit frame the
//! root is at the cockpit's horizontal origin, and the gap is the target less the lean
//! ([`crate::controllers::body::rig::lean`]).
//!
//! The frame half measures the gap once per rendered frame ([`measure`]) and publishes it with the
//! completed chase movement it had folded into the cockpit shift then ([`ChaseFrame`]). Each
//! simulation tick takes away what the chase has completed since ([`ChaseFrame::effective_gap`]),
//! so two ticks in one frame never chase the same gap twice, and asks for a share of what is left
//! plus the head's own velocity ([`velocity`]). Nothing accumulates: every tick starts from the head
//! as the latest frame saw it.
//!
//! Cockpit vectors are `(x, z)` in the cockpit frame (+X right, -Z forward); world vectors are
//! world `(x, z)`.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::body::{
    rig::{
        config::{ChaseConfig, LeanConfig},
        lean::{Lean, lean},
        root::ledger::CompletedMovement,
    },
    torso::EYES_TO_NECK_PIVOT,
};

/// The ground point under the upright eyes, cockpit `(x, z)`, for eyes at `eyes` turned `head`
/// over a torso facing `torso_yaw`.
pub fn target(eyes: Vec3, head: Quat, torso_yaw: f32) -> Vec2 {
    let neck = eyes + head.normalize() * EYES_TO_NECK_PIVOT;
    let upright = neck - Quat::from_rotation_y(torso_yaw) * EYES_TO_NECK_PIVOT;
    let point = Vec2::new(upright.x, upright.z);
    if point.is_finite() { point } else { Vec2::ZERO }
}

/// What the frame half publishes for the simulation half.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ChaseFrame {
    /// The gap the chase closes, world `(x, z)`, metres: the target less the root, less the lean.
    pub gap: Vec2,
    /// The cumulative completed chase movement the cockpit shift had folded in when the gap was
    /// measured, world `(x, z)`.
    pub observed: Vec2,
    /// The head's horizontal velocity, world `(x, z)`, metres per second.
    pub head_velocity: Vec2,
}

impl ChaseFrame {
    /// The gap left once the chase movement completed since the frame is taken away, for the
    /// cumulative completed movement `completed`.
    pub fn effective_gap(&self, completed: Vec2) -> Vec2 {
        let gap = self.gap - (completed - self.observed);
        if gap.is_finite() { gap } else { Vec2::ZERO }
    }
}

/// The chase velocity for a tick of `dt` seconds, world `(x, z)`: the configured share of the
/// effective gap per tick, plus the head's velocity weighted by the feed-forward, at most the speed
/// limit. Zero with the chase off.
pub fn velocity(
    frame: &ChaseFrame,
    completed: CompletedMovement,
    dt: f32,
    config: &ChaseConfig,
) -> Vec2 {
    if !config.enabled || !dt.is_finite() || dt <= 0.0 {
        return Vec2::ZERO;
    }
    let gap = frame.effective_gap(completed.current);
    let velocity = frame.head_velocity * config.feed_forward.max(0.0)
        + gap * (config.gain.clamp(0.0, 1.0) / dt);
    let velocity = velocity.clamp_length_max(config.max_speed_mps.max(0.0));
    if velocity.is_finite() {
        velocity
    } else {
        Vec2::ZERO
    }
}

/// One frame's inputs to the measurement, in the cockpit frame unless named otherwise.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MeasureInput {
    /// The head's cockpit position, with the composed height.
    pub raw: Vec3,
    /// The cockpit shift for the current side of the pose pair.
    pub shift: Vec3,
    /// The completed chase movement the shift has folded in, world `(x, z)`.
    pub observed: Vec2,
    pub head: Quat,
    /// The torso's heading.
    pub torso_yaw: f32,
    /// The body rotation the cockpit composes with.
    pub view: Quat,
    /// World metres per cockpit metre.
    pub world_scale: f32,
    /// How far the neck pivot is below the standing height, metres.
    pub drop_m: f32,
    /// Seconds since the last frame.
    pub dt: f32,
}

/// One frame's measurement.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Measurement {
    /// The target relative to the root, cockpit `(x, z)`.
    pub target: Vec2,
    pub lean: Lean,
    /// Where the player's feet are, cockpit `(x, z)` relative to the raw cockpit's origin: the
    /// target less the lean.
    pub feet: Vec2,
    pub frame: ChaseFrame,
}

impl Measurement {
    /// How far the target is from the root, before the lean, metres in the cockpit frame.
    pub fn gap_m(&self) -> f32 {
        self.target.length()
    }

    /// How far the target is from the root outside the lean, metres in the cockpit frame: what
    /// the chase still has to close.
    pub fn residual_m(&self) -> f32 {
        (self.target - self.lean.vector).length()
    }
}

/// Measure the gap for one rendered frame. `head_velocity` keeps the head's smoothed velocity
/// between frames.
pub fn measure(
    input: &MeasureInput,
    head_velocity: &mut HeadVelocity,
    lean_config: &LeanConfig,
) -> Measurement {
    let raw_target = target(input.raw, input.head, input.torso_yaw);
    let shift = Vec2::new(input.shift.x, input.shift.z);
    let shift = if shift.is_finite() { shift } else { Vec2::ZERO };
    let relative = raw_target - shift;
    let lean = lean(relative, input.torso_yaw, input.drop_m, lean_config);
    let velocity = head_velocity.update(raw_target, input.dt);
    // A head moving within the lean moves nothing below it, so its velocity feeds nothing forward;
    // only a head out past the lean pulls the feet after it.
    let within_lean = lean.allowance_m > 0.0 && (relative - lean.vector).length() <= WITHIN_LEAN_M;
    let velocity = if within_lean { Vec2::ZERO } else { velocity };
    let to_world = |v: Vec2| to_world(v, input.view, input.world_scale);
    Measurement {
        target: relative,
        lean,
        feet: raw_target - lean.vector,
        frame: ChaseFrame {
            gap: to_world(relative - lean.vector),
            observed: input.observed,
            head_velocity: to_world(velocity),
        },
    }
}

/// A cockpit `(x, z)` vector carried into the world by the body rotation `view` and the world
/// scale, world `(x, z)`.
pub fn to_world(v: Vec2, view: Quat, world_scale: f32) -> Vec2 {
    let world = view.normalize() * Vec3::new(v.x, 0.0, v.y) * world_scale;
    let flat = Vec2::new(world.x, world.z);
    if flat.is_finite() { flat } else { Vec2::ZERO }
}

/// A world `(x, z)` vector carried into the cockpit frame by the inverse of [`to_world`].
pub fn to_cockpit(v: Vec2, view: Quat, world_scale: f32) -> Vec3 {
    let cockpit = view.normalize().inverse() * Vec3::new(v.x, 0.0, v.y) / world_scale;
    let flat = Vec3::new(cockpit.x, 0.0, cockpit.z);
    if flat.is_finite() { flat } else { Vec3::ZERO }
}

/// The head's horizontal velocity in the cockpit frame, smoothed over [`HEAD_VELOCITY_S`], with
/// tracking noise below [`HEAD_VELOCITY_FLOOR_MPS`] taken out.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HeadVelocity {
    last: Option<Vec2>,
    smoothed: Vec2,
}

impl HeadVelocity {
    pub const fn new() -> Self {
        Self {
            last: None,
            smoothed: Vec2::ZERO,
        }
    }

    /// Forget the head: the next update starts at rest. For a recenter or a teleport, which move
    /// the cockpit position without the head moving.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Advance with the head's point at `point` after `dt` seconds, returning the velocity, metres
    /// per second.
    pub fn update(&mut self, point: Vec2, dt: f32) -> Vec2 {
        if !point.is_finite() || !dt.is_finite() || dt > HEAD_VELOCITY_GAP_S {
            self.reset();
            self.last = point.is_finite().then_some(point);
            return Vec2::ZERO;
        }
        if let Some(last) = self.last
            && dt > 0.0
        {
            let alpha = 1.0 - (-dt / HEAD_VELOCITY_S).exp();
            self.smoothed = self.smoothed.lerp((point - last) / dt, alpha);
        }
        self.last = Some(point);
        let speed = self.smoothed.length();
        if speed <= HEAD_VELOCITY_FLOOR_MPS {
            Vec2::ZERO
        } else {
            self.smoothed * ((speed - HEAD_VELOCITY_FLOOR_MPS) / speed)
        }
    }
}

/// How quickly the head's velocity follows its motion, seconds (a time constant).
pub const HEAD_VELOCITY_S: f32 = 0.05;

/// The head speed that counts as tracking noise, metres per second: the feed-forward takes only
/// what is above it, so a head at rest never drives the root.
pub const HEAD_VELOCITY_FLOOR_MPS: f32 = 0.05;

/// How close to the lean's edge a gap counts as within it, metres.
const WITHIN_LEAN_M: f32 = 1e-4;

/// A gap between frames longer than this restarts the head's velocity, seconds.
const HEAD_VELOCITY_GAP_S: f32 = 0.25;

/// Whether a single-tick body displacement of `distance` metres, at `speed` metres per second over
/// `dt` seconds, is a teleport rather than movement: further than `min_m`, and further than three
/// times what the speed covers, so that fast vehicles and dives never trip it.
pub fn is_warp(distance: f32, speed: f32, dt: f32, min_m: f32) -> bool {
    let travelled = if speed.is_finite() && dt.is_finite() {
        3.0 * speed.abs() * dt.max(0.0)
    } else {
        0.0
    };
    distance.is_finite() && distance > min_m.max(travelled)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: ChaseConfig = ChaseConfig::new();
    const DT: f32 = 1.0 / 30.0;

    #[test]
    fn nodding_and_turning_the_head_leave_the_target() {
        let at = |head: Quat| {
            // Eyes swung about a pivot standing at the origin.
            let eyes = head * -EYES_TO_NECK_PIVOT;
            target(eyes, head, 0.0)
        };
        // The upright eyes are ahead of the pivot.
        let neutral = at(Quat::IDENTITY);
        assert!((neutral - Vec2::new(0.0, -EYES_TO_NECK_PIVOT.z)).length() < 1e-6);
        for head in [
            Quat::from_rotation_x(-0.8),
            Quat::from_rotation_y(0.9),
            Quat::from_rotation_y(-0.6) * Quat::from_rotation_x(0.4),
        ] {
            assert!((at(head) - neutral).length() < 1e-5, "{head}");
        }
        assert_eq!(target(Vec3::NAN, Quat::IDENTITY, 0.0), Vec2::ZERO);
    }

    #[test]
    fn the_chase_closes_a_share_of_the_gap_per_tick() {
        let frame = ChaseFrame {
            gap: Vec2::new(0.0, 0.1),
            ..ChaseFrame::default()
        };
        let v = velocity(&frame, CompletedMovement::default(), DT, &CONFIG);
        assert!((v * DT - Vec2::new(0.0, 0.04)).length() < 1e-6, "{v}");
        let off = ChaseConfig {
            enabled: false,
            ..CONFIG
        };
        assert_eq!(
            velocity(&frame, CompletedMovement::default(), DT, &off),
            Vec2::ZERO
        );
    }

    #[test]
    fn there_is_no_deadzone() {
        let frame = ChaseFrame {
            gap: Vec2::new(0.005, 0.0),
            ..ChaseFrame::default()
        };
        assert!(velocity(&frame, CompletedMovement::default(), DT, &CONFIG).x > 0.0);
    }

    #[test]
    fn the_chase_is_capped_and_adds_the_head_velocity() {
        let far = ChaseFrame {
            gap: Vec2::new(5.0, 0.0),
            ..ChaseFrame::default()
        };
        let v = velocity(&far, CompletedMovement::default(), DT, &CONFIG);
        assert!((v.length() - CONFIG.max_speed_mps).abs() < 1e-5);
        let walking = ChaseFrame {
            head_velocity: Vec2::new(0.0, -1.4),
            ..ChaseFrame::default()
        };
        let v = velocity(&walking, CompletedMovement::default(), DT, &CONFIG);
        assert!((v - Vec2::new(0.0, -1.4)).length() < 1e-6);
    }

    #[test]
    fn completed_movement_since_the_frame_comes_off_the_gap() {
        let frame = ChaseFrame {
            gap: Vec2::new(0.3, 0.0),
            observed: Vec2::new(1.0, 0.0),
            head_velocity: Vec2::ZERO,
        };
        let completed = Vec2::new(1.12, 0.0);
        assert!((frame.effective_gap(completed) - Vec2::new(0.18, 0.0)).length() < 1e-6);
    }

    #[test]
    fn the_head_velocity_ignores_noise_and_follows_a_walk() {
        let mut velocity = HeadVelocity::new();
        let dt = 1.0 / 90.0;
        // Millimetre jitter at rest.
        for i in 0..90 {
            let jitter = if i % 2 == 0 { 0.0005 } else { -0.0005 };
            assert_eq!(velocity.update(Vec2::new(jitter, 0.0), dt), Vec2::ZERO);
        }
        // A steady walk at 1.4 m/s: within a few frames the velocity is the walk's, less the floor.
        let mut v = Vec2::ZERO;
        for i in 0..45 {
            v = velocity.update(Vec2::new(0.0, -1.4 * dt * i as f32), dt);
        }
        assert!((v - Vec2::new(0.0, -1.35)).length() < 0.01, "{v}");
        // A long gap starts over.
        assert_eq!(velocity.update(Vec2::ZERO, 1.0), Vec2::ZERO);
    }

    #[test]
    fn the_cockpit_carries_into_the_world_by_the_view_and_the_scale() {
        let quarter = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let world = to_world(Vec2::new(0.0, -1.0), quarter, 2.0);
        assert!((world - Vec2::new(-2.0, 0.0)).length() < 1e-5, "{world}");
    }

    #[test]
    fn a_warp_is_far_and_faster_than_the_body_moves() {
        assert!(!is_warp(0.5, 5.0, DT, 8.0));
        assert!(is_warp(20.0, 5.0, DT, 8.0));
        // A diving wingsuit covers ten metres a tick at 300 m/s.
        assert!(!is_warp(10.0, 300.0, DT, 8.0));
        assert!(!is_warp(f32::NAN, 0.0, 0.0, 8.0));
    }
}
