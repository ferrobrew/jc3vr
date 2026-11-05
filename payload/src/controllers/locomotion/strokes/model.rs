//! The strokes' pure model: the hands as paddles pushing the swimmer, the forward drive they keep
//! up, and the turn an uneven stroke makes. Everything is in the cockpit frame (+X right, +Y up,
//! +Z back, metres), which rides the body.

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
    /// The move stick's forward the strokes ask for, `0..=1`.
    pub forward: f32,
    /// How far to turn the heading this tick, radians, positive to the right.
    pub turn_right: f32,
    /// Each hand's push on the swimmer this tick (+X right, +Z back), metres per second of paddle
    /// speed, by [`Hand::index`](crate::controllers::frame::Hand::index).
    pub push: [Vec2; 2],
    /// Whether each hand is in the water.
    pub in_water: [bool; 2],
    /// The strength, `0..=1`, of each hand's stroke that caught the water this tick.
    pub caught: [Option<f32>; 2],
    /// The forward drive, before it is limited to a full stick.
    pub drive: f32,
    /// The smoothed turn the hands make about the head, positive turning left (counter-clockwise
    /// from above), square metres per second.
    pub torque: f32,
}

impl StrokeStep {
    pub const NONE: StrokeStep = StrokeStep {
        forward: 0.0,
        turn_right: 0.0,
        push: [Vec2::ZERO; 2],
        in_water: [false; 2],
        caught: [None; 2],
        drive: 0.0,
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
    drive: f32,
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
            drive: 0.0,
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
            step.drive = self.drive;
            step.forward = self.drive.min(1.0);
            step.torque = self.torque;
            return step;
        }
        let min_speed = config.min_speed_mps.max(0.0);
        let facing = config.palm_facing_deg.clamp(0.0, 89.0).to_radians().cos();
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
            thrust += -push.z;
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
        self.drive =
            ((self.drive + config.gain.max(0.0) * thrust * dt) * decay).clamp(0.0, MAX_DRIVE);
        let smoothing = follow_factor(dt, config.turn_smoothing_s.max(0.01));
        self.torque += (torque - self.torque) * smoothing;
        let max_turn = config.max_turn_deg_s.max(0.0).to_radians();
        let rate = (-config.turn_gain * self.torque).clamp(-max_turn, max_turn);
        step.turn_right = if rate.is_finite() { rate * dt } else { 0.0 };
        step.drive = self.drive;
        step.forward = self.drive.min(1.0);
        step.torque = self.torque;
        step
    }
}

/// The longest tick the strokes integrate, seconds: a hitch longer than this is taken as this
/// long, so a stalled frame does not read as a fast stroke.
const MAX_DT: f32 = 0.1;

/// The time constant the hands' velocities are smoothed over, seconds: long enough to steady a
/// tracker's jitter, short enough to keep a stroke's catch crisp.
const VELOCITY_SMOOTHING_S: f32 = 0.04;

/// The forward drive's ceiling: past a full stick, so a strong rhythm holds full speed through the
/// recovery between strokes, and short of piling up a long glide.
const MAX_DRIVE: f32 = 1.5;
