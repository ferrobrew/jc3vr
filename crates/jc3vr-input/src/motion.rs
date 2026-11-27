//! The hands' recent motion: a short history of each controller's grip pose and velocities, sampled
//! at every published frame, and the pure estimates built on it (a throw's release velocity and a
//! strike's peak speed), and the one shaping of a release's velocity every release uses
//! ([`shaped_release`]).
//!
//! The sim tick runs at roughly 30 Hz while the controllers are sampled at the display rate, so a
//! consumer on the input tick that read only the latest frame would see one sample of a motion that
//! lasts a few frames. A throw decelerates in the last moments before the fingers open, and a
//! strike peaks between two ticks; both need the samples in between. [`record`] runs from
//! [`crate::frame::publish`], and the consumers read [`history`].
//!
//! Velocities are the runtime's own where it reports them, and a finite difference of the positions
//! otherwise. A point held away from the grip (a grenade in the palm) moves with the grip's linear
//! velocity plus the rotation's contribution at its offset.

use std::collections::VecDeque;

use body_rig::math::{geometry::point_velocity, rotation::angular_velocity};
use glam::{Quat, Vec3};
use parking_lot::Mutex;

use crate::frame::{ControllerFrame, Hand};

/// One sample of a controller's grip in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MotionSample {
    /// The frame's predicted display time, seconds, on the runtime's clock.
    pub time_s: f64,
    pub position: Vec3,
    pub orientation: Quat,
    /// Metres per second, when the runtime reported it.
    pub linear_velocity: Option<Vec3>,
    /// Radians per second about each axis, when the runtime reported it.
    pub angular_velocity: Option<Vec3>,
}

impl MotionSample {
    /// The velocity of the point at `offset` in the grip's frame, from the reported velocities, or
    /// `None` when the runtime reported no linear velocity.
    pub fn point_velocity(&self, offset: Vec3) -> Option<Vec3> {
        let angular = self.angular_velocity.unwrap_or(Vec3::ZERO);
        Some(point_velocity(
            self.position,
            self.linear_velocity?,
            angular,
            self.point(offset),
        ))
    }

    /// The position of the point at `offset` in the grip's frame.
    pub fn point(&self, offset: Vec3) -> Vec3 {
        self.position + self.orientation * offset
    }
}

/// How a release velocity is estimated from the samples before the release.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReleaseEstimate {
    /// How far back from the release the samples are considered, seconds.
    pub window_s: f32,
    /// The span around the fastest sample whose velocities are averaged, seconds on each side.
    pub peak_span_s: f32,
}

/// How a release reads the hand's history: the fastest moment in the last quarter second, averaged
/// with its immediate neighbours, because a throwing hand slows before the fingers finish opening.
pub const RELEASE_ESTIMATE: ReleaseEstimate = ReleaseEstimate {
    window_s: 0.25,
    peak_span_s: 0.015,
};

/// The velocity of the point at `offset` in the grip's frame at a release at `release_s`: the
/// average velocity of the samples within `peak_span_s` of the fastest sample in the window before
/// the release. The fastest moment rather than the last is used because a throwing hand slows
/// before the fingers finish opening. `None` without at least one velocity in the window.
///
/// `samples` must be in time order.
pub fn release_velocity(
    samples: &[MotionSample],
    release_s: f64,
    offset: Vec3,
    estimate: ReleaseEstimate,
) -> Option<Vec3> {
    let start = release_s - f64::from(estimate.window_s.max(0.0));
    let velocities = velocities(samples, offset)
        .filter(|(time, _)| *time >= start && *time <= release_s)
        .collect::<Vec<_>>();
    let (peak_time, _) = velocities
        .iter()
        .copied()
        .max_by(|a, b| a.1.length_squared().total_cmp(&b.1.length_squared()))?;
    let span = f64::from(estimate.peak_span_s.max(0.0));
    let (sum, count) = velocities
        .iter()
        .filter(|(time, _)| (time - peak_time).abs() <= span)
        .fold((Vec3::ZERO, 0u32), |(sum, count), (_, v)| {
            (sum + *v, count + 1)
        });
    (count > 0).then(|| sum / count as f32)
}

/// The grip's angular release velocity, using the same peak-window policy as [`release_velocity`].
/// Missing runtime angular velocities use the shortest orientation difference in cockpit axes.
/// `samples` must be in time order.
pub fn release_angular_velocity(
    samples: &[MotionSample],
    release_s: f64,
    estimate: ReleaseEstimate,
) -> Option<Vec3> {
    let start = release_s - f64::from(estimate.window_s.max(0.0));
    let velocities = samples
        .iter()
        .enumerate()
        .filter(|(_, sample)| sample.time_s >= start && sample.time_s <= release_s)
        .filter_map(|(index, sample)| {
            let angular = sample
                .angular_velocity
                .filter(|v| v.is_finite())
                .or_else(|| {
                    let previous = samples.get(index.checked_sub(1)?)?;
                    let dt = (sample.time_s - previous.time_s) as f32;
                    angular_velocity(previous.orientation, sample.orientation, dt)
                })?;
            angular.is_finite().then_some((sample.time_s, angular))
        })
        .collect::<Vec<_>>();
    let (peak_time, _) = velocities
        .iter()
        .copied()
        .max_by(|a, b| a.1.length_squared().total_cmp(&b.1.length_squared()))?;
    let span = f64::from(estimate.peak_span_s.max(0.0));
    let (sum, count) = velocities
        .iter()
        .filter(|(time, _)| (time - peak_time).abs() <= span)
        .fold((Vec3::ZERO, 0u32), |(sum, count), (_, angular)| {
            (sum + *angular, count + 1)
        });
    (count > 0).then(|| sum / count as f32)
}

/// How a release's velocity is shaped from the hand's: the hand's own share of its velocity, scaled
/// and capped, on top of the velocity it inherits from what carries it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReleaseShape {
    /// The factor on the hand's own share; a negative factor counts as zero.
    pub scale: f32,
    /// The cap on the scaled share, metres per second; a negative cap counts as zero.
    pub max_speed_mps: f32,
}

/// The velocity a release leaves with: `carry`, what the hand inherits from what carries it, plus
/// the hand's own share (`hand` less `carry`) scaled and capped by `shape`. A release with nothing
/// carrying it passes a zero `carry`. Zero when `hand` or `carry` is not finite, and `carry` alone
/// when the share or the scale is not.
///
/// The estimate of `hand` is the caller's: a hand's history ([`release_velocity`]) or the palm's
/// velocity on the release's tick.
pub fn shaped_release(hand: Vec3, carry: Vec3, shape: ReleaseShape) -> Vec3 {
    if !(hand.is_finite() && carry.is_finite()) {
        return Vec3::ZERO;
    }
    let share = hand - carry;
    if !share.is_finite() || !shape.scale.is_finite() {
        return carry;
    }
    carry + (share * shape.scale.max(0.0)).clamp_length_max(shape.max_speed_mps.max(0.0))
}

/// The fastest velocity of the grip among the samples after `since_s`, or `None` without one.
///
/// `samples` must be in time order.
pub fn peak_velocity(samples: &[MotionSample], since_s: f64) -> Option<(f64, Vec3)> {
    velocities(samples, Vec3::ZERO)
        .filter(|(time, _)| *time > since_s)
        .max_by(|a, b| a.1.length_squared().total_cmp(&b.1.length_squared()))
}

/// A copy of `hand`'s history, oldest first.
pub fn history(hand: Hand) -> Vec<MotionSample> {
    HISTORY.lock().hands[hand.index()].iter().copied().collect()
}

/// The display time of the latest recorded frame, seconds, or `None` before the first.
pub fn latest_time() -> Option<f64> {
    HISTORY.lock().latest
}

/// Record a published frame's grips. A hand whose grip is not tracked this frame records nothing,
/// so an inferred pose (an occluded controller) never feeds a throw or a strike, and a hand that is
/// not tracked at all forgets its history.
pub fn record(frame: &ControllerFrame) {
    let time_s = frame.display_time_ns as f64 * 1e-9;
    let mut history = HISTORY.lock();
    history.latest = Some(time_s);
    for hand in Hand::BOTH {
        let h = frame.hand(hand);
        let samples = &mut history.hands[hand.index()];
        if !h.active || !h.grip.is_valid() {
            samples.clear();
            continue;
        }
        if !h.grip.is_tracked() {
            continue;
        }
        push(
            samples,
            MotionSample {
                time_s,
                position: h.grip.pose.position,
                orientation: h.grip.pose.orientation,
                linear_velocity: h.grip.linear_velocity,
                angular_velocity: h.grip.angular_velocity,
            },
        );
    }
}

/// Forget both hands' histories: the controllers are disabled or the session is gone.
pub fn clear() {
    let mut history = HISTORY.lock();
    history.hands.iter_mut().for_each(VecDeque::clear);
    history.latest = None;
}

/// How long the history reaches back, seconds: longer than any window a consumer reads.
const HORIZON_S: f64 = 0.5;

/// The most samples kept per hand, a bound for display rates far above the usual.
const MAX_SAMPLES: usize = 256;

struct History {
    hands: [VecDeque<MotionSample>; 2],
    latest: Option<f64>,
}

static HISTORY: Mutex<History> = Mutex::new(History {
    hands: [VecDeque::new(), VecDeque::new()],
    latest: None,
});

/// Append `sample`, dropping any sample not older than it (the runtime's clock went back, as on a
/// session restart) and every sample past the horizon.
fn push(samples: &mut VecDeque<MotionSample>, sample: MotionSample) {
    while samples
        .back()
        .is_some_and(|last| last.time_s >= sample.time_s)
    {
        samples.pop_back();
    }
    samples.push_back(sample);
    while samples
        .front()
        .is_some_and(|first| sample.time_s - first.time_s > HORIZON_S)
        || samples.len() > MAX_SAMPLES
    {
        samples.pop_front();
    }
}

/// Each sample's velocity of the point at `offset`: the reported one, or the finite difference from
/// the previous sample.
fn velocities(samples: &[MotionSample], offset: Vec3) -> impl Iterator<Item = (f64, Vec3)> + '_ {
    samples.iter().enumerate().filter_map(move |(i, s)| {
        let velocity = s.point_velocity(offset).or_else(|| {
            let previous = samples.get(i.checked_sub(1)?)?;
            let dt = (s.time_s - previous.time_s) as f32;
            (dt > 0.0).then(|| (s.point(offset) - previous.point(offset)) / dt)
        })?;
        velocity.is_finite().then_some((s.time_s, velocity))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHAPE: ReleaseShape = ReleaseShape {
        scale: 2.0,
        max_speed_mps: 8.0,
    };

    #[test]
    fn a_release_scales_and_caps_only_the_hands_own_share() {
        let carry = Vec3::new(30.0, 0.0, 0.0);
        assert_eq!(
            shaped_release(carry + Vec3::Y * 20.0, carry, SHAPE),
            carry + Vec3::Y * 8.0
        );
        assert_eq!(
            shaped_release(carry + Vec3::Y, carry, SHAPE),
            carry + Vec3::Y * 2.0
        );
        assert_eq!(shaped_release(carry, carry, SHAPE), carry);
    }

    #[test]
    fn a_release_with_nothing_carrying_it_is_the_hand_scaled_and_capped() {
        assert_eq!(
            shaped_release(Vec3::new(1.0, 2.0, 0.0), Vec3::ZERO, SHAPE),
            Vec3::new(2.0, 4.0, 0.0)
        );
        assert!(
            (shaped_release(Vec3::new(0.0, 0.0, 9.0), Vec3::ZERO, SHAPE).length() - 8.0).abs()
                < 1e-5
        );
    }

    #[test]
    fn a_release_that_is_not_finite_leaves_with_nothing_or_its_carry() {
        assert_eq!(shaped_release(Vec3::NAN, Vec3::ZERO, SHAPE), Vec3::ZERO);
        assert_eq!(shaped_release(Vec3::ONE, Vec3::INFINITY, SHAPE), Vec3::ZERO);
        let unscaled = ReleaseShape {
            scale: f32::NAN,
            ..SHAPE
        };
        assert_eq!(shaped_release(Vec3::ONE, Vec3::X, unscaled), Vec3::X);
        let negative = ReleaseShape {
            scale: -1.0,
            max_speed_mps: -1.0,
        };
        assert_eq!(shaped_release(Vec3::ONE, Vec3::X, negative), Vec3::X);
    }

    const DT: f64 = 1.0 / 90.0;

    const ESTIMATE: ReleaseEstimate = ReleaseEstimate {
        window_s: 0.1,
        peak_span_s: 0.015,
    };

    fn sample(i: usize, position: Vec3, velocity: Option<Vec3>) -> MotionSample {
        MotionSample {
            time_s: i as f64 * DT,
            position,
            orientation: Quat::IDENTITY,
            linear_velocity: velocity,
            angular_velocity: None,
        }
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn the_throws_window_reaches_a_peak_before_the_fingers_open() {
        // A throw peaks at 8 m/s, then slows over a fifth of a second before the fingers open: a
        // relaxed squeeze lags the throw. The release's window still finds the peak; a window of a
        // tenth of a second only finds the slowed hand.
        let peak = Vec3::new(0.0, 2.0, -8.0);
        let samples = (0..60)
            .map(|i| {
                let t = i as f64 * DT;
                let release_s = 59.0 * DT;
                let since_peak = (t - (release_s - 0.2)).max(0.0) as f32;
                let velocity = peak * (1.0 - since_peak * 4.0).max(0.1);
                sample(i, Vec3::ZERO, Some(velocity))
            })
            .collect::<Vec<_>>();
        let release_s = 59.0 * DT;
        let wide = release_velocity(&samples, release_s, Vec3::ZERO, RELEASE_ESTIMATE).unwrap();
        let narrow = release_velocity(&samples, release_s, Vec3::ZERO, ESTIMATE).unwrap();
        assert!(wide.length() > 7.0, "{wide:?}");
        assert!(narrow.length() < wide.length() * 0.8, "{narrow:?}");
    }

    #[test]
    fn a_steady_motion_releases_at_its_velocity() {
        let v = Vec3::new(0.0, 2.0, -6.0);
        let samples = (0..20)
            .map(|i| sample(i, v * (i as f64 * DT) as f32, Some(v)))
            .collect::<Vec<_>>();
        let release = samples.last().unwrap().time_s;
        let got = release_velocity(&samples, release, Vec3::ZERO, ESTIMATE).unwrap();
        assert!(close(got, v), "{got}");
    }

    #[test]
    fn the_release_takes_the_peak_not_the_slowing_end() {
        // Speeds up to 8 m/s, then slows to 2 m/s before the grip opens.
        let speeds: [f32; 8] = [1.0, 3.0, 5.0, 7.0, 8.0, 6.0, 4.0, 2.0];
        let samples = speeds
            .iter()
            .enumerate()
            .map(|(i, s)| sample(i, Vec3::ZERO, Some(Vec3::new(0.0, 0.0, -s))))
            .collect::<Vec<_>>();
        let release = samples.last().unwrap().time_s;
        let got = release_velocity(&samples, release, Vec3::ZERO, ESTIMATE).unwrap();
        // The peak and its neighbours within the span: 7, 8, and 6.
        assert!(close(got, Vec3::new(0.0, 0.0, -7.0)), "{got}");
    }

    #[test]
    fn samples_outside_the_window_are_ignored() {
        let mut samples = vec![sample(0, Vec3::ZERO, Some(Vec3::new(0.0, 0.0, -20.0)))];
        samples.extend((30..33).map(|i| sample(i, Vec3::ZERO, Some(Vec3::X))));
        let release = samples.last().unwrap().time_s;
        let got = release_velocity(&samples, release, Vec3::ZERO, ESTIMATE).unwrap();
        assert!(close(got, Vec3::X), "{got}");
        assert_eq!(release_velocity(&[], 1.0, Vec3::ZERO, ESTIMATE), None);
    }

    #[test]
    fn positions_stand_in_for_missing_velocities() {
        let v = Vec3::new(3.0, 0.0, 0.0);
        let samples = (0..6)
            .map(|i| sample(i, v * (i as f64 * DT) as f32, None))
            .collect::<Vec<_>>();
        let release = samples.last().unwrap().time_s;
        let got = release_velocity(&samples, release, Vec3::ZERO, ESTIMATE).unwrap();
        assert!(close(got, v), "{got}");
    }

    #[test]
    fn a_point_off_the_grip_adds_the_rotation() {
        // Spinning about +Y at 10 rad/s, a point 0.1 m along -Z moves along -X at 1 m/s.
        let s = MotionSample {
            angular_velocity: Some(Vec3::new(0.0, 10.0, 0.0)),
            ..sample(0, Vec3::ZERO, Some(Vec3::ZERO))
        };
        let got = s.point_velocity(Vec3::new(0.0, 0.0, -0.1)).unwrap();
        assert!(close(got, Vec3::new(-1.0, 0.0, 0.0)), "{got}");
    }

    #[test]
    fn the_peak_is_taken_after_the_given_time() {
        let samples = [
            sample(0, Vec3::ZERO, Some(Vec3::X * 9.0)),
            sample(1, Vec3::ZERO, Some(Vec3::X * 2.0)),
            sample(2, Vec3::ZERO, Some(Vec3::X * 4.0)),
        ];
        let (time, v) = peak_velocity(&samples, samples[0].time_s).unwrap();
        assert_eq!(time, samples[2].time_s);
        assert!(close(v, Vec3::X * 4.0));
        assert_eq!(peak_velocity(&samples, samples[2].time_s), None);
    }

    #[test]
    fn the_history_drops_old_and_out_of_order_samples() {
        let mut samples = VecDeque::new();
        for i in 0..200 {
            push(&mut samples, sample(i, Vec3::ZERO, None));
        }
        let span = samples.back().unwrap().time_s - samples.front().unwrap().time_s;
        assert!(span <= HORIZON_S);
        push(&mut samples, sample(5, Vec3::ZERO, None));
        assert_eq!(
            samples.len(),
            1,
            "a clock that went back restarts the history"
        );
    }
}
