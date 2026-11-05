//! The pure parts of a snatched gun's aligned landing: over its final approach the flying body is
//! steered so that it reaches the catching hand already in the pose the hand will hold it in, and
//! the arrival's slide into the hand has only what is left to close.
//!
//! The rotation is a terminal-time drive on the body's angular velocity ([`turn_drive`]): each tick
//! it asks for the hand's own turn rate plus the turn still to make over the time left until the
//! catch, so its gain grows as the catch nears and it closes the turn by then, held to a spin cap
//! and an angular acceleration cap. The position is
//! corrected only while the line pulls, and only sideways to the line by a capped shift of its aim
//! ([`aim_shift`]), so the gun's grip rather than its centre heads for the palm, and a hand that
//! moves away after the line goes slack still misses. The catching hand is the free hand the path passes nearer, with hysteresis
//! ([`catching_hand`]).

use body_rig::rotation::{angular_velocity, rotation_error};
use glam::{Quat, Vec3};

use crate::controllers::frame::Hand;

/// The landing's tuning.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LandingTuning {
    /// How far into the final approach the drive takes to come to full strength, metres.
    pub ramp_m: f32,
    /// How many ticks the drive takes to come to full strength at most, for a snatch that bites
    /// inside the final approach: the first tick counts as one.
    pub ramp_ticks: f32,
    /// The least and the most time to the catch the drive works to, seconds.
    pub min_t_go_s: f32,
    pub max_t_go_s: f32,
    /// The fastest the drive spins the body, radians per second.
    pub max_spin_rps: f32,
    /// The hardest the drive turns the body, radians per second squared.
    pub max_angular_accel: f32,
    /// The farthest the line's aim moves sideways off the palm, metres.
    pub lateral_window_m: f32,
    /// How much nearer the other hand must be to the path to take the catch over, metres.
    pub hysteresis_m: f32,
    /// How far ahead the hand's pose is extrapolated by its motion, at most, seconds.
    pub max_extrapolate_s: f32,
    /// The residual below which the arrival puts the gun in the hand at once: an angle, radians,
    /// and a distance, metres, both under.
    pub settled_rad: f32,
    pub settled_m: f32,
    /// The residual angle at which the arrival takes its full slide, radians.
    pub full_slide_rad: f32,
}

/// A catching hand's distance from the target's path, for the choice of hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandOffer {
    /// How far the hand's grip point is from the target's path, metres.
    pub miss_m: f32,
}

/// The hand that catches: of the hands offered (by [`Hand::index`]), the one the target's path
/// passes nearer, keeping `current` unless the other is nearer by `hysteresis` metres or more.
/// `fallback` when no hand is offered.
pub fn catching_hand(
    current: Option<Hand>,
    offers: [Option<HandOffer>; 2],
    hysteresis: f32,
    fallback: Hand,
) -> Hand {
    let nearest = Hand::BOTH
        .into_iter()
        .filter_map(|hand| offers[hand.index()].map(|o| (hand, o.miss_m)))
        .filter(|(_, miss)| miss.is_finite())
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((nearest, nearest_miss)) = nearest else {
        return current.unwrap_or(fallback);
    };
    match current.and_then(|hand| offers[hand.index()].map(|o| (hand, o.miss_m))) {
        Some((hand, miss)) if miss.is_finite() && miss - nearest_miss < hysteresis.max(0.0) => hand,
        _ => nearest,
    }
}

/// How far `point` is from the path a body at `at` flying at `velocity` takes ahead of it, metres:
/// its distance from the ray, or from `at` for a point behind it or a body at rest.
pub fn miss_distance(at: Vec3, velocity: Vec3, point: Vec3) -> f32 {
    let Some(direction) = velocity.try_normalize() else {
        return at.distance(point);
    };
    let along = (point - at).dot(direction).max(0.0);
    (at + direction * along).distance(point)
}

/// The time left until a body at `at` flying at `velocity` comes within `reach` metres of
/// `target`, where the catch takes it, seconds: the distance left over the speed it closes at,
/// held between [`LandingTuning::min_t_go_s`] and [`LandingTuning::max_t_go_s`], and the most
/// while it does not close.
pub fn time_to_go(
    at: Vec3,
    velocity: Vec3,
    target: Vec3,
    reach: f32,
    tuning: &LandingTuning,
) -> f32 {
    let offset = target - at;
    let left = (offset.length() - reach.max(0.0)).max(0.0);
    let closing = offset.try_normalize().map_or(0.0, |d| velocity.dot(d));
    let t_go = if closing > f32::EPSILON && left.is_finite() {
        left / closing
    } else {
        tuning.max_t_go_s
    };
    t_go.clamp(tuning.min_t_go_s, tuning.max_t_go_s.max(tuning.min_t_go_s))
}

/// An orientation and the rate it turns at, radians per second.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Turning {
    pub orientation: Quat,
    pub angular: Vec3,
}

/// The body's angular velocity after a tick of `dt` seconds of the turn drive: the body turning as
/// `body` says, toward `target`, `t_go` seconds before the catch. It asks for the target's turn
/// rate plus the turn left over the time left, so the turn is closed by the catch and the body then
/// turns with the hand; the change is held to [`LandingTuning::max_angular_accel`] and the result
/// to [`LandingTuning::max_spin_rps`].
pub fn turn_drive(
    body: Turning,
    target: Turning,
    t_go: f32,
    dt: f32,
    tuning: &LandingTuning,
) -> Vec3 {
    let t_go = t_go.max(tuning.min_t_go_s).max(dt);
    let error = rotation_error(body.orientation, target.orientation);
    let wanted = target.angular + error / t_go;
    let step = (wanted - body.angular).clamp_length_max(tuning.max_angular_accel.max(0.0) * dt);
    let next = (body.angular + step).clamp_length_max(tuning.max_spin_rps.max(0.0));
    if next.is_finite() { next } else { body.angular }
}

/// How far the line's aim moves off the catching hand's palm `palm`, for a target that flies along
/// `line` (unit length) and whose centre the hand will hold at `target`: the part of the offset
/// across the line, held to [`LandingTuning::lateral_window_m`], scaled by the drive's `weight`.
/// The line then carries the centre to where the hand will hold it rather than to the palm, so the
/// gun's grip, not its centre, heads for the hand; the line's own acceleration cap holds how fast.
pub fn aim_shift(
    palm: Vec3,
    target: Vec3,
    line: Vec3,
    weight: f32,
    tuning: &LandingTuning,
) -> Vec3 {
    let offset = target - palm;
    let across = offset - line * offset.dot(line);
    let shift = across.clamp_length_max(tuning.lateral_window_m.max(0.0)) * weight.clamp(0.0, 1.0);
    if shift.is_finite() { shift } else { Vec3::ZERO }
}

/// The drive's strength, from 0 to 1: it comes on over [`LandingTuning::ramp_m`] past where it
/// started (`start_m` from the hand, `distance_m` now), or over
/// [`LandingTuning::ramp_ticks`] ticks (`ticks` since it started), whichever is sooner.
pub fn drive_weight(start_m: f32, distance_m: f32, ticks: u32, tuning: &LandingTuning) -> f32 {
    let by_distance = if tuning.ramp_m > 0.0 {
        (start_m - distance_m) / tuning.ramp_m
    } else {
        1.0
    };
    let by_ticks = if tuning.ramp_ticks > 0.0 {
        ticks as f32 / tuning.ramp_ticks
    } else {
        1.0
    };
    let weight = by_distance.max(by_ticks);
    if weight.is_finite() {
        weight.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// How long the arrival slides the gun into the hand, seconds, for a residual of `angle` radians
/// and `distance` metres between where the gun arrived and where the hand holds it, the gun
/// arriving at `speed` metres per second: none once both are under the settled thresholds;
/// otherwise the longer of `slide_s` in proportion to the angle's share of the full-slide angle and
/// the time the gun takes to fly the distance left, never more than `slide_s`.
pub fn arrival_slide(
    angle: f32,
    distance: f32,
    speed: f32,
    slide_s: f32,
    tuning: &LandingTuning,
) -> f32 {
    if !(angle.is_finite() && distance.is_finite()) {
        return slide_s;
    }
    if angle < tuning.settled_rad && distance < tuning.settled_m {
        return 0.0;
    }
    let by_angle = if tuning.full_slide_rad > 0.0 {
        slide_s * angle / tuning.full_slide_rad
    } else {
        slide_s
    };
    let by_flight = distance / speed.max(1.0);
    by_angle.max(by_flight).clamp(0.0, slide_s)
}

/// Extrapolate a pose from `last` (seen `dt` seconds before) to `now`, `ahead` seconds on, held to
/// `max_ahead`: the position by its velocity and the rotation by its turn rate. Returns the pose's
/// position, rotation, and angular velocity.
pub fn extrapolate(
    last: Option<(Vec3, Quat)>,
    now: (Vec3, Quat),
    dt: f32,
    ahead: f32,
    max_ahead: f32,
) -> (Vec3, Quat, Vec3) {
    let Some((last_position, last_rotation)) = last.filter(|_| dt > f32::EPSILON) else {
        return (now.0, now.1, Vec3::ZERO);
    };
    let velocity = (now.0 - last_position) / dt;
    let angular = angular_velocity(last_rotation, now.1, dt).unwrap_or(Vec3::ZERO);
    let ahead = ahead.clamp(0.0, max_ahead.max(0.0));
    let turn = angular * ahead;
    let rotation = match turn.try_normalize() {
        Some(axis) => (Quat::from_axis_angle(axis, turn.length()) * now.1).normalize(),
        None => now.1,
    };
    let (velocity, angular) = if velocity.is_finite() && angular.is_finite() {
        (velocity, angular)
    } else {
        (Vec3::ZERO, Vec3::ZERO)
    };
    (now.0 + velocity * ahead, rotation, angular)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::controllers::snatch::config::SNATCH;

    const DT: f32 = 1.0 / 30.0;

    fn turning(orientation: Quat, angular: Vec3) -> Turning {
        Turning {
            orientation,
            angular,
        }
    }

    fn still(orientation: Quat) -> Turning {
        turning(orientation, Vec3::ZERO)
    }

    fn tuning() -> LandingTuning {
        SNATCH.landing
    }

    /// Integrate the turn drive from `orientation` spinning at `spin` toward a still `target`
    /// arriving in `ticks` ticks, returning the angle left at the arrival, radians.
    fn residual(orientation: Quat, spin: Vec3, target: Quat, ticks: u32) -> f32 {
        let mut q = orientation;
        let mut w = spin;
        for tick in 0..ticks {
            let t_go = (ticks - tick) as f32 * DT;
            w = turn_drive(turning(q, w), still(target), t_go, DT, &tuning());
            if let Some(axis) = w.try_normalize() {
                q = (Quat::from_axis_angle(axis, w.length() * DT) * q).normalize();
            }
        }
        rotation_error(q, target).length()
    }

    #[test]
    fn the_drive_lands_a_half_turn_in_the_ticks_of_a_short_snatch() {
        // A snatch from a metre and a half flies about four ticks.
        let target = Quat::from_rotation_y(0.3);
        let from = Quat::from_rotation_x(std::f32::consts::PI * 0.97) * target;
        assert!(residual(from, Vec3::ZERO, target, 4) < 10f32.to_radians());
    }

    #[test]
    fn the_drive_takes_out_the_line_s_spin_on_the_way() {
        let target = Quat::IDENTITY;
        for spin in [Vec3::X * 25.0, Vec3::new(10.0, -18.0, 5.0)] {
            let left = residual(Quat::from_rotation_z(1.0), spin, target, 4);
            assert!(left < 10f32.to_radians(), "{spin}: {}", left.to_degrees());
        }
    }

    #[test]
    fn the_drive_closes_a_small_turn_in_a_tick_and_holds_to_its_caps() {
        let target = Quat::IDENTITY;
        let w = turn_drive(
            still(Quat::from_rotation_y(0.02)),
            still(target),
            DT,
            DT,
            &tuning(),
        );
        // A small error at the catch is closed in the tick, not passed.
        assert!((w.length() * DT - 0.02).abs() < 1e-4, "{w}");
        let far = turn_drive(
            turning(Quat::from_rotation_y(3.0), Vec3::NEG_Y * 30.0),
            still(target),
            0.05,
            DT,
            &tuning(),
        );
        assert!(far.length() <= tuning().max_spin_rps + 1e-4);
        let step = far - Vec3::NEG_Y * 30.0;
        assert!(step.length() <= tuning().max_angular_accel * DT + 1e-3);
    }

    #[test]
    fn the_drive_follows_a_turning_hand() {
        let target_rate = Vec3::Y * 3.0;
        let w = turn_drive(
            turning(Quat::IDENTITY, target_rate),
            turning(Quat::IDENTITY, target_rate),
            0.2,
            DT,
            &tuning(),
        );
        assert!(w.distance(target_rate) < 1e-4);
    }

    #[test]
    fn the_aim_moves_only_across_the_line_and_only_so_far() {
        let line = Vec3::NEG_Z;
        let tuning = tuning();
        let shift = aim_shift(Vec3::ZERO, Vec3::new(0.1, 0.0, -0.2), line, 1.0, &tuning);
        assert!(shift.distance(Vec3::X * 0.1) < 1e-6, "{shift}");
        let held = aim_shift(Vec3::ZERO, Vec3::X * 2.0, line, 1.0, &tuning);
        assert!((held.length() - tuning.lateral_window_m).abs() < 1e-5);
        assert_eq!(
            aim_shift(Vec3::ZERO, Vec3::X, line, 0.0, &tuning),
            Vec3::ZERO
        );
    }

    #[test]
    fn the_catching_hand_switches_only_for_a_clearly_nearer_one() {
        let offer = |miss_m| Some(HandOffer { miss_m });
        let pick = |current, left, right| catching_hand(current, [left, right], 0.1, Hand::Right);
        assert_eq!(pick(None, offer(0.3), offer(0.2)), Hand::Right);
        assert_eq!(pick(Some(Hand::Left), offer(0.25), offer(0.2)), Hand::Left);
        assert_eq!(pick(Some(Hand::Left), offer(0.35), offer(0.2)), Hand::Right);
        assert_eq!(pick(Some(Hand::Left), None, offer(0.6)), Hand::Right);
        assert_eq!(pick(None, None, None), Hand::Right);
        assert_eq!(pick(Some(Hand::Left), None, None), Hand::Left);
    }

    #[test]
    fn the_arrival_slides_only_what_is_left() {
        let tuning = tuning();
        assert_eq!(arrival_slide(0.01, 0.005, 12.0, 0.12, &tuning), 0.0);
        let half = arrival_slide(tuning.full_slide_rad / 2.0, 0.0, 12.0, 0.12, &tuning);
        assert!((half - 0.06).abs() < 1e-6);
        assert_eq!(arrival_slide(3.0, 0.0, 12.0, 0.12, &tuning), 0.12);
        // A gun aligned but 20 cm out at 12 m/s flies the rest in under a fiftieth of a second.
        let flight = arrival_slide(0.0, 0.2, 12.0, 0.12, &tuning);
        assert!((flight - 0.2 / 12.0).abs() < 1e-6);
        assert_eq!(arrival_slide(f32::NAN, 0.0, 12.0, 0.12, &tuning), 0.12);
    }

    #[test]
    fn the_drive_comes_on_over_its_ramp_or_its_ticks() {
        let tuning = tuning();
        assert_eq!(drive_weight(2.5, 2.5, 0, &tuning), 0.0);
        assert_eq!(drive_weight(2.5, 2.4, 1, &tuning), 1.0);
        assert_eq!(drive_weight(2.5, 2.5 - tuning.ramp_m, 0, &tuning), 1.0);
        assert_eq!(
            drive_weight(0.8, 0.8, tuning.ramp_ticks as u32, &tuning),
            1.0
        );
    }

    #[test]
    fn time_to_go_runs_to_the_catch_and_is_held_to_its_limits() {
        let tuning = tuning();
        let t = time_to_go(
            Vec3::ZERO,
            Vec3::NEG_Z * 10.0,
            Vec3::NEG_Z * 2.25,
            0.25,
            &tuning,
        );
        assert!((t - 0.2).abs() < 1e-5);
        assert_eq!(
            time_to_go(Vec3::ZERO, Vec3::Z, Vec3::NEG_Z, 0.25, &tuning),
            tuning.max_t_go_s
        );
        assert_eq!(
            time_to_go(
                Vec3::ZERO,
                Vec3::NEG_Z * 100.0,
                Vec3::NEG_Z * 0.3,
                0.25,
                &tuning
            ),
            tuning.min_t_go_s
        );
    }

    #[test]
    fn the_miss_is_measured_ahead_of_the_body() {
        let miss = miss_distance(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(0.2, 0.0, -3.0));
        assert!((miss - 0.2).abs() < 1e-6);
        let behind = miss_distance(Vec3::ZERO, Vec3::NEG_Z, Vec3::new(0.0, 0.0, 1.0));
        assert!((behind - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_hand_pose_is_extrapolated_by_its_motion() {
        let last = (Vec3::ZERO, Quat::IDENTITY);
        let now = (Vec3::X * 0.1, Quat::from_rotation_y(0.1));
        let (position, rotation, angular) = extrapolate(Some(last), now, 0.1, 0.05, 0.1);
        assert!(position.distance(Vec3::X * 0.15) < 1e-5);
        assert!(rotation.angle_between(Quat::from_rotation_y(0.15)) < 1e-4);
        assert!(angular.distance(Vec3::Y) < 1e-4);
        // Held to its limit.
        let (held, _, _) = extrapolate(Some(last), now, 0.1, 1.0, 0.1);
        assert!(held.distance(Vec3::X * 0.2) < 1e-5);
        assert_eq!(extrapolate(None, now, 0.1, 0.05, 0.1).0, now.0);
    }
}
