//! The standing regime's steps. Pure, in world space.
//!
//! Planted feet hold their world pose. The support is the region the two feet cover, their
//! footprints' hull widened by a margin, and the capture point is where the pelvis would come to
//! rest at its current velocity, `p + v·√(h/g)`. A foot steps when the capture point leaves the
//! support, when the foot is further from its rest place than a threshold, or when it is turned
//! from its rest orientation by more than one. The stepping foot is the one on the side the capture
//! point went, otherwise the one further from its rest. Small disturbances inside the support step
//! nothing (after Kenwright).

use glam::{Vec2, Vec3};

use crate::controllers::body::rig::legs::swing::FootPose;

/// The standing steps' tuning.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StepTuning {
    /// How far, horizontally, a foot may be from its rest place before it steps, metres.
    pub rest_m: f32,
    /// How far a foot may be turned from its rest orientation before it steps, radians.
    pub turn_rad: f32,
    /// How long a standing step takes, seconds.
    pub duration_s: f32,
    /// How high a standing step lifts the foot at its middle, metres.
    pub height_m: f32,
    /// How far past the footprints the support reaches, metres.
    pub margin_m: f32,
    /// How far a foot tilts to the ground's slope, at most, radians.
    pub tilt_limit_rad: f32,
}

/// Why a foot steps.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StepReason {
    /// The capture point left the support.
    Capture,
    /// The foot is too far from its rest place, or turned too far from it.
    Rest,
}

/// Which foot steps, and why.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct StepChoice {
    pub foot: usize,
    pub reason: StepReason,
}

/// Which planted foot steps, if any. `planted` and `rest` are the feet's poses and rest poses,
/// left then right, `pelvis` the pelvis's ground point, and `capture` the capture point.
pub fn choose(
    planted: &[FootPose; 2],
    rest: &[FootPose; 2],
    pelvis: Vec3,
    capture: Vec3,
    tuning: &StepTuning,
) -> Option<StepChoice> {
    let errors = [0, 1].map(|i| rest_error(&planted[i], &rest[i], tuning));
    let worse = if errors[1] > errors[0] { 1 } else { 0 };
    let forward = stance_forward(rest);
    let outside = support_distance(flat(capture), planted, forward, tuning.margin_m) > 0.0;
    if outside {
        let middle = (planted[0].position + planted[1].position) * 0.5;
        let excursion = flat(capture - middle);
        let side = |i: usize| flat(rest[i].position - pelvis).dot(excursion);
        let foot = if (side(0) - side(1)).abs() > SIDE_EPSILON {
            if side(1) > side(0) { 1 } else { 0 }
        } else {
            worse
        };
        return Some(StepChoice {
            foot,
            reason: StepReason::Capture,
        });
    }
    (errors[worse] > 1.0).then_some(StepChoice {
        foot: worse,
        reason: StepReason::Rest,
    })
}

/// How far a planted foot is from its rest pose, as a share of the thresholds: above one, it steps.
pub fn rest_error(planted: &FootPose, rest: &FootPose, tuning: &StepTuning) -> f32 {
    let distance = flat(rest.position - planted.position).length();
    let turn = planted.rotation.angle_between(rest.rotation);
    let error = (distance / tuning.rest_m.max(1e-3)).max(turn / tuning.turn_rad.max(1e-3));
    if error.is_finite() { error } else { 0.0 }
}

/// How far `point` (`(x, z)`) is outside the support of the `feet`, metres: the hull of their
/// footprints, laid along `forward`, widened by the footprint's half-width and `margin`. Zero or
/// less inside.
pub fn support_distance(point: Vec2, feet: &[FootPose; 2], forward: Vec2, margin: f32) -> f32 {
    let along = forward * FOOT_HALF_LENGTH_M;
    let corners: Vec<Vec2> = feet
        .iter()
        .flat_map(|f| {
            let c = flat(f.position);
            [c + along, c - along]
        })
        .collect();
    let hull = convex_hull(&corners);
    let reach = FOOT_HALF_WIDTH_M + margin.max(0.0);
    let distance = distance_to_hull(point, &hull);
    if distance.is_finite() {
        distance - reach
    } else {
        0.0
    }
}

/// The stance's forward, `(x, z)`: perpendicular to the line from the left rest place to the right,
/// toward -Z for a left foot on -X.
pub fn stance_forward(rest: &[FootPose; 2]) -> Vec2 {
    let across = flat(rest[1].position - rest[0].position);
    Vec2::new(across.y, -across.x).normalize_or(Vec2::new(0.0, -1.0))
}

/// How far each footprint reaches along the foot from its middle, metres.
pub const FOOT_HALF_LENGTH_M: f32 = 0.11;

/// How far each footprint reaches across the foot from its middle, metres.
pub const FOOT_HALF_WIDTH_M: f32 = 0.05;

/// How much closer to the excursion one foot must be than the other to be the side it went, in the
/// excursion's units.
const SIDE_EPSILON: f32 = 1e-4;

fn flat(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// The convex hull of `points`, counter-clockwise, without repeated points.
fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut sorted: Vec<Vec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    sorted.dedup_by(|a, b| a.distance(*b) < 1e-6);
    if sorted.len() < 3 {
        return sorted;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<Vec2> = Vec::with_capacity(sorted.len() * 2);
    for pass in [false, true] {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> = if pass {
            Box::new(sorted.iter().rev())
        } else {
            Box::new(sorted.iter())
        };
        for &p in iter {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// How far `point` is from the convex polygon `hull`: zero inside.
fn distance_to_hull(point: Vec2, hull: &[Vec2]) -> f32 {
    match hull.len() {
        0 => f32::INFINITY,
        1 => point.distance(hull[0]),
        2 => distance_to_segment(point, hull[0], hull[1]),
        n => {
            let inside =
                (0..n).all(|i| (hull[(i + 1) % n] - hull[i]).perp_dot(point - hull[i]) >= 0.0);
            if inside {
                return 0.0;
            }
            (0..n)
                .map(|i| distance_to_segment(point, hull[i], hull[(i + 1) % n]))
                .fold(f32::INFINITY, f32::min)
        }
    }
}

fn distance_to_segment(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 1e-12 {
        ((point - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    point.distance(a + ab * t)
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    const TUNING: StepTuning = StepTuning {
        rest_m: 0.22,
        turn_rad: 35.0 * std::f32::consts::PI / 180.0,
        duration_s: 0.25,
        height_m: 0.06,
        margin_m: 0.04,
        tilt_limit_rad: 0.4,
    };

    fn at(x: f32, z: f32) -> FootPose {
        FootPose {
            position: Vec3::new(x, 0.0, z),
            rotation: Quat::IDENTITY,
        }
    }

    fn rest() -> [FootPose; 2] {
        [at(-0.12, 0.0), at(0.12, 0.0)]
    }

    #[test]
    fn the_support_covers_the_feet_and_a_margin() {
        let feet = rest();
        let forward = stance_forward(&feet);
        assert!((forward - Vec2::new(0.0, -1.0)).length() < 1e-6);
        assert!(support_distance(Vec2::ZERO, &feet, forward, 0.04) < 0.0);
        assert!(support_distance(Vec2::new(0.0, -0.15), &feet, forward, 0.04) < 0.0);
        assert!(support_distance(Vec2::new(0.0, -0.25), &feet, forward, 0.04) > 0.0);
        assert!(support_distance(Vec2::new(0.3, 0.0), &feet, forward, 0.04) > 0.0);
    }

    #[test]
    fn a_small_disturbance_steps_nothing() {
        let feet = rest();
        let pelvis = Vec3::ZERO;
        assert_eq!(
            choose(&feet, &feet, pelvis, Vec3::new(0.05, 0.0, -0.05), &TUNING),
            None
        );
    }

    #[test]
    fn a_capture_point_out_to_one_side_steps_that_foot() {
        let feet = rest();
        let choice = choose(&feet, &feet, Vec3::ZERO, Vec3::new(0.35, 0.0, 0.0), &TUNING).unwrap();
        assert_eq!(choice.foot, 1);
        assert_eq!(choice.reason, StepReason::Capture);
        let left = choose(
            &feet,
            &feet,
            Vec3::ZERO,
            Vec3::new(-0.35, 0.0, 0.0),
            &TUNING,
        )
        .unwrap();
        assert_eq!(left.foot, 0);
    }

    #[test]
    fn a_foot_far_from_its_rest_or_turned_steps() {
        let planted = rest();
        let moved = [at(-0.12, -0.3), at(0.12, -0.05)];
        let choice = choose(&planted, &moved, Vec3::ZERO, Vec3::ZERO, &TUNING);
        assert_eq!(
            choice,
            Some(StepChoice {
                foot: 0,
                reason: StepReason::Rest
            })
        );
        let mut turned = rest();
        turned[1].rotation = Quat::from_rotation_y(0.8);
        let choice = choose(&planted, &turned, Vec3::ZERO, Vec3::ZERO, &TUNING).unwrap();
        assert_eq!(choice.foot, 1);
    }
}
