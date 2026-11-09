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
    pub forward: Vec3,
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
    /// Each hand's position relative to the head on the last tick.
    last: [Option<Vec3>; 2],
    /// Each hand's smoothed velocity.
    velocity: [Vec3; 2],
    phase: [StrokePhase; 2],
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
            phase: [StrokePhase::Recovery; 2],
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
        let forward = Vec3::new(input.forward.x, 0.0, input.forward.z).normalize_or_zero();
        let right = forward.cross(Vec3::Y);
        let wet = input.hands.map(|hand| {
            hand.is_some_and(|hand| {
                input.head.is_finite()
                    && hand.position.is_finite()
                    && hand.palm.is_finite()
                    && (input.underwater
                        || hand.position.y <= input.head.y - config.water_below_eyes_m.max(0.0))
            })
        });
        for (i, hand) in input.hands.iter().enumerate() {
            if !wet[i] || input.dt > MAX_DT || forward == Vec3::ZERO {
                self.last[i] = None;
                self.velocity[i] = Vec3::ZERO;
                self.phase[i] = StrokePhase::Recovery;
                continue;
            }
            let hand = hand.unwrap();
            let position = hand.position - input.head;
            let raw = self.last[i].map_or(Vec3::ZERO, |last| (position - last) / dt);
            self.last[i] = Some(position);
            self.velocity[i] = self.velocity[i].lerp(raw, follow_factor(dt, VELOCITY_SMOOTHING_S));
            let reach = position.dot(forward);
            if raw.dot(forward) > min_speed || reach < -FINISH_BEHIND_HEAD_M {
                self.phase[i] = StrokePhase::Recovery;
                self.velocity[i] = raw;
            } else if reach >= CATCH_REACH_M && self.phase[i] == StrokePhase::Recovery {
                self.phase[i] = StrokePhase::Ready;
            }
        }
        let outward = [0, 1].map(|i| {
            let side = if i == 0 { -1.0 } else { 1.0 };
            input.hands[i].is_some_and(|hand| {
                wet[i]
                    && self.phase[i] != StrokePhase::Recovery
                    && (hand.position - input.head).dot(forward) >= CATCH_REACH_M
                    && (hand.position - input.head).dot(right) * side > 0.0
                    && self.velocity[i].dot(right) * side > min_speed
            })
        });
        let breaststroke = outward[0] && outward[1];
        let mut thrust = 0.0;
        let mut torque = 0.0;
        for (i, hand) in input.hands.iter().enumerate() {
            step.in_water[i] = wet[i];
            let Some(hand) = hand.filter(|_| wet[i] && self.last[i].is_some()) else {
                continue;
            };
            if self.phase[i] == StrokePhase::Recovery {
                continue;
            }
            let velocity = self.velocity[i];
            let speed = velocity.length();
            if speed <= min_speed {
                if matches!(self.phase[i], StrokePhase::Loading(_)) {
                    self.phase[i] = StrokePhase::Ready;
                }
                continue;
            }
            let backward = (-velocity.dot(forward)).max(0.0);
            let side = if i == 0 { -1.0 } else { 1.0 };
            let lateral = if breaststroke {
                (velocity.dot(right) * side).max(0.0) * BREASTSTROKE_SWEEP_SHARE
            } else {
                0.0
            };
            let drive = (backward + lateral - min_speed).max(0.0);
            let along = hand.palm.normalize_or_zero().dot(velocity / speed);
            let alignment = if along < -0.1 || hand.palm.length_squared() < 1e-6 {
                0.0
            } else {
                ((along - facing) / (1.0 - facing).max(1e-3))
                    .clamp(0.0, 1.0)
                    .sqrt()
                    .max(0.35)
            };
            let paddle = drive * alignment;
            if paddle <= 0.0 && self.phase[i] != StrokePhase::Power {
                self.phase[i] = StrokePhase::Ready;
                continue;
            }
            match self.phase[i] {
                StrokePhase::Ready | StrokePhase::Loading(_) => {
                    let travel = match self.phase[i] {
                        StrokePhase::Loading(travel) => travel,
                        _ => 0.0,
                    } + paddle * dt;
                    if travel >= CATCH_TRAVEL_M {
                        self.phase[i] = StrokePhase::Power;
                        step.caught[i] =
                            Some((paddle / config.catch_mps.max(0.05)).clamp(0.45, 1.0));
                    } else {
                        self.phase[i] = StrokePhase::Loading(travel);
                        continue;
                    }
                }
                StrokePhase::Power => {}
                StrokePhase::Recovery => continue,
            }
            let push = forward * paddle;
            step.push[i] = Vec2::new(push.x, push.z);
            thrust += paddle;
            let arm = hand.position - input.head;
            torque += arm.z * push.x - arm.x * push.z;
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

/// The longest tick that can contain a stroke, seconds. Longer gaps forget the hands' motion.
const MAX_DT: f32 = 0.1;

/// The time constant the hands' velocities are smoothed over, seconds: long enough to steady a
/// tracker's jitter, short enough to keep a stroke's catch crisp.
const VELOCITY_SMOOTHING_S: f32 = 0.04;

const CATCH_REACH_M: f32 = 0.2;
const FINISH_BEHIND_HEAD_M: f32 = 0.15;
const CATCH_TRAVEL_M: f32 = 0.045;
const BREASTSTROKE_SWEEP_SHARE: f32 = 0.85;

#[derive(Debug, Copy, Clone, PartialEq)]
enum StrokePhase {
    Recovery,
    Ready,
    Loading(f32),
    Power,
}
