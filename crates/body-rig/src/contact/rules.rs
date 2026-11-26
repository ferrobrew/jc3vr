//! The release rules: which planted foot wants to step, and why. Pure.

use glam::{Vec2, Vec3};

use crate::{
    contact::{ContactFrame, FootPose, Release, Rules, Side, off_stance},
    math::angle::{forward, wrap},
};

/// What the rules found this frame.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct Need {
    /// Why each foot wants to step, if it does, left then right.
    pub reasons: [Option<Release>; 2],
    /// Whether each foot's hip is out of its reach: the rule that, on both feet at once, makes a
    /// walk.
    pub urgent: [bool; 2],
    /// How far past its reach each planted foot's hip is, metres; zero within it.
    pub reach_excess: [f32; 2],
    /// The foot that wants to step most.
    pub first: Option<Side>,
}

/// Evaluate the rules for the planted feet `planted` (left then right, `None` where a foot is not
/// planted), advancing the twist's and the tidy's timers by `dt`. `drift` is how far something
/// other than the player has carried the root since the last landing.
pub(crate) fn needs(
    frame: &ContactFrame,
    planted: [Option<FootPose>; 2],
    drift: Vec2,
    twist_held: &mut f32,
    tidy_held: &mut [f32; 2],
    dt: f32,
    rules: Rules,
) -> Need {
    let tuning = rules.tuning;
    let mut need = Need::default();
    let both = planted[0].is_some() && planted[1].is_some();
    let off = |side: Side| {
        planted[side.index()].map_or(0.0, |pose| {
            (pose.flat() - crate::contact::stance_spot(frame, side).0).length()
        })
    };
    let furthest_off = || {
        Side::BOTH
            .into_iter()
            .filter(|side| planted[side.index()].is_some())
            .max_by(|a, b| off(*a).total_cmp(&off(*b)))
    };
    let limit = frame.leg_m.max(0.0) * tuning.reach_share;
    for side in Side::BOTH {
        let Some(pose) = planted[side.index()] else {
            continue;
        };
        // The caller lowers the drawn pelvis by up to the reach's drop to keep a reaching leg on
        // its foot, so the hip counts as that much lower.
        let hip = frame.hips[side.index()] - Vec3::Y * tuning.reach_drop_m.max(0.0);
        let excess = hip.distance(pose.ankle) - limit;
        if excess.is_finite() && excess > 0.0 {
            need.reach_excess[side.index()] = excess;
            need.urgent[side.index()] = true;
            want(&mut need.reasons, side, Release::Reach);
        }
    }

    if both {
        let points = support_points(
            planted.iter().flatten().copied(),
            tuning.foot_m,
            rules.file.heel_m,
        );
        let neck = Vec2::new(frame.neck.x, frame.neck.z);
        let pelvis = Vec2::new(frame.pelvis.x, frame.pelvis.z);
        let com = pelvis.lerp(neck, tuning.com_neck_share.clamp(0.0, 1.0));
        if outside_support(com, &points) > tuning.com_margin_m
            && let Some(side) = furthest_off()
        {
            want(&mut need.reasons, side, Release::CentreOfMass);
        }
    }

    if drift.length() > tuning.root_tolerance_m
        && let Some(side) = furthest_off()
    {
        want(&mut need.reasons, side, Release::RootMoved);
    }

    let headings: Vec<f32> = Side::BOTH
        .into_iter()
        .filter_map(|side| {
            planted[side.index()].map(|pose| {
                // The stance's heading the foot stands for, its toe-out taken back out.
                wrap(pose.yaw + frame.stance.toe_out_deg.to_radians() * side.across())
            })
        })
        .collect();
    let twist = match headings.as_slice() {
        [one] => Some(wrap(frame.pelvis_yaw - one)),
        [a, b] => Some(wrap(frame.pelvis_yaw - wrap(a + wrap(b - a) * 0.5))),
        _ => None,
    };
    match twist {
        Some(twist) if twist.abs() > tuning.twist_deg.to_radians() => *twist_held += dt,
        _ => *twist_held = 0.0,
    }
    if let Some(twist) = twist
        && *twist_held >= tuning.twist_hold_s
    {
        // A turn toward positive headings is a turn left: the left foot opens it.
        let side = if twist > 0.0 { Side::Left } else { Side::Right };
        let side = if planted[side.index()].is_some() {
            side
        } else {
            side.other()
        };
        want(&mut need.reasons, side, Release::Twist);
    }

    for side in Side::BOTH {
        let slot = &mut tidy_held[side.index()];
        if both && off_stance(frame, side, planted[side.index()], tuning, 1.0) {
            *slot += dt;
        } else {
            *slot = 0.0;
        }
        if *slot >= tuning.tidy_s {
            want(&mut need.reasons, side, Release::Tidy);
        }
    }

    need.first = Side::BOTH
        .into_iter()
        .filter(|side| need.reasons[side.index()].is_some())
        .max_by(|a, b| {
            let rank = |side: Side| need.reasons[side.index()].map_or(0, priority);
            rank(*a).cmp(&rank(*b)).then_with(|| {
                need.reach_excess[a.index()]
                    .total_cmp(&need.reach_excess[b.index()])
                    .then_with(|| off(*a).total_cmp(&off(*b)))
            })
        });
    need
}

/// The support polygon's points of the feet `feet`, `(x, z)`: each foot's heel, `heel_m` behind
/// the ankle, and its toe, `foot_m` ahead of it along its heading.
pub fn support_points(
    feet: impl IntoIterator<Item = FootPose>,
    foot_m: f32,
    heel_m: f32,
) -> Vec<Vec2> {
    feet.into_iter()
        .flat_map(|foot| {
            let ahead = forward(foot.yaw);
            [
                foot.flat() - ahead * heel_m.max(0.0),
                foot.flat() + ahead * foot_m.max(0.0),
            ]
        })
        .filter(|p| p.is_finite())
        .collect()
}

/// How far `point` lies outside the convex hull of `points`, metres: zero inside it, and the
/// distance to the nearest point or segment for a hull of fewer than three corners. Infinite for
/// no points.
pub fn outside_support(point: Vec2, points: &[Vec2]) -> f32 {
    let hull = convex_hull(points);
    match hull.as_slice() {
        [] => f32::INFINITY,
        [one] => point.distance(*one),
        [a, b] => segment_distance(point, *a, *b),
        corners => {
            let inside = corners
                .iter()
                .zip(corners.iter().cycle().skip(1))
                .all(|(a, b)| cross(*b - *a, point - *a) >= 0.0);
            if inside {
                0.0
            } else {
                corners
                    .iter()
                    .zip(corners.iter().cycle().skip(1))
                    .map(|(a, b)| segment_distance(point, *a, *b))
                    .fold(f32::INFINITY, f32::min)
            }
        }
    }
}

/// Give `side` the reason `reason` unless it already has a stronger one.
fn want(reasons: &mut [Option<Release>; 2], side: Side, reason: Release) {
    let slot = &mut reasons[side.index()];
    if slot.is_none_or(|current| priority(reason) > priority(current)) {
        *slot = Some(reason);
    }
}

/// How strongly a reason calls for a step: the highest wins a foot.
fn priority(reason: Release) -> u8 {
    match reason {
        Release::Reach => 6,
        Release::CentreOfMass => 5,
        Release::RootMoved => 4,
        Release::Twist => 3,
        Release::FollowUp => 2,
        Release::Tidy => 1,
    }
}

/// The convex hull of `points`, anticlockwise in `(x, z)` as the cross product counts it, without
/// collinear corners: Andrew's monotone chain.
fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut sorted: Vec<Vec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    sorted.dedup_by(|a, b| a.distance_squared(*b) < 1e-12);
    if sorted.len() < 3 {
        return sorted;
    }
    let mut hull: Vec<Vec2> = Vec::with_capacity(sorted.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let chain = |p: Vec2, hull: &mut Vec<Vec2>| {
            while hull.len() >= start + 2 {
                let n = hull.len();
                if cross(hull[n - 1] - hull[n - 2], p - hull[n - 2]) <= 0.0 {
                    hull.pop();
                } else {
                    break;
                }
            }
            hull.push(p);
        };
        if pass == 0 {
            for p in &sorted {
                chain(*p, &mut hull);
            }
        } else {
            for p in sorted.iter().rev() {
                chain(*p, &mut hull);
            }
        }
        hull.pop();
    }
    hull
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

fn segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let along = b - a;
    let length_sq = along.length_squared();
    if length_sq < 1e-12 {
        return point.distance(a);
    }
    let t = ((point - a).dot(along) / length_sq).clamp(0.0, 1.0);
    point.distance(a + along * t)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_point_inside_a_square_is_supported_and_one_outside_is_measured() {
        let square = [
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(0.5, 0.5),
        ];
        assert_eq!(outside_support(Vec2::new(0.4, 0.7), &square), 0.0);
        assert!((outside_support(Vec2::new(1.5, 0.5), &square) - 0.5).abs() < 1e-6);
        assert!((outside_support(Vec2::new(-0.3, -0.4), &square) - 0.5).abs() < 1e-6);
        assert_eq!(outside_support(Vec2::ZERO, &[]), f32::INFINITY);
        assert!((outside_support(Vec2::new(0.5, 1.0), &square[..2]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn two_square_feet_support_the_point_between_them() {
        let feet = [
            FootPose {
                ankle: glam::Vec3::new(-0.15, 0.08, 0.0),
                yaw: 0.0,
            },
            FootPose {
                ankle: glam::Vec3::new(0.15, 0.08, 0.0),
                yaw: 0.0,
            },
        ];
        let points = support_points(feet, 0.18, 0.05);
        assert_eq!(points.len(), 4);
        // Between the ankles, a little ahead: inside.
        assert_eq!(outside_support(Vec2::new(0.0, -0.05), &points), 0.0);
        // Well ahead of the toes (-Z).
        assert!((outside_support(Vec2::new(0.0, -0.4), &points) - 0.22).abs() < 1e-5);
    }

    proptest! {
        #[test]
        fn every_hull_corner_is_supported(
            points in proptest::collection::vec((-1.0f32..1.0, -1.0f32..1.0), 1..12),
        ) {
            let points: Vec<Vec2> = points.into_iter().map(|(x, y)| Vec2::new(x, y)).collect();
            for p in &points {
                prop_assert!(outside_support(*p, &points) < 1e-4);
            }
        }
    }
}
