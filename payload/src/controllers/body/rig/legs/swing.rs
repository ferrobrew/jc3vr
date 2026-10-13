//! A foot through its step and on the ground: the eased arc a step follows, the ground under a
//! foothold, the tilt to its slope, the heel's roll, and the strafe's gap between the feet. Pure,
//! in world space.

use glam::{Quat, Vec3};

/// A foot's place and orientation in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FootPose {
    pub position: Vec3,
    pub rotation: Quat,
}

/// The ground under a foothold.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Ground {
    /// The ground's height, metres.
    pub height: f32,
    /// The ground's upward normal.
    pub normal: Vec3,
}

/// The foot's pose `t` of the way through a step: eased along the line, lifted on a sine arch that
/// also clears the rise of a step up.
pub fn arc(from: &FootPose, to: &FootPose, t: f32, height: f32) -> FootPose {
    let t = t.clamp(0.0, 1.0);
    let eased = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let rise = (to.position.y - from.position.y).abs() * 0.5;
    FootPose {
        position: from.position.lerp(to.position, eased)
            + Vec3::Y * ((height + rise) * (std::f32::consts::PI * t).sin()),
        rotation: from.rotation.slerp(to.rotation, eased),
    }
}

/// `pose` tilted so the foot lies on ground with `normal`, at most `limit` radians.
pub fn tilt(pose: &FootPose, normal: Vec3, limit: f32) -> FootPose {
    let normal = normal.normalize_or(Vec3::Y);
    let angle = normal.angle_between(Vec3::Y);
    if angle.is_nan() || angle <= 1e-4 {
        return *pose;
    }
    let axis = Vec3::Y.cross(normal).normalize_or_zero();
    if axis == Vec3::ZERO {
        return *pose;
    }
    let turn = Quat::from_axis_angle(axis, angle.min(limit.max(0.0)));
    FootPose {
        position: pose.position,
        rotation: (turn * pose.rotation).normalize(),
    }
}

/// `wanted` stood on the ground under it: raised or lowered by the ground's height against the
/// root's, so the ankle keeps its height above the ground, with the ground's normal. Without
/// ground, the wanted place on a level floor.
pub fn grounded(
    wanted: &FootPose,
    root_height: f32,
    ground: &mut impl FnMut(Vec3) -> Option<Ground>,
) -> (FootPose, Vec3) {
    let found = ground(Vec3::new(wanted.position.x, root_height, wanted.position.z))
        .filter(|g| g.height.is_finite() && g.normal.is_finite() && g.normal.y > 0.0);
    match found {
        Some(g) => (
            FootPose {
                position: wanted.position + Vec3::Y * (g.height - root_height),
                rotation: wanted.rotation,
            },
            g.normal.normalize(),
        ),
        None => (*wanted, Vec3::Y),
    }
}

/// `pose` moved horizontally by `offset`.
pub fn with_offset(pose: &FootPose, offset: Vec3) -> FootPose {
    FootPose {
        position: pose.position + Vec3::new(offset.x, 0.0, offset.z),
        rotation: pose.rotation,
    }
}

/// `ahead`, the foothold of foot `i`, moved out to its own side of `other`, the other foot, by at
/// least [`MIN_GAP_M`] along the line between the rest places (the stance's sideways axis), so a
/// strafe never crosses the feet.
pub fn beside(ahead: FootPose, i: usize, other: Vec3, rest: &[FootPose; 2]) -> FootPose {
    let across = rest[1].position - rest[0].position;
    let Some(axis) = Vec3::new(across.x, 0.0, across.z).try_normalize() else {
        return ahead;
    };
    let side = if i == 0 { -1.0 } else { 1.0 };
    let gap = (ahead.position - other).dot(axis) * side;
    if gap >= MIN_GAP_M || !gap.is_finite() {
        return ahead;
    }
    with_offset(&ahead, axis * side * (MIN_GAP_M - gap))
}

/// The heel's lift for a planted foot at `now` in its cycle, where the swing is `0..swing` and the
/// stance the rest: it rises over the last of the stance as the foot pushes off.
pub fn push_off(now: f32, swing: f32) -> f32 {
    let stance = (1.0 - swing).max(1e-3);
    let s = ((now - swing) / stance).clamp(0.0, 1.0);
    let u = ((s - (1.0 - PUSH_OFF_SHARE)) / PUSH_OFF_SHARE).clamp(0.0, 1.0);
    MAX_HEEL_LIFT * u * u * (3.0 - 2.0 * u)
}

/// The heel's lift for a foot `t` of the way through its walking swing: what the push-off left,
/// easing level.
pub fn toe_off(t: f32) -> f32 {
    let u = (t / TOE_OFF_SHARE).clamp(0.0, 1.0);
    MAX_HEEL_LIFT * (1.0 - u * u * (3.0 - 2.0 * u))
}

/// The heels' lift a tick of `dt` seconds after the walk stops: down quickly rather than at once.
pub fn heels_down(lifts: [f32; 2], dt: f32) -> [f32; 2] {
    lifts.map(|lift| {
        let lift = lift * (-dt / HEEL_DOWN_S).exp();
        if lift < 1e-4 { 0.0 } else { lift }
    })
}

/// The least sideways gap between the feet a walking step keeps, metres.
pub const MIN_GAP_M: f32 = 0.1;

/// How far a heel lifts at the end of a walking stance, radians: about twenty-five degrees.
const MAX_HEEL_LIFT: f32 = 0.44;

/// The time constant of the heels coming down when the walk stops, seconds.
const HEEL_DOWN_S: f32 = 0.06;

/// The share of the stance, from its end, over which the heel lifts.
const PUSH_OFF_SHARE: f32 = 0.35;

/// The share of the swing, from its start, over which the foot comes level again.
const TOE_OFF_SHARE: f32 = 0.5;

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, z: f32) -> FootPose {
        FootPose {
            position: Vec3::new(x, 0.0, z),
            rotation: Quat::IDENTITY,
        }
    }

    #[test]
    fn the_arc_lifts_in_the_middle_and_lands_on_the_end() {
        let from = at(0.0, 0.0);
        let to = at(0.0, -0.5);
        assert_eq!(arc(&from, &to, 0.0, 0.1), from);
        assert!((arc(&from, &to, 0.5, 0.1).position.y - 0.1).abs() < 1e-5);
        assert!(arc(&from, &to, 1.0, 0.1).position.distance(to.position) < 1e-5);
    }

    #[test]
    fn a_steep_slope_tilts_the_foot_only_to_the_limit() {
        let normal = Vec3::new(0.0, 1.0, 1.0).normalize();
        let tilted = tilt(&at(0.0, 0.0), normal, 25f32.to_radians());
        let angle = (tilted.rotation * Vec3::Y).angle_between(Vec3::Y);
        assert!((angle - 25f32.to_radians()).abs() < 1e-4);
    }

    #[test]
    fn a_foothold_stands_on_the_ground_found_under_it() {
        let mut step_up = |_: Vec3| {
            Some(Ground {
                height: 0.2,
                normal: Vec3::Y,
            })
        };
        let (pose, normal) = grounded(&at(0.0, 0.0), 0.0, &mut step_up);
        assert!((pose.position.y - 0.2).abs() < 1e-6);
        assert_eq!(normal, Vec3::Y);
        let mut none = |_: Vec3| None;
        assert_eq!(grounded(&at(1.0, 0.0), 0.0, &mut none).0, at(1.0, 0.0));
    }

    #[test]
    fn a_strafing_foot_keeps_to_its_own_side() {
        let rest = [at(-0.12, 0.0), at(0.12, 0.0)];
        let crossed = beside(at(0.2, 0.0), 0, Vec3::new(0.1, 0.0, 0.0), &rest);
        assert!((crossed.position.x - (0.1 - MIN_GAP_M)).abs() < 1e-5);
        let fine = beside(at(-0.3, 0.0), 0, Vec3::new(0.1, 0.0, 0.0), &rest);
        assert_eq!(fine, at(-0.3, 0.0));
    }

    #[test]
    fn the_heel_rolls_up_at_push_off_and_levels_in_the_swing() {
        assert_eq!(push_off(0.45, 0.4), 0.0);
        assert!(push_off(0.99, 0.4) > 0.3);
        assert!((toe_off(0.0) - MAX_HEEL_LIFT).abs() < 1e-6);
        assert_eq!(toe_off(0.6), 0.0);
        assert_eq!(heels_down([0.0, 0.3], 1.0), [0.0, 0.0]);
    }
}
