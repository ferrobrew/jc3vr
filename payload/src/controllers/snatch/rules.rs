//! The snatch's pure rules: which target the grapple's aim picks, what a loose object is to it by
//! its mass, where and how fast the line pulls
//! the target, when the line goes slack and which hand catches it or whether it is missed, when it
//! has stalled or run out of line, and the toss that a tap at a gun gives it.

use glam::Vec3;

use crate::controllers::{aim::ray::Ray, frame::Hand};

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
/// the ray's origin, or with a point that is not finite, is never chosen ([`Ray::offset`]).
pub fn choose<K: Copy>(
    ray: &Ray,
    candidates: impl IntoIterator<Item = Candidate<K>>,
    cone: impl Fn(&K, f32) -> f32,
    range: f32,
) -> Option<Chosen<K>> {
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let offset = ray.offset(candidate.point)?;
            offset
                .within(range, cone(&candidate.kind, offset.distance))
                .then_some(Chosen {
                    candidate,
                    distance: offset.distance,
                    angle: offset.angle,
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
/// the ground. The rise fades to nothing over the last `approach` metres, so the final approach
/// aims at the hand itself.
pub fn pull_point(body: Vec3, hand: Vec3, arc: f32, approach: f32) -> Vec3 {
    let distance = body.distance(hand);
    if !distance.is_finite() {
        return hand;
    }
    let fade = if approach > 0.0 {
        (distance / approach).min(1.0)
    } else {
        1.0
    };
    hand + Vec3::Y * distance * arc.max(0.0) * fade
}

/// The speed the line asks of the target `distance` metres from the hand: `far` beyond
/// `approach`, falling in proportion to the distance to `arrival` at the hand, never more than
/// `far`.
pub fn line_speed(distance: f32, far: f32, arrival: f32, approach: f32) -> f32 {
    let far = far.max(0.0);
    let arrival = arrival.clamp(0.0, far);
    if !(approach > 0.0 && distance.is_finite()) {
        return far;
    }
    let share = (distance.max(0.0) / approach).min(1.0);
    arrival + (far - arrival) * share
}

/// How near the hand the line goes slack for a target moving at `speed` metres per second: the
/// distance it covers in `coast_s` seconds, kept between `min` and `max` metres. A slow target,
/// one a step away that the line has had little time to speed up, is guided nearer the hand
/// before it flies on by itself.
pub fn slack_radius(speed: f32, coast_s: f32, min: f32, max: f32) -> f32 {
    let max = max.max(min);
    let radius = speed.max(0.0) * coast_s.max(0.0);
    if radius.is_finite() {
        radius.clamp(min, max)
    } else {
        max
    }
}

/// What a tick of the flight finds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The line pulls the target toward the hand.
    Pulling,
    /// The line has gone slack near the hand, and the target flies on by its own momentum.
    Slack,
    /// The target passed through this hand's catch this tick.
    Caught(Hand),
    /// The target flew past the hand, or stopped short of it, without being caught.
    Missed,
    /// The target has moved too little for a while: snagged on the world.
    Stalled,
    /// The target is farther from the hand than the line reaches.
    OutOfLine,
}

/// The flight's limits.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FlightLimits {
    /// How near the hand the line goes slack, metres.
    pub slack_radius_m: f32,
    /// How near a catching hand the target's centre must pass to be caught, metres.
    pub catch_radius_m: f32,
    /// How far the line reaches, metres.
    pub line_m: f32,
    /// How long the target may fly slack before it counts as missed, seconds.
    pub max_slack_s: f32,
}

/// One tick of the target's flight.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FlightSample {
    /// The target's centre at the last tick, or now on the first.
    pub from: Vec3,
    /// The target's centre now.
    pub to: Vec3,
    /// The target's velocity now.
    pub velocity: Vec3,
    /// The hand the line pulls toward, now.
    pub hand: Vec3,
    /// The palms that may catch the target now, by [`Hand::index`]: a free hand whose grip is
    /// held.
    pub catchers: [Option<Vec3>; 2],
    /// How long the line has been slack, seconds, or `None` while it pulls.
    pub slack_s: Option<f32>,
}

/// The verdict for a tick of the flight, with `stalled` whether the stall watch has fired.
///
/// The catch is swept: a hand catches the target when the target's centre passed within the catch
/// radius of its palm anywhere along its path since the last tick, so a fast target is not stepped
/// over, and of two hands that qualify the one it passed nearer catches it. Nothing draws it in:
/// once the line is slack the target flies on, and it is missed once it recedes from the line's
/// hand and from every catching hand.
pub fn verdict(sample: &FlightSample, stalled: bool, limits: &FlightLimits) -> Verdict {
    let distance = sample.to.distance(sample.hand);
    if distance.is_nan() {
        return Verdict::OutOfLine;
    }
    let caught = Hand::BOTH
        .into_iter()
        .filter_map(|hand| {
            let palm = sample.catchers[hand.index()]?;
            let pass = closest_approach(sample.from, sample.to, palm);
            (pass <= limits.catch_radius_m).then_some((hand, pass))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((hand, _)) = caught {
        return Verdict::Caught(hand);
    }
    if let Some(slack_s) = sample.slack_s {
        let receding = |palm: Vec3| (palm - sample.to).dot(sample.velocity) <= 0.0;
        let receding =
            receding(sample.hand) && sample.catchers.iter().flatten().all(|&p| receding(p));
        let too_long = slack_s.is_nan() || slack_s > limits.max_slack_s;
        return if stalled || receding || too_long {
            Verdict::Missed
        } else {
            Verdict::Slack
        };
    }
    if distance > limits.line_m {
        Verdict::OutOfLine
    } else if stalled {
        Verdict::Stalled
    } else if distance <= limits.slack_radius_m {
        Verdict::Slack
    } else {
        Verdict::Pulling
    }
}

/// The least distance from `point` to the segment from `from` to `to`.
pub fn closest_approach(from: Vec3, to: Vec3, point: Vec3) -> f32 {
    let segment = to - from;
    let length_squared = segment.length_squared();
    let t = if length_squared > f32::EPSILON {
        ((point - from).dot(segment) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (from + segment * t).distance(point)
}

/// The velocity the line asks of the point `at` on the target: `speed` metres per second toward
/// `toward`.
pub fn line_velocity(at: Vec3, toward: Vec3, speed: f32) -> Vec3 {
    (toward - at).try_normalize().unwrap_or(Vec3::ZERO) * speed.max(0.0)
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

/// What the snatch does with a loose object.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PropHandling {
    /// Pulled into the hand and caught: a hand can carry it.
    Carry,
    /// Swung on the held line, never brought to the hand: too heavy to carry, but within the
    /// swing's ceiling.
    Swing,
}

/// What the snatch does with a loose object of `mass_kg`: carried up to `carry_max_kg`, swung
/// above it up to `ceiling_kg`, and not offered above the ceiling or without a finite, positive
/// mass. A ceiling below the carry limit swings nothing.
pub fn prop_handling(mass_kg: f32, carry_max_kg: f32, ceiling_kg: f32) -> Option<PropHandling> {
    if !(mass_kg.is_finite() && mass_kg > 0.0) {
        None
    } else if mass_kg <= carry_max_kg {
        Some(PropHandling::Carry)
    } else if mass_kg <= ceiling_kg {
        Some(PropHandling::Swing)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{
        bindings::yank::{YankDetector, reach_along},
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
        let body = SEATED_HEAD - Vec3::Y * 0.3;
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
        let far = pull_point(Vec3::new(0.0, 0.0, -10.0), hand, 0.25, 2.5);
        assert!(far.y > hand.y + 2.0);
        let near = pull_point(hand + Vec3::new(0.0, 0.0, -0.1), hand, 0.25, 2.5);
        assert!((near.y - hand.y) < 0.05);
    }

    #[test]
    fn the_rise_fades_over_the_final_approach() {
        let hand = Vec3::ZERO;
        let at = |d: f32| pull_point(Vec3::new(0.0, 0.0, -d), hand, 0.08, 2.5).y;
        assert!(
            (at(5.0) - 0.4).abs() < 1e-5,
            "the full rise beyond the approach"
        );
        assert!((at(2.5) - 0.2).abs() < 1e-5, "the full rise at its start");
        assert!((at(1.0) - 0.032).abs() < 1e-5, "a fraction within it");
        assert!(at(1.0) < 1.0 * 0.08, "lower than the rise without the fade");
        assert!((pull_point(Vec3::new(0.0, 0.0, -1.0), hand, 0.08, 0.0).y - 0.08).abs() < 1e-5);
    }

    #[test]
    fn the_line_slows_over_the_final_approach_to_its_arrival_speed() {
        assert_eq!(line_speed(10.0, 18.0, 12.0, 2.5), 18.0);
        assert_eq!(line_speed(2.5, 18.0, 12.0, 2.5), 18.0);
        assert!((line_speed(1.25, 18.0, 12.0, 2.5) - 15.0).abs() < 1e-5);
        assert_eq!(line_speed(0.0, 18.0, 12.0, 2.5), 12.0);
        assert_eq!(
            line_speed(1.0, 18.0, 30.0, 2.5),
            18.0,
            "never faster than its far speed"
        );
        assert_eq!(line_speed(f32::NAN, 18.0, 12.0, 2.5), 18.0);
        assert_eq!(line_speed(1.0, 18.0, 12.0, 0.0), 18.0);
    }

    #[test]
    fn the_line_goes_slack_a_couple_of_ticks_out_whatever_the_speed() {
        assert!((slack_radius(12.0, 0.07, 0.4, 1.0) - 0.84).abs() < 1e-5);
        assert_eq!(
            slack_radius(18.0, 0.07, 0.4, 1.0),
            1.0,
            "a fast target from its farthest"
        );
        assert_eq!(
            slack_radius(3.0, 0.07, 0.4, 1.0),
            0.4,
            "a slow one guided nearer"
        );
        assert_eq!(slack_radius(f32::INFINITY, 0.07, 0.4, 1.0), 1.0);
        assert_eq!(slack_radius(-5.0, 0.07, 0.4, 1.0), 0.4);
    }

    const LIMITS: FlightLimits = FlightLimits {
        slack_radius_m: 1.0,
        catch_radius_m: 0.25,
        line_m: 20.0,
        max_slack_s: 0.5,
    };

    /// A tick of a flight toward the left hand at the origin, which is the one catching hand.
    fn sample(from: Vec3, to: Vec3, velocity: Vec3, slack_s: Option<f32>) -> FlightSample {
        FlightSample {
            from,
            to,
            velocity,
            hand: Vec3::ZERO,
            catchers: [Some(Vec3::ZERO), None],
            slack_s,
        }
    }

    #[test]
    fn the_line_pulls_until_the_target_nears_the_hand_then_goes_slack() {
        let v = Vec3::new(0.0, 0.0, 18.0);
        let far = sample(
            Vec3::new(0.0, 0.0, -6.0),
            Vec3::new(0.0, 0.0, -5.4),
            v,
            None,
        );
        assert_eq!(verdict(&far, false, &LIMITS), Verdict::Pulling);
        let near = sample(
            Vec3::new(0.0, 0.0, -1.5),
            Vec3::new(0.0, 0.0, -0.9),
            v,
            None,
        );
        assert_eq!(verdict(&near, false, &LIMITS), Verdict::Slack);
        assert_eq!(verdict(&far, true, &LIMITS), Verdict::Stalled);
        let beyond = sample(
            Vec3::new(0.0, 0.0, -25.0),
            Vec3::new(0.0, 0.0, -24.0),
            v,
            None,
        );
        assert_eq!(verdict(&beyond, false, &LIMITS), Verdict::OutOfLine);
        let lost = sample(Vec3::ZERO, Vec3::new(f32::NAN, 0.0, 0.0), v, None);
        assert_eq!(verdict(&lost, false, &LIMITS), Verdict::OutOfLine);
    }

    #[test]
    fn a_fast_target_is_caught_where_its_path_crosses_the_hand() {
        // Sixty centimetres a tick: the target is never within the catch radius at a tick, but its
        // path passes through the hand.
        let v = Vec3::new(0.0, 0.0, 18.0);
        let through = sample(
            Vec3::new(0.05, 0.0, -0.3),
            Vec3::new(0.05, 0.0, 0.3),
            v,
            Some(0.02),
        );
        assert_eq!(
            verdict(&through, false, &LIMITS),
            Verdict::Caught(Hand::Left)
        );
        let pulled = sample(Vec3::new(0.1, 0.0, -0.4), Vec3::new(0.1, 0.0, 0.2), v, None);
        assert_eq!(
            verdict(&pulled, false, &LIMITS),
            Verdict::Caught(Hand::Left)
        );
    }

    #[test]
    fn a_slack_target_the_hand_is_not_on_is_missed() {
        let v = Vec3::new(0.0, 0.0, 18.0);
        let wide = sample(
            Vec3::new(0.5, 0.0, -0.3),
            Vec3::new(0.5, 0.0, 0.3),
            v,
            Some(0.05),
        );
        assert_eq!(
            verdict(&wide, false, &LIMITS),
            Verdict::Missed,
            "past the hand"
        );
        let coming = sample(
            Vec3::new(0.5, 0.0, -0.9),
            Vec3::new(0.5, 0.0, -0.6),
            v,
            Some(0.05),
        );
        assert_eq!(
            verdict(&coming, false, &LIMITS),
            Verdict::Slack,
            "still on its way"
        );
        assert_eq!(
            verdict(&coming, true, &LIMITS),
            Verdict::Missed,
            "stopped short"
        );
        let late = sample(
            Vec3::new(0.5, 0.0, -0.9),
            Vec3::new(0.5, 0.0, -0.6),
            v,
            Some(0.6),
        );
        assert_eq!(
            verdict(&late, false, &LIMITS),
            Verdict::Missed,
            "flew too long"
        );
    }

    #[test]
    fn the_other_hand_catches_a_target_on_its_way_to_the_line_hand() {
        // The line pulls toward the left hand at the origin; the right hand, its grip held, is put
        // in the target's path twenty centimetres short of it.
        let v = Vec3::new(0.0, 0.0, 12.0);
        let mut tick = sample(
            Vec3::new(0.3, 0.0, -0.6),
            Vec3::new(0.3, 0.0, -0.2),
            v,
            Some(0.03),
        );
        tick.catchers = [Some(Vec3::ZERO), Some(Vec3::new(0.3, 0.0, -0.3))];
        assert_eq!(verdict(&tick, false, &LIMITS), Verdict::Caught(Hand::Right));
    }

    #[test]
    fn of_two_hands_on_the_path_the_nearer_palm_catches() {
        let v = Vec3::new(0.0, 0.0, 12.0);
        let mut tick = sample(
            Vec3::new(0.0, 0.0, -0.4),
            Vec3::new(0.0, 0.0, 0.2),
            v,
            Some(0.03),
        );
        tick.catchers = [
            Some(Vec3::new(0.15, 0.0, 0.0)),
            Some(Vec3::new(-0.05, 0.0, -0.1)),
        ];
        assert_eq!(verdict(&tick, false, &LIMITS), Verdict::Caught(Hand::Right));
        tick.catchers = [
            Some(Vec3::new(0.02, 0.0, 0.0)),
            Some(Vec3::new(-0.2, 0.0, -0.1)),
        ];
        assert_eq!(verdict(&tick, false, &LIMITS), Verdict::Caught(Hand::Left));
    }

    #[test]
    fn a_target_past_the_line_hand_is_not_missed_while_it_still_nears_another_catcher() {
        let v = Vec3::new(0.0, 0.0, 12.0);
        let mut tick = sample(
            Vec3::new(0.6, 0.0, -0.2),
            Vec3::new(0.6, 0.0, 0.2),
            v,
            Some(0.05),
        );
        tick.catchers = [Some(Vec3::ZERO), Some(Vec3::new(0.6, 0.0, 0.8))];
        assert_eq!(verdict(&tick, false, &LIMITS), Verdict::Slack);
        tick.catchers = [Some(Vec3::ZERO), None];
        assert_eq!(verdict(&tick, false, &LIMITS), Verdict::Missed);
    }

    #[test]
    fn a_hand_that_is_not_free_and_gripping_never_catches() {
        let v = Vec3::new(0.0, 0.0, 12.0);
        let mut tick = sample(
            Vec3::new(0.0, 0.0, -0.3),
            Vec3::new(0.0, 0.0, 0.3),
            v,
            Some(0.02),
        );
        tick.catchers = [None, None];
        assert_eq!(verdict(&tick, false, &LIMITS), Verdict::Missed);
    }

    #[test]
    fn the_closest_approach_is_measured_along_the_segment() {
        let (from, to) = (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        assert!((closest_approach(from, to, Vec3::new(0.0, 0.3, 0.0)) - 0.3).abs() < 1e-6);
        assert!((closest_approach(from, to, Vec3::new(2.0, 0.0, 0.0)) - 1.0).abs() < 1e-6);
        assert!((closest_approach(from, from, Vec3::new(-1.0, 0.5, 0.0)) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_line_pulls_at_its_speed_whatever_the_distance() {
        let far = line_velocity(Vec3::new(0.0, 0.0, -10.0), Vec3::ZERO, 18.0);
        let near = line_velocity(Vec3::new(0.0, 0.0, -1.0), Vec3::ZERO, 18.0);
        assert!((far.z - 18.0).abs() < 1e-5);
        assert_eq!(far, near, "no easing in near the hand");
        assert_eq!(line_velocity(Vec3::ZERO, Vec3::ZERO, 18.0), Vec3::ZERO);
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

    #[test]
    fn a_loose_object_is_carried_swung_or_not_offered_by_its_mass() {
        let at = |mass| prop_handling(mass, 35.0, 400.0);
        assert_eq!(at(4.0), Some(PropHandling::Carry));
        assert_eq!(at(35.0), Some(PropHandling::Carry));
        assert_eq!(at(35.5), Some(PropHandling::Swing));
        assert_eq!(at(400.0), Some(PropHandling::Swing));
        assert_eq!(at(400.5), None);
        assert_eq!(at(0.0), None);
        assert_eq!(at(f32::NAN), None);
        assert_eq!(at(f32::INFINITY), None);
        assert_eq!(
            prop_handling(100.0, 35.0, 20.0),
            None,
            "a low ceiling swings nothing"
        );
    }
}
