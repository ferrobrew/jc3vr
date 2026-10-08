//! The stable view anchor's pure core.
//!
//! On foot the camera hangs from an anchor, the head bone's world position, and the headset's
//! motion is added onto it. Taken from the animated head, the anchor carries every bob, lean, and
//! start or stop lurch of the clips into the view. Fixed to Rico's root instead, the anchor is the
//! root plus a neutral standing head measured once from the idle animation, so the view moves only
//! when the root does: at the stick's velocity, smoothly, and as the player's own head moves.
//!
//! - [`NeutralCapture`] measures the neutral head, neck, and eyes in model space, averaging the
//!   animated pose over a stretch of standing still.
//! - [`HeightFilter`] smooths the root's height while grounded, so a staircase's steps read as a
//!   ramp, while a drop or a teleport passes at once.
//! - [`Handover`] eases the anchor between the animated head and the root over a short window, so
//!   leaving the ground or entering a vehicle never jumps the view.
//! - [`stable_anchors`] composes the anchors from the neutral pose and the root's two tick-spaced
//!   transforms, and [`blend`] mixes them with the animated ones.

use glam::{Mat4, Vec3};

use crate::{controllers::locomotion::config::StableViewConfig, headpose::Anchors};

/// The neutral standing head, neck, and eye midpoint, in Rico's model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Neutral {
    pub head: Vec3,
    pub neck: Vec3,
    pub eyes: Vec3,
}

impl Neutral {
    fn is_plausible(&self) -> bool {
        self.head.is_finite()
            && self.neck.is_finite()
            && self.eyes.is_finite()
            && (MIN_HEAD_HEIGHT_M..=MAX_HEAD_HEIGHT_M).contains(&self.head.y)
            && self.head.distance(self.neck) < MAX_BONE_SPREAD_M
            && self.eyes.distance(self.neck) < MAX_BONE_SPREAD_M
    }
}

/// Averages the animated head, neck, and eyes over the time Rico stands still.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct NeutralCapture {
    sum: [Vec3; 3],
    seconds: f32,
}

impl NeutralCapture {
    pub const fn new() -> Self {
        Self {
            sum: [Vec3::ZERO; 3],
            seconds: 0.0,
        }
    }

    /// Add a sample held for `dt` seconds. Returns the neutral pose once `capture_s` seconds have
    /// been sampled, and starts over. An implausible sample restarts the capture.
    pub fn push(&mut self, sample: Neutral, dt: f32, capture_s: f32) -> Option<Neutral> {
        if !sample.is_plausible() || !dt.is_finite() || dt <= 0.0 {
            *self = Self::new();
            return None;
        }
        self.sum[0] += sample.head * dt;
        self.sum[1] += sample.neck * dt;
        self.sum[2] += sample.eyes * dt;
        self.seconds += dt;
        if self.seconds < capture_s {
            return None;
        }
        let neutral = Neutral {
            head: self.sum[0] / self.seconds,
            neck: self.sum[1] / self.seconds,
            eyes: self.sum[2] / self.seconds,
        };
        *self = Self::new();
        neutral.is_plausible().then_some(neutral)
    }

    /// Drop the samples so far: Rico moved.
    pub fn interrupt(&mut self) {
        *self = Self::new();
    }

    /// The seconds sampled so far.
    pub fn progress(&self) -> f32 {
        self.seconds
    }
}

/// A critically damped follower of the root's height.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HeightFilter {
    state: HeightState,
    /// The height the last step returned.
    last: Option<f32>,
}

/// What the height filter is doing.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
enum HeightState {
    /// Not following: the height is the root's.
    #[default]
    Idle,
    /// Following the root on the ground: the smoothed height and its rate.
    Following { height: f32, rate: f32 },
    /// Off the ground, winding what the smoothing still hid down to nothing: the offset from the
    /// root and how fast it shrinks, metres per second.
    Releasing { offset: f32, rate: f32 },
}

/// The smoothed height at the previous and the current tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeightPair {
    pub previous: f32,
    pub current: f32,
}

impl HeightFilter {
    pub const fn new() -> Self {
        Self {
            state: HeightState::Idle,
            last: None,
        }
    }

    /// Follow the root height `raw` for one tick of `dt` seconds. `supported` says the body stands
    /// on the ground this tick. Off it, what the smoothing still hides winds down to nothing over
    /// [`RELEASE_S`], so leaving the ground never jumps the view; a change beyond the snap distance
    /// (a drop, a teleport) passes at once.
    pub fn step(
        &mut self,
        raw: f32,
        dt: f32,
        supported: bool,
        config: &StableViewConfig,
    ) -> HeightPair {
        let previous = self.last.unwrap_or(raw);
        let current = self.advance(raw, dt, supported, config);
        self.last = Some(current);
        HeightPair { previous, current }
    }

    fn advance(&mut self, raw: f32, dt: f32, supported: bool, config: &StableViewConfig) -> f32 {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if !raw.is_finite() {
            self.state = HeightState::Idle;
            return raw;
        }
        let smoothed = match self.state {
            HeightState::Idle => raw,
            HeightState::Following { height, .. } => height,
            HeightState::Releasing { offset, .. } => raw + offset,
        };
        if (smoothed - raw).abs() > config.height_snap_m {
            self.state = HeightState::Idle;
            return raw;
        }
        if supported && config.height_smoothing_s > 0.0 {
            let rate = match self.state {
                HeightState::Following { rate, .. } => rate,
                _ => 0.0,
            };
            let (mut y, mut rate) =
                smooth_critically_damped(smoothed, rate, raw, config.height_smoothing_s, dt);
            let lag = config.max_height_lag_m.max(0.0);
            if (y - raw).abs() > lag {
                y = raw + (y - raw).clamp(-lag, lag);
                rate = 0.0;
            }
            self.state = HeightState::Following { height: y, rate };
            return y;
        }
        let (offset, rate) = match self.state {
            HeightState::Releasing { offset, rate } => (offset, rate),
            _ => (smoothed - raw, (smoothed - raw).abs() / RELEASE_S),
        };
        let offset = offset - offset.signum() * (rate * dt).min(offset.abs());
        self.state = if offset == 0.0 {
            HeightState::Idle
        } else {
            HeightState::Releasing { offset, rate }
        };
        raw + offset
    }
}

/// How long the offset the smoothing hid takes to wind down once the body leaves the ground,
/// seconds.
pub const RELEASE_S: f32 = 0.1;

/// The handover between the animated anchor (`0`) and the stable one (`1`).
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Handover {
    /// The linear progress, eased by [`Handover::weight`].
    progress: f32,
}

impl Handover {
    pub const fn new() -> Self {
        Self { progress: 0.0 }
    }

    /// Move toward the stable anchor while `stable`, toward the animated one otherwise, over
    /// `blend_s` seconds, and return the eased weight of the stable anchor.
    pub fn step(&mut self, stable: bool, dt: f32, blend_s: f32) -> f32 {
        let target = if stable { 1.0 } else { 0.0 };
        if blend_s <= 0.0 || !dt.is_finite() {
            self.progress = target;
        } else {
            let step = (dt.max(0.0) / blend_s).min(1.0);
            self.progress += (target - self.progress).clamp(-step, step);
        }
        self.weight()
    }

    /// The eased weight of the stable anchor.
    pub fn weight(&self) -> f32 {
        let t = self.progress.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

/// The anchors of the neutral pose carried by the root: `world_t0` and `world_t1` are the
/// character's previous- and current-tick world matrices, and `height` the smoothed root heights
/// that replace theirs.
pub fn stable_anchors(
    neutral: &Neutral,
    world_t0: Mat4,
    world_t1: Mat4,
    height: HeightPair,
) -> Anchors {
    let lift = |world: Mat4, smoothed: f32| Vec3::Y * (smoothed - world.w_axis.y);
    let lift_t1 = lift(world_t1, height.current);
    let lift_t0 = lift(world_t0, height.previous);
    Anchors {
        head: world_t1.transform_point3(neutral.head) + lift_t1,
        head_prev: world_t0.transform_point3(neutral.head) + lift_t0,
        neck: world_t1.transform_point3(neutral.neck) + lift_t1,
        eye_arm: neutral.eyes - neutral.neck,
    }
}

/// `animated` mixed toward `stable` by `weight`.
pub fn blend(animated: &Anchors, stable: &Anchors, weight: f32) -> Anchors {
    let w = weight.clamp(0.0, 1.0);
    Anchors {
        head: animated.head.lerp(stable.head, w),
        head_prev: animated.head_prev.lerp(stable.head_prev, w),
        neck: animated.neck.lerp(stable.neck, w),
        eye_arm: animated.eye_arm.lerp(stable.eye_arm, w),
    }
}

/// The range of neutral head heights above the root, metres, that describe a standing person.
const MIN_HEAD_HEIGHT_M: f32 = 1.0;
const MAX_HEAD_HEIGHT_M: f32 = 2.5;

/// The furthest the neck may sit from the head or the eyes in a plausible pose, metres.
const MAX_BONE_SPREAD_M: f32 = 0.5;

/// One step of a critically damped spring of time constant `tau` toward `target`, exact for a
/// constant target and stable for any step (the rational approximation of the exponential from
/// Game Programming Gems 4, "Critically damped ease-in/ease-out smoothing").
fn smooth_critically_damped(value: f32, rate: f32, target: f32, tau: f32, dt: f32) -> (f32, f32) {
    let omega = 2.0 / tau;
    let x = omega * dt;
    let decay = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = value - target;
    let temp = (rate + omega * change) * dt;
    let rate = (rate - omega * temp) * decay;
    let value = target + (change + temp) * decay;
    (value, rate)
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    const DT: f32 = 1.0 / 30.0;

    fn neutral() -> Neutral {
        Neutral {
            head: Vec3::new(0.0, 1.7, -0.05),
            neck: Vec3::new(0.0, 1.6, -0.02),
            eyes: Vec3::new(0.0, 1.72, -0.12),
        }
    }

    #[test]
    fn the_capture_averages_once_it_has_enough() {
        let mut capture = NeutralCapture::new();
        let mut result = None;
        for tick in 0..60 {
            let sway = if tick % 2 == 0 { 0.02 } else { -0.02 };
            let mut sample = neutral();
            sample.head.x += sway;
            if let Some(neutral) = capture.push(sample, DT, 1.5) {
                result = Some(neutral);
                break;
            }
        }
        let result = result.expect("the capture never completed");
        assert!(result.head.abs_diff_eq(neutral().head, 1e-3));
    }

    #[test]
    fn an_implausible_sample_restarts_the_capture() {
        let mut capture = NeutralCapture::new();
        capture.push(neutral(), 1.0, 1.5);
        let mut bad = neutral();
        bad.head = Vec3::ZERO;
        assert_eq!(capture.push(bad, DT, 1.5), None);
        assert_eq!(capture.progress(), 0.0);
    }

    #[test]
    fn the_height_filter_ramps_a_step_and_snaps_a_drop() {
        let config = StableViewConfig::new();
        let mut filter = HeightFilter::new();
        filter.step(0.0, DT, true, &config);
        let first = filter.step(0.2, DT, true, &config);
        assert!(first.current > 0.0 && first.current < 0.2);
        assert_eq!(first.previous, 0.0);
        let mut last = first.current;
        for _ in 0..30 {
            let pair = filter.step(0.2, DT, true, &config);
            assert!(pair.current >= last - 1e-6 && pair.current <= 0.2 + 1e-6);
            last = pair.current;
        }
        assert!((last - 0.2).abs() < 1e-3);
        let drop = filter.step(-5.0, DT, true, &config);
        assert_eq!(drop.current, -5.0);
    }

    #[test]
    fn the_height_filter_never_lags_beyond_its_limit() {
        let config = StableViewConfig::new();
        let mut filter = HeightFilter::new();
        let mut raw = 0.0;
        for _ in 0..120 {
            raw += 3.0 * DT;
            let pair = filter.step(raw, DT, true, &config);
            assert!(raw - pair.current <= config.max_height_lag_m + 1e-5);
        }
    }

    #[test]
    fn leaving_the_ground_winds_the_hidden_offset_down_without_a_jump() {
        let config = StableViewConfig::new();
        let mut filter = HeightFilter::new();
        filter.step(0.0, DT, true, &config);
        // A jump's first tick still on the ground: the smoothing trails the rising root.
        let mut root = 0.1;
        let trailing = filter.step(root, DT, true, &config).current;
        assert!(trailing < root);
        let mut last_gap = f32::INFINITY;
        let mut last = trailing;
        for _ in 0..10 {
            root += 0.1;
            let pair = filter.step(root, DT, false, &config);
            // Continuous with the last tick, rising with the root, and closing on it without
            // passing it.
            assert_eq!(pair.previous, last);
            assert!(pair.current >= last);
            let gap = root - pair.current;
            assert!(
                (0.0..=last_gap + 1e-6).contains(&gap),
                "{gap} after {last_gap}"
            );
            last_gap = gap;
            last = pair.current;
        }
        assert_eq!(last, root);
        // With nothing hidden, the air's heights pass through.
        assert_eq!(
            filter.step(root + 0.3, DT, false, &config).current,
            root + 0.3
        );
    }

    #[test]
    fn the_handover_eases_both_ways() {
        let mut handover = Handover::new();
        let mut weights = vec![];
        for _ in 0..10 {
            weights.push(handover.step(true, DT, 0.25));
        }
        assert!(weights.windows(2).all(|w| w[1] >= w[0]));
        assert_eq!(*weights.last().unwrap(), 1.0);
        for _ in 0..10 {
            handover.step(false, DT, 0.25);
        }
        assert_eq!(handover.weight(), 0.0);
    }

    #[test]
    fn the_stable_anchors_ride_the_root_and_ignore_its_bob() {
        let yaw = Quat::from_rotation_y(0.7);
        let t1 = Mat4::from_rotation_translation(yaw, Vec3::new(3.0, 10.0, -2.0));
        let t0 = Mat4::from_rotation_translation(yaw, Vec3::new(2.9, 10.0, -2.0));
        let height = HeightPair {
            previous: 10.0,
            current: 10.0,
        };
        let anchors = stable_anchors(&neutral(), t0, t1, height);
        assert!(
            anchors
                .head
                .abs_diff_eq(t1.transform_point3(neutral().head), 1e-5)
        );
        assert!((anchors.head - anchors.head_prev).abs_diff_eq(Vec3::new(0.1, 0.0, 0.0), 1e-5));
        let smoothed = HeightPair {
            previous: 9.9,
            current: 9.95,
        };
        let lowered = stable_anchors(&neutral(), t0, t1, smoothed);
        assert!((anchors.head.y - lowered.head.y - 0.05).abs() < 1e-5);
        assert!((anchors.head_prev.y - lowered.head_prev.y - 0.1).abs() < 1e-5);
    }

    #[test]
    fn blending_is_exact_at_the_ends() {
        let a = Anchors {
            head: Vec3::X,
            head_prev: Vec3::Y,
            neck: Vec3::Z,
            eye_arm: Vec3::ONE,
        };
        let b = Anchors {
            head: Vec3::NEG_X,
            head_prev: Vec3::NEG_Y,
            neck: Vec3::NEG_Z,
            eye_arm: Vec3::ZERO,
        };
        assert_eq!(blend(&a, &b, 0.0).head, a.head);
        assert_eq!(blend(&a, &b, 1.0).neck, b.neck);
    }
}
