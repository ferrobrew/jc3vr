//! The strokes' pure model: the hands as paddles pushing the swimmer, the swim speed they keep up,
//! the turn an uneven stroke makes, and how that speed joins the velocity the game's swim cores
//! wrote ([`propel`]). The hands are in the cockpit frame (+X right, +Y up, +Z back, metres),
//! which rides the body.

use body_rig::smooth::follow_factor;
use glam::{Vec2, Vec3};

use crate::controllers::locomotion::config::SwimStrokeConfig;

/// The palm's normal in the wrist frame: the fingers run along -Z and the back of the hand faces
/// +Y, so the palm faces -Y.
pub const WRIST_PALM: Vec3 = Vec3::NEG_Y;

/// One hand as the strokes read it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StrokeHand {
    /// The grip's position.
    pub position: Vec3,
    /// The palm's normal, a unit vector.
    pub palm: Vec3,
}

/// One tick's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StrokeInput {
    /// Indexed by [`Hand::index`](crate::controllers::frame::Hand::index); `None` for a hand that
    /// is untracked or may not stroke.
    pub hands: [Option<StrokeHand>; 2],
    /// The head (the eye midpoint): the strokes turn the body about it, and a hand is in the water
    /// below it.
    pub head: Vec3,
    /// Whether the swimmer is underwater, where every hand is in the water.
    pub underwater: bool,
    /// Seconds.
    pub dt: f32,
}

/// What one tick of strokes asks of the swimmer.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StrokeStep {
    /// The swim speed the strokes keep up, metres per second along the way the swimmer faces.
    pub speed: f32,
    /// How far to turn the heading this tick, radians, positive to the right.
    pub turn_right: f32,
    /// Each hand's push on the swimmer this tick (+X right, +Z back), metres per second of paddle
    /// speed, by [`Hand::index`](crate::controllers::frame::Hand::index).
    pub push: [Vec2; 2],
    /// Whether each hand is in the water.
    pub in_water: [bool; 2],
    /// The strength, `0..=1`, of each hand's stroke that caught the water this tick.
    pub caught: [Option<f32>; 2],
    /// The smoothed turn the hands make about the head, positive turning left (counter-clockwise
    /// from above), square metres per second.
    pub torque: f32,
}

impl StrokeStep {
    pub const NONE: StrokeStep = StrokeStep {
        speed: 0.0,
        turn_right: 0.0,
        push: [Vec2::ZERO; 2],
        in_water: [false; 2],
        caught: [None; 2],
        torque: 0.0,
    };
}

/// The strokes' state across ticks.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StrokeModel {
    /// Each hand's position on the last tick.
    last: [Option<Vec3>; 2],
    /// Each hand's smoothed velocity.
    velocity: [Vec3; 2],
    /// Whether each hand's stroke is pushing, for the catch cue's hysteresis.
    stroking: [bool; 2],
    speed: f32,
    torque: f32,
}

impl Default for StrokeModel {
    fn default() -> Self {
        Self::new()
    }
}

impl StrokeModel {
    pub const fn new() -> Self {
        Self {
            last: [None; 2],
            velocity: [Vec3::ZERO; 2],
            stroking: [false; 2],
            speed: 0.0,
            torque: 0.0,
        }
    }

    /// Advance by one tick.
    pub fn step(&mut self, input: &StrokeInput, config: &SwimStrokeConfig) -> StrokeStep {
        let dt = if input.dt.is_finite() {
            input.dt.clamp(0.0, MAX_DT)
        } else {
            0.0
        };
        let mut step = StrokeStep::NONE;
        if dt <= 0.0 {
            step.speed = self.speed;
            step.torque = self.torque;
            return step;
        }
        let min_speed = config.min_speed_mps.max(0.0);
        let facing = config.palm_cone_deg.clamp(0.0, 89.0).to_radians().cos();
        let mut thrust = 0.0;
        let mut torque = 0.0;
        for (i, hand) in input.hands.iter().enumerate() {
            let Some(hand) = hand.filter(|h| h.position.is_finite() && h.palm.is_finite()) else {
                self.last[i] = None;
                self.velocity[i] = Vec3::ZERO;
                self.stroking[i] = false;
                continue;
            };
            let raw = self.last[i].map_or(Vec3::ZERO, |last| (hand.position - last) / dt);
            self.last[i] = Some(hand.position);
            let blend = follow_factor(dt, VELOCITY_SMOOTHING_S);
            self.velocity[i] = self.velocity[i].lerp(raw, blend);
            let in_water = input.underwater
                || hand.position.y <= input.head.y - config.water_below_eyes_m.max(0.0);
            step.in_water[i] = in_water;
            let velocity = self.velocity[i];
            let speed = velocity.length();
            let palm = hand.palm.normalize_or_zero();
            let paddle = if in_water && speed > min_speed {
                let along = palm.dot(velocity / speed);
                ((along - facing) / (1.0 - facing).max(1e-3)).clamp(0.0, 1.0) * (speed - min_speed)
            } else {
                0.0
            };
            // The water pushes the swimmer against the hand's motion.
            let push = -velocity.normalize_or_zero() * paddle;
            let flat = Vec2::new(push.x, push.z);
            step.push[i] = flat;
            let forward = -push.z;
            thrust += if forward < 0.0 {
                forward * BRAKE_SHARE
            } else {
                forward
            };
            let arm = hand.position - input.head;
            torque += arm.z * push.x - arm.x * push.z;
            let backward = -push.z;
            let was = self.stroking[i];
            let now = if was {
                backward > config.catch_mps * 0.5
            } else {
                backward > config.catch_mps
            };
            self.stroking[i] = now;
            if now && !was {
                step.caught[i] = Some((backward / (config.catch_mps * 2.0)).clamp(0.3, 1.0));
            }
        }
        let decay = (-dt / config.decay_s.max(0.05)).exp();
        let speed = (self.speed + config.speed_per_metre.max(0.0) * thrust * dt) * decay;
        self.speed = if speed.is_finite() {
            speed.clamp(0.0, config.max_speed_mps.max(0.0))
        } else {
            0.0
        };
        let smoothing = follow_factor(dt, config.turn_smoothing_s.max(0.01));
        self.torque += (torque - self.torque) * smoothing;
        let max_turn = config.max_turn_deg_s.max(0.0).to_radians();
        let rate = (-config.turn_gain * self.torque).clamp(-max_turn, max_turn);
        step.turn_right = if rate.is_finite() { rate * dt } else { 0.0 };
        step.speed = self.speed;
        step.torque = self.torque;
        step
    }
}

/// The velocity the swimmer moves at with the strokes' `speed` joined to `velocity`, the one the
/// game's swim core wrote, world space. `direction` is the way the swimmer faces, a unit vector:
/// level at the surface, so buoyancy keeps the vertical, and pitched underwater, where the swimmer
/// moves where it faces. The strokes raise the velocity's share along `direction` to `speed` and
/// leave everything else the core wrote (the drift across, the buoyancy, the waves' push); a core
/// already swimming faster along it, the left stick's crawl, keeps its own speed.
pub fn propel(velocity: Vec3, direction: Vec3, speed: f32) -> Vec3 {
    let direction = direction.normalize_or_zero();
    if !velocity.is_finite() || !speed.is_finite() || speed <= 0.0 || direction == Vec3::ZERO {
        return velocity;
    }
    let along = velocity.dot(direction);
    if along >= speed {
        velocity
    } else {
        velocity + direction * (speed - along)
    }
}

/// The longest tick the strokes integrate, seconds: a hitch longer than this is taken as this
/// long, so a stalled frame does not read as a fast stroke.
const MAX_DT: f32 = 0.1;

/// The time constant the hands' velocities are smoothed over, seconds: long enough to steady a
/// tracker's jitter, short enough to keep a stroke's catch crisp.
const VELOCITY_SMOOTHING_S: f32 = 0.04;

/// The share of a palm-first push forward that brakes the swimmer. A recovery swept forward with
/// the palm half facing the way it moves reads as a push forward, and at full strength it took
/// back about half of what real strokes gave; real water resists a slower recovery less
/// than the faster pull, which this stands in for.
const BRAKE_SHARE: f32 = 0.5;
