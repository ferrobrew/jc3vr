//! The snatch's pure rules: which target the grapple's aim picks, where the pull carries the
//! target, when it arrives, when it has stalled or run out of line, and the toss that a tap at a gun
//! gives it.

use glam::Vec3;

use crate::controllers::aim::ray::Ray;

/// A thing the snatch could take, as the aim or the noted characters place it this tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Candidate<K> {
    /// What it is, for the caller to act on.
    pub kind: K,
    /// Where the hook would go, world space.
    pub point: Vec3,
}

/// The candidate chosen, with how far it is from the ray's origin.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Chosen<K> {
    pub candidate: Candidate<K>,
    pub distance: f32,
    /// The angle between the ray and the line to the candidate, radians.
    pub angle: f32,
}

/// The candidate of `candidates` within its cone of `ray` and `range` metres of its origin whose
/// line makes the smallest angle with the ray; of equal angles, the nearer. `cone` gives a
/// candidate's cone, radians, from its kind and its distance from the ray's origin. A candidate at
/// the ray's origin, or with a point that is not finite, is never chosen.
pub fn choose<K: Copy>(
    ray: &Ray,
    candidates: impl IntoIterator<Item = Candidate<K>>,
    cone: impl Fn(&K, f32) -> f32,
    range: f32,
) -> Option<Chosen<K>> {
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let offset = candidate.point - ray.origin;
            let distance = offset.length();
            if !(distance.is_finite() && distance > f32::EPSILON && distance <= range) {
                return None;
            }
            let angle = ray.direction.angle_between(offset / distance);
            (angle.is_finite() && angle <= cone(&candidate.kind, distance)).then_some(Chosen {
                candidate,
                distance,
                angle,
            })
        })
        .min_by(|a, b| {
            a.angle
                .total_cmp(&b.angle)
                .then(a.distance.total_cmp(&b.distance))
        })
}

/// The cone, radians, inside which a weapon lying in the world `distance` metres from the ray's
/// origin is chosen: `base`, widened by up to `widen` as the weapon nears `range`. A gun lying flat
/// subtends a fraction of a degree at range, so the ray's own hit cannot be asked of it, and the
/// farther it lies the more the hand's tremor and the ground's grazing angle throw the ray off it.
pub fn floor_cone(distance: f32, base: f32, widen: f32, range: f32) -> f32 {
    let share = if range > 0.0 && distance.is_finite() {
        (distance / range).clamp(0.0, 1.0)
    } else {
        0.0
    };
    base + widen.max(0.0) * share
}

/// The point the pull carries the target's centre toward: the hand, raised by `arc` of the
/// distance still to go, so a far target rises and falls into the hand rather than dragging along
/// the ground, and a near one comes straight in.
pub fn pull_point(body: Vec3, hand: Vec3, arc: f32) -> Vec3 {
    let distance = body.distance(hand);
    if !distance.is_finite() {
        return hand;
    }
    hand + Vec3::Y * distance * arc.max(0.0)
}

/// What a tick of the pull finds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The target is still on its way.
    Pulling,
    /// The target is within the capture radius of the hand.
    Arrived,
    /// The target has moved too little for a while: snagged on the world.
    Stalled,
    /// The target is farther from the hand than the line reaches.
    OutOfLine,
}

/// The pull's limits.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PullLimits {
    /// How near the hand the target arrives, metres.
    pub capture_radius_m: f32,
    /// How far the line reaches, metres.
    pub line_m: f32,
}

/// The verdict for a target `distance` metres from the hand, with `stalled` whether the stall
/// watch has fired.
pub fn verdict(distance: f32, stalled: bool, limits: &PullLimits) -> Verdict {
    if distance.is_nan() || distance > limits.line_m {
        Verdict::OutOfLine
    } else if distance <= limits.capture_radius_m {
        Verdict::Arrived
    } else if stalled {
        Verdict::Stalled
    } else {
        Verdict::Pulling
    }
}

/// Watches a pulled body for a stall: it has stalled when, over the last `window_s` seconds of
/// samples, it has stayed within `distance_m` of where it was at the window's start.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StallWatch {
    /// `(time, position)` samples, oldest first.
    samples: std::collections::VecDeque<(f32, Vec3)>,
    time: f32,
}

impl StallWatch {
    /// Advance by `dt` seconds with the body at `position`, and return whether it has stalled.
    pub fn step(&mut self, dt: f32, position: Vec3, window_s: f32, distance_m: f32) -> bool {
        self.time += if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.samples.push_back((self.time, position));
        // Keep one sample at or before the window's start, so a full window can be measured.
        while self.samples.len() > 1
            && self
                .samples
                .get(1)
                .is_some_and(|&(t, _)| self.time - t >= window_s)
        {
            self.samples.pop_front();
        }
        let Some(&(start, from)) = self.samples.front() else {
            return false;
        };
        if self.time - start < window_s {
            return false;
        }
        self.samples
            .iter()
            .all(|&(_, p)| p.distance(from) < distance_m)
    }
}

/// How far along `line`, the grapple hand's aim when the grip armed the snatch, the hand's grip at
/// `grip` reaches, metres; both in the cockpit frame. A yank back from the target is a drop in it,
/// whichever way the line points and wherever the hand is held: a seated player's hand pulled back
/// past their side, which neither nears the body nor the head, is a yank all the same. `None` for a
/// line with no direction.
pub fn reach_along(grip: Vec3, line: Vec3) -> Option<f32> {
    let line = line.try_normalize()?;
    let reach = grip.dot(line);
    reach.is_finite().then_some(reach)
}

/// The velocity that throws a body at `from` to land at `to` after `flight_s` seconds under
/// `gravity`: the toss a tap of the grapple at a gun gives it.
pub fn toss_velocity(from: Vec3, to: Vec3, flight_s: f32, gravity: Vec3) -> Vec3 {
    let t = if flight_s.is_finite() {
        flight_s.max(0.05)
    } else {
        0.05
    };
    (to - from) / t - gravity * t * 0.5
}

/// How long a toss over `distance` metres flies, seconds: the distance at `speed` metres per
/// second, kept between `shortest` and `longest`.
pub fn toss_flight(distance: f32, speed: f32, shortest: f32, longest: f32) -> f32 {
    let t = if speed > 0.0 {
        distance / speed
    } else {
        longest
    };
    if t.is_finite() {
        t.clamp(shortest, longest.max(shortest))
    } else {
        longest.max(shortest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{
        bindings::yank::{YankDetector, body_point},
        snatch::config::SnatchConfig,
    };

    /// A seated player's grip on a weapon on the ground ahead and to the right, then a pull of the
    /// hand back and out past their left side at about 0.9 m/s, sampled at the game's tick, with the
    /// aim at the grip edge and the head; cockpit frame. The shape of a recorded attempt.
    const SEATED_PULL: [Vec3; 13] = [
        Vec3::new(-0.029, -0.274, -0.390),
        Vec3::new(-0.047, -0.264, -0.386),
        Vec3::new(-0.060, -0.252, -0.381),
        Vec3::new(-0.078, -0.243, -0.368),
        Vec3::new(-0.101, -0.235, -0.354),
        Vec3::new(-0.126, -0.226, -0.341),
        Vec3::new(-0.150, -0.221, -0.329),
        Vec3::new(-0.173, -0.217, -0.317),
        Vec3::new(-0.191, -0.216, -0.307),
        Vec3::new(-0.205, -0.215, -0.297),
        Vec3::new(-0.216, -0.216, -0.288),
        Vec3::new(-0.221, -0.217, -0.282),
        Vec3::new(-0.225, -0.219, -0.277),
    ];
    const SEATED_AIM: Vec3 = Vec3::new(0.51, -0.46, -0.73);
    const SEATED_HEAD: Vec3 = Vec3::new(0.07, -0.046, 0.0);
    const SEATED_DT: f32 = 0.0266;

    fn fires(reaches: impl Iterator<Item = Option<f32>>) -> bool {
        let config = SnatchConfig::new().yank;
        let mut detector = YankDetector::new();
        reaches
            .map(|reach| detector.step(SEATED_DT, reach, true, &config))
            .any(|yank| yank.is_some())
    }

    #[test]
    fn a_seated_pull_past_the_side_is_a_yank_along_the_aim() {
        assert!(fires(
            SEATED_PULL
                .iter()
                .map(|&grip| reach_along(grip, SEATED_AIM))
        ));
        // The same pull keeps its distance from a point below the head, which is why the body's
        // distance is not the snatch's measure.
        let config = SnatchConfig::new().yank;
        let body = body_point(SEATED_HEAD, &config);
        assert!(!fires(
            SEATED_PULL.iter().map(|&grip| Some(grip.distance(body)))
        ));
    }

    #[test]
    fn a_reach_toward_the_target_or_across_the_aim_is_no_yank() {
        let toward = SEATED_PULL
            .iter()
            .rev()
            .map(|&grip| reach_along(grip, SEATED_AIM));
        assert!(!fires(toward));
        let across = (0..12).map(|k| {
            let grip = Vec3::new(0.0, -0.25, -0.4) + Vec3::X * 0.03 * k as f32;
            reach_along(grip, Vec3::NEG_Z)
        });
        assert!(!fires(across));
    }

    #[test]
    fn a_line_with_no_direction_measures_nothing() {
        assert_eq!(reach_along(Vec3::ONE, Vec3::ZERO), None);
        assert_eq!(reach_along(Vec3::ONE, Vec3::NAN), None);
        assert_eq!(
            reach_along(Vec3::new(0.0, 0.0, -0.4), Vec3::NEG_Z),
            Some(0.4)
        );
    }

    fn ray() -> Ray {
        Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap()
    }

    fn at(kind: u32, point: Vec3) -> Candidate<u32> {
        Candidate { kind, point }
    }

    #[test]
    fn the_aim_picks_the_candidate_nearest_its_line() {
        let cone = 6f32.to_radians();
        let chosen = choose(
            &ray(),
            [
                at(1, Vec3::new(0.5, 0.0, -10.0)),
                at(2, Vec3::new(0.1, 0.0, -10.0)),
                at(3, Vec3::new(0.0, 0.0, 10.0)),
            ],
            |_, _| cone,
            20.0,
        )
        .unwrap();
        assert_eq!(chosen.candidate.kind, 2);
        assert!((chosen.distance - 10.0).abs() < 0.01);
    }

    #[test]
    fn the_aim_ignores_what_is_out_of_the_cone_or_the_range() {
        let cone = |_: &u32, _: f32| 6f32.to_radians();
        assert_eq!(
            choose(&ray(), [at(1, Vec3::new(3.0, 0.0, -10.0))], cone, 20.0),
            None,
            "17 degrees off the ray"
        );
        assert_eq!(
            choose(&ray(), [at(1, Vec3::new(0.0, 0.0, -25.0))], cone, 20.0),
            None,
            "beyond the range"
        );
        assert_eq!(
            choose(&ray(), [at(1, Vec3::new(f32::NAN, 0.0, -5.0))], cone, 20.0),
            None
        );
        assert_eq!(choose(&ray(), [at(1, Vec3::ZERO)], cone, 20.0), None);
    }

    #[test]
    fn of_two_on_the_line_the_nearer_wins() {
        let chosen = choose(
            &ray(),
            [
                at(1, Vec3::new(0.0, 0.0, -12.0)),
                at(2, Vec3::new(0.0, 0.0, -4.0)),
            ],
            |_, _| 0.1,
            20.0,
        )
        .unwrap();
        assert_eq!(chosen.candidate.kind, 2);
    }

    #[test]
    fn a_far_weapon_on_the_ground_is_chosen_off_the_ray_where_a_near_one_is_not() {
        let (base, widen, range) = (6f32.to_radians(), 4f32.to_radians(), 20.0);
        let cone = |_: &u32, distance: f32| floor_cone(distance, base, widen, range);
        // Eight degrees off the ray: outside the base cone, inside the cone widened at range.
        let off = |distance: f32| {
            let angle = 8f32.to_radians();
            Vec3::new(angle.sin(), 0.0, -angle.cos()) * distance
        };
        assert_eq!(
            choose(&ray(), [at(1, off(18.0))], cone, range).map(|c| c.candidate.kind),
            Some(1)
        );
        assert_eq!(choose(&ray(), [at(1, off(3.0))], cone, range), None);
    }

    #[test]
    fn the_floor_cone_widens_with_distance_and_stops_at_the_range() {
        let (base, widen) = (0.1, 0.05);
        assert_eq!(floor_cone(0.0, base, widen, 20.0), base);
        assert!((floor_cone(10.0, base, widen, 20.0) - 0.125).abs() < 1e-6);
        assert_eq!(floor_cone(40.0, base, widen, 20.0), base + widen);
        assert_eq!(floor_cone(f32::NAN, base, widen, 20.0), base);
        assert_eq!(floor_cone(10.0, base, widen, 0.0), base);
        assert_eq!(floor_cone(10.0, base, -1.0, 20.0), base);
    }

    #[test]
    fn the_pull_rises_far_out_and_comes_straight_in_near() {
        let hand = Vec3::new(0.0, 1.0, 0.0);
        let far = pull_point(Vec3::new(0.0, 0.0, -10.0), hand, 0.25);
        assert!(far.y > hand.y + 2.0);
        let near = pull_point(hand + Vec3::new(0.0, 0.0, -0.1), hand, 0.25);
        assert!((near.y - hand.y) < 0.05);
    }

    #[test]
    fn the_pull_arrives_within_the_capture_radius() {
        let limits = PullLimits {
            capture_radius_m: 0.4,
            line_m: 20.0,
        };
        assert_eq!(verdict(5.0, false, &limits), Verdict::Pulling);
        assert_eq!(verdict(0.3, false, &limits), Verdict::Arrived);
        assert_eq!(
            verdict(0.3, true, &limits),
            Verdict::Arrived,
            "arrival wins over a stall"
        );
        assert_eq!(verdict(5.0, true, &limits), Verdict::Stalled);
        assert_eq!(verdict(25.0, false, &limits), Verdict::OutOfLine);
        assert_eq!(verdict(f32::NAN, false, &limits), Verdict::OutOfLine);
    }

    #[test]
    fn a_body_that_barely_moves_for_the_window_stalls() {
        let mut watch = StallWatch::default();
        let dt = 1.0 / 30.0;
        let mut stalled = false;
        for i in 0..30 {
            let p = Vec3::new(0.0, 0.0, -5.0 + i as f32 * 0.001);
            stalled = watch.step(dt, p, 0.5, 0.1);
        }
        assert!(stalled);
    }

    #[test]
    fn a_moving_body_never_stalls_and_a_short_watch_does_not_judge() {
        let mut watch = StallWatch::default();
        let dt = 1.0 / 30.0;
        for i in 0..60 {
            let p = Vec3::new(0.0, 0.0, -10.0 + i as f32 * 0.2);
            assert!(!watch.step(dt, p, 0.5, 0.1), "tick {i}");
        }
        let mut fresh = StallWatch::default();
        assert!(!fresh.step(dt, Vec3::ZERO, 0.5, 0.1), "no window yet");
    }

    #[test]
    fn a_toss_lands_where_it_was_aimed() {
        let gravity = Vec3::new(0.0, -9.81, 0.0);
        let from = Vec3::new(0.0, 0.2, -8.0);
        let to = Vec3::new(0.0, 0.0, -1.0);
        let t = 0.8;
        let v = toss_velocity(from, to, t, gravity);
        let landed = from + v * t + gravity * t * t * 0.5;
        assert!(landed.distance(to) < 1e-4);
        assert!(v.y > 0.0, "a toss goes up first");
    }

    #[test]
    fn the_toss_flight_is_kept_within_its_bounds() {
        assert!((toss_flight(8.0, 10.0, 0.4, 1.2) - 0.8).abs() < 1e-6);
        assert_eq!(toss_flight(0.1, 10.0, 0.4, 1.2), 0.4);
        assert_eq!(toss_flight(100.0, 10.0, 0.4, 1.2), 1.2);
        assert_eq!(toss_flight(5.0, 0.0, 0.4, 1.2), 1.2);
    }
}
