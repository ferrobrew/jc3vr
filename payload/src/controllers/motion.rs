//! The hands' recent motion: a short history of each controller's grip pose and velocities, sampled
//! at every published frame, and the pure estimates built on it (a throw's release velocity and a
//! strike's peak speed).
//!
//! The sim tick runs at roughly 30 Hz while the controllers are sampled at the display rate, so a
//! consumer on the input tick that read only the latest frame would see one sample of a motion that
//! lasts a few frames. A throw decelerates in the last moments before the fingers open, and a
//! strike peaks between two ticks; both need the samples in between. [`record`] runs from
//! [`crate::controllers::frame::publish`], and the consumers read [`history`].
//!
//! Velocities are the runtime's own where it reports them, and a finite difference of the positions
//! otherwise. A point held away from the grip (a grenade in the palm) moves with the grip's linear
//! velocity plus the rotation's contribution at its offset.

use std::collections::VecDeque;

use glam::{Quat, Vec3};
use parking_lot::Mutex;

use crate::controllers::frame::{ControllerFrame, Hand};

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
        let linear = self.linear_velocity?;
        let lever = self.orientation * offset;
        Some(linear + self.angular_velocity.map_or(Vec3::ZERO, |w| w.cross(lever)))
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
pub(crate) fn record(frame: &ControllerFrame) {
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
        if !(h.grip.position_tracked && h.grip.orientation_tracked) {
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
pub(crate) fn clear() {
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
