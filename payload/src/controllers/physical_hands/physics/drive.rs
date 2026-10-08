//! The drive of a hand body, pure: the velocities that carry a body to where its hand should be at
//! the end of the next physics step, within the hand's strength, and the push the hand's lag gives
//! the player.
//!
//! The drive is a velocity target solved once per step: the body is asked to reach its goal in one
//! step, and the change of velocity that asks for it is capped by the hand's force and twist over
//! the step. The physics step then integrates the velocities and resolves the body's contacts, so a
//! hand that meets a wall stops at it, and the next step asks again, no harder than its strength.

use glam::{Mat3, Quat, Vec2, Vec3};

/// How a hand body is driven.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DriveTuning {
    pub mass_kg: f32,
    /// The strongest pull, newtons.
    pub max_force_n: f32,
    /// The strongest twist, newton metres.
    pub max_torque_nm: f32,
    /// The share of the position error closed in one step, `0..=1`.
    pub linear_gain: f32,
    /// The share of the rotation error closed in one step, `0..=1`.
    pub angular_gain: f32,
    pub max_speed_mps: f32,
    pub max_angular_speed_rps: f32,
}

/// A hand body's state before a step, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyMotion {
    pub position: Vec3,
    pub orientation: Quat,
    pub linear: Vec3,
    pub angular: Vec3,
    /// The inverse inertia, world space.
    pub inverse_inertia: Mat3,
}

/// Where a hand body should be at the end of the next step, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Goal {
    pub position: Vec3,
    pub orientation: Quat,
}

/// The drive of one step.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Drive {
    /// The velocities to give the body, world space.
    pub linear: Vec3,
    pub angular: Vec3,
    /// The share of the hand's strongest pull used, `0..=1`.
    pub force_share: f32,
    /// The share of the hand's strongest twist used, `0..=1`.
    pub torque_share: f32,
}

/// The velocities that carry `body` to `goal` over a step of `step_s` seconds, within `tuning`. A
/// body or goal that is not finite keeps the body's velocities.
pub fn drive(body: &BodyMotion, goal: &Goal, tuning: &DriveTuning, step_s: f32) -> Drive {
    let kept = Drive {
        linear: body.linear,
        angular: body.angular,
        force_share: 0.0,
        torque_share: 0.0,
    };
    if !(step_s.is_finite() && step_s > 0.0 && finite(body) && finite_goal(goal)) {
        return kept;
    }
    let (linear, force_share) = drive_linear(body, goal, tuning, step_s);
    let (angular, torque_share) = drive_angular(body, goal, tuning, step_s);
    Drive {
        linear,
        angular,
        force_share,
        torque_share,
    }
}

/// How a hand's lag pushes the player.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PushTuning {
    /// The hand body's mass, kilograms.
    pub hand_mass_kg: f32,
    /// The strongest pull of the hand, newtons.
    pub max_force_n: f32,
    /// The mass the reaction moves, kilograms.
    pub body_mass_kg: f32,
    /// The lag left alone, metres.
    pub deadband_m: f32,
    /// The least horizontal share of the lag that pushes, `0..=1`.
    pub min_horizontal: f32,
}

/// The push a hand gives the player, world `(x, z)` metres per second, from `lag`: how far the hand
/// body is short of the goal it was driven to on the last step, world space. A hand that tracks
/// freely has no lag; one that a wall or an object too heavy to move stops falls behind its
/// controller, and it pushes the player the other way as hard as the spring between the body and
/// the controller pulls, within the hand's strength, over a step of `step_s` seconds. A lag that is
/// mostly vertical (pressing down on a table or the floor) does not push.
pub fn lag_push(lag: Vec3, tuning: &PushTuning, step_s: f32) -> Vec2 {
    if !(lag.is_finite() && step_s.is_finite() && step_s > 0.0 && tuning.body_mass_kg > 0.0) {
        return Vec2::ZERO;
    }
    let length = lag.length();
    if length <= tuning.deadband_m || length <= f32::EPSILON {
        return Vec2::ZERO;
    }
    let horizontal = Vec2::new(lag.x, lag.z);
    if horizontal.length() < tuning.min_horizontal * length {
        return Vec2::ZERO;
    }
    let pressed = length - tuning.deadband_m;
    let force = (tuning.hand_mass_kg * pressed / (step_s * step_s)).min(tuning.max_force_n);
    let speed = force * step_s / tuning.body_mass_kg;
    -horizontal.normalize_or_zero() * speed
}

/// Fold `push` into the smoothed push `smoothed` over `dt` seconds with the time constant
/// `smoothing_s`, and cap the result at `max_speed`.
pub fn smooth_push(smoothed: Vec2, push: Vec2, dt: f32, smoothing_s: f32, max_speed: f32) -> Vec2 {
    if !push.is_finite() || !smoothed.is_finite() {
        return Vec2::ZERO;
    }
    let blend = if smoothing_s > 0.0 && dt.is_finite() && dt > 0.0 {
        1.0 - (-dt / smoothing_s).exp()
    } else {
        1.0
    };
    smoothed
        .lerp(push, blend)
        .clamp_length_max(max_speed.max(0.0))
}

/// Whether the rig moved under the hands between two palms' ticks without the hands moving: the
/// body's facing turned by more than [`RIG_JUMP_RAD`] (a snap turn, not the body turning), or the
/// recenter count changed. `last` is the facing and the count of the last tick, if there was one.
pub fn rig_jumped(last: Option<(Quat, u32)>, rotation: Quat, recenters: u32) -> bool {
    last.is_some_and(|(last_rotation, last_recenters)| {
        last_recenters != recenters || last_rotation.angle_between(rotation) > RIG_JUMP_RAD
    })
}

/// The turn of the body's facing between two palms' ticks past which it is a snap turn.
pub const RIG_JUMP_RAD: f32 = 0.2;

/// Where a body at `body` goes when the rig carries its hand's palm from `from` to `to` in one
/// tick: the body keeps its place and turn relative to the palm.
pub fn rig_carry(body: Goal, from: Goal, to: Goal) -> Goal {
    let turn = (to.orientation * from.orientation.inverse()).normalize();
    let carried = Goal {
        position: to.position + turn * (body.position - from.position),
        orientation: (turn * body.orientation).normalize(),
    };
    if finite_goal(&carried) { carried } else { to }
}

fn finite(body: &BodyMotion) -> bool {
    body.position.is_finite()
        && body.orientation.is_finite()
        && body.linear.is_finite()
        && body.angular.is_finite()
        && body.inverse_inertia.is_finite()
}

fn finite_goal(goal: &Goal) -> bool {
    goal.position.is_finite() && goal.orientation.is_finite()
}

/// The linear velocity toward the goal, and the share of the strongest pull it took.
fn drive_linear(body: &BodyMotion, goal: &Goal, tuning: &DriveTuning, step_s: f32) -> (Vec3, f32) {
    let wanted = ((goal.position - body.position) * tuning.linear_gain.clamp(0.0, 1.0) / step_s)
        .clamp_length_max(tuning.max_speed_mps.max(0.0));
    let change = wanted - body.linear;
    let max_change = tuning.max_force_n.max(0.0) * step_s / tuning.mass_kg.max(f32::EPSILON);
    let needed = change.length();
    let share = if max_change > 0.0 {
        (needed / max_change).min(1.0)
    } else if needed > 0.0 {
        1.0
    } else {
        0.0
    };
    (body.linear + change.clamp_length_max(max_change), share)
}

/// The angular velocity toward the goal's orientation, and the share of the strongest twist it
/// took.
fn drive_angular(body: &BodyMotion, goal: &Goal, tuning: &DriveTuning, step_s: f32) -> (Vec3, f32) {
    let error =
        rotation_vector(goal.orientation.normalize() * body.orientation.normalize().inverse());
    let wanted = (error * tuning.angular_gain.clamp(0.0, 1.0) / step_s)
        .clamp_length_max(tuning.max_angular_speed_rps.max(0.0));
    let change = wanted - body.angular;
    if body.inverse_inertia.determinant().abs() <= f32::EPSILON {
        // An infinite inertia about some axis: no twist turns the body, so it keeps its spin.
        return (body.angular, 0.0);
    }
    let torque = body.inverse_inertia.inverse() * change / step_s;
    let needed = torque.length();
    let max_torque = tuning.max_torque_nm.max(0.0);
    if needed <= max_torque {
        let share = if max_torque > 0.0 {
            needed / max_torque
        } else {
            0.0
        };
        return (wanted, share);
    }
    let scale = max_torque / needed;
    (body.angular + change * scale, 1.0)
}

/// The rotation `q` as an axis times its angle, taking the shorter way round.
fn rotation_vector(q: Quat) -> Vec3 {
    let q = if q.w < 0.0 { -q } else { q };
    let (axis, angle) = q.to_axis_angle();
    if !axis.is_finite() || !angle.is_finite() {
        return Vec3::ZERO;
    }
    axis * angle
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    const STEP: f32 = 1.0 / 30.0;

    fn tuning() -> DriveTuning {
        DriveTuning {
            mass_kg: 10.0,
            max_force_n: 5000.0,
            max_torque_nm: 400.0,
            linear_gain: 1.0,
            angular_gain: 1.0,
            max_speed_mps: 20.0,
            max_angular_speed_rps: 30.0,
        }
    }

    fn at_rest(position: Vec3) -> BodyMotion {
        BodyMotion {
            position,
            orientation: Quat::IDENTITY,
            linear: Vec3::ZERO,
            angular: Vec3::ZERO,
            inverse_inertia: Mat3::from_diagonal(Vec3::splat(10.0)),
        }
    }

    fn goal(position: Vec3) -> Goal {
        Goal {
            position,
            orientation: Quat::IDENTITY,
        }
    }

    #[test]
    fn a_free_body_reaches_a_near_goal_in_one_step() {
        let d = drive(&at_rest(Vec3::ZERO), &goal(Vec3::X * 0.05), &tuning(), STEP);
        assert!((d.linear - Vec3::X * 0.05 / STEP).length() < 1e-4);
        assert!(d.force_share > 0.0 && d.force_share < 1.0);
    }

    #[test]
    fn the_pull_is_capped_by_the_hand_strength() {
        let t = tuning();
        // Far enough that the wanted speed is past what the strength gives in one step, but under
        // the speed cap.
        let d = drive(&at_rest(Vec3::ZERO), &goal(Vec3::X * 0.6), &t, STEP);
        let max_change = t.max_force_n * STEP / t.mass_kg;
        assert!((d.linear.length() - max_change).abs() < 1e-3);
        assert_eq!(d.force_share, 1.0);
    }

    #[test]
    fn a_body_on_its_goal_moving_with_it_keeps_its_velocity() {
        let mut body = at_rest(Vec3::ZERO);
        body.linear = Vec3::Z * 2.0;
        let d = drive(&body, &goal(Vec3::Z * 2.0 * STEP), &tuning(), STEP);
        assert!((d.linear - body.linear).length() < 1e-4);
        assert!(d.force_share < 1e-3);
    }

    #[test]
    fn the_speed_is_capped() {
        let mut t = tuning();
        t.max_speed_mps = 1.0;
        t.max_force_n = 1.0e9;
        let d = drive(&at_rest(Vec3::ZERO), &goal(Vec3::Y * 10.0), &t, STEP);
        assert!((d.linear.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_rotation_error_turns_the_body_about_its_axis() {
        let mut t = tuning();
        t.max_angular_speed_rps = 1000.0;
        t.max_torque_nm = 1.0e9;
        let g = Goal {
            position: Vec3::ZERO,
            orientation: Quat::from_rotation_y(FRAC_PI_2),
        };
        let d = drive(&at_rest(Vec3::ZERO), &g, &t, STEP);
        assert!((d.angular - Vec3::Y * FRAC_PI_2 / STEP).length() < 1e-2);
    }

    #[test]
    fn the_twist_is_capped_by_the_hand_strength() {
        let mut t = tuning();
        t.max_angular_speed_rps = 1000.0;
        t.max_torque_nm = 1.0;
        let g = Goal {
            position: Vec3::ZERO,
            orientation: Quat::from_rotation_x(1.0),
        };
        let body = at_rest(Vec3::ZERO);
        let d = drive(&body, &g, &t, STEP);
        let torque = body.inverse_inertia.inverse() * d.angular / STEP;
        assert!((torque.length() - 1.0).abs() < 1e-3);
        assert_eq!(d.torque_share, 1.0);
    }

    #[test]
    fn a_rotation_takes_the_shorter_way_round() {
        let v = rotation_vector(-Quat::from_rotation_z(0.3));
        assert!((v - Vec3::Z * 0.3).length() < 1e-4);
    }

    #[test]
    fn a_snap_turn_or_a_recenter_is_a_rig_jump() {
        let facing = Quat::from_rotation_y(0.5);
        assert!(!rig_jumped(None, facing, 0));
        assert!(!rig_jumped(
            Some((facing, 3)),
            Quat::from_rotation_y(0.52),
            3
        ));
        assert!(rig_jumped(Some((facing, 3)), Quat::from_rotation_y(1.3), 3));
        assert!(rig_jumped(Some((facing, 3)), facing, 4));
    }

    #[test]
    fn the_rig_carries_a_body_with_its_palm() {
        let from = Goal {
            position: Vec3::new(0.0, 1.0, -0.5),
            orientation: Quat::IDENTITY,
        };
        // A quarter snap turn about the player at the origin.
        let snap = Quat::from_rotation_y(FRAC_PI_2);
        let to = Goal {
            position: snap * from.position,
            orientation: snap,
        };
        // A body held 10 cm short of its palm along the palm's -Z stays 10 cm short along it.
        let body = Goal {
            position: from.position + Vec3::Z * 0.1,
            orientation: Quat::IDENTITY,
        };
        let carried = rig_carry(body, from, to);
        assert!(
            carried
                .position
                .abs_diff_eq(to.position + snap * Vec3::Z * 0.1, 1e-5)
        );
        assert!(carried.orientation.angle_between(snap) < 1e-5);
        let bad = Goal {
            position: Vec3::NAN,
            ..body
        };
        assert_eq!(rig_carry(bad, from, to), to);
    }

    #[test]
    fn a_body_that_is_not_finite_keeps_its_velocities() {
        let mut body = at_rest(Vec3::ZERO);
        body.linear = Vec3::X;
        body.position = Vec3::NAN;
        let d = drive(&body, &goal(Vec3::ZERO), &tuning(), STEP);
        assert_eq!(d.linear, Vec3::X);
        assert_eq!(d.force_share, 0.0);
    }

    fn push_tuning() -> PushTuning {
        PushTuning {
            hand_mass_kg: 10.0,
            max_force_n: 5000.0,
            body_mass_kg: 90.0,
            deadband_m: 0.02,
            min_horizontal: 0.3,
        }
    }

    #[test]
    fn a_hand_held_back_by_a_wall_pushes_the_player_away() {
        // The hand is short of its goal along +X: the wall is at +X, so the player goes -X.
        let push = lag_push(Vec3::X * 0.1, &push_tuning(), STEP);
        assert!(push.x < 0.0);
        assert!(push.y.abs() < 1e-6);
    }

    #[test]
    fn a_small_lag_does_not_push() {
        assert_eq!(lag_push(Vec3::X * 0.01, &push_tuning(), STEP), Vec2::ZERO);
    }

    #[test]
    fn pressing_down_does_not_push() {
        assert_eq!(
            lag_push(Vec3::new(0.01, -0.2, 0.0), &push_tuning(), STEP),
            Vec2::ZERO
        );
    }

    #[test]
    fn the_push_is_capped_by_the_hand_strength() {
        let t = push_tuning();
        let far = lag_push(Vec3::Z * 10.0, &t, STEP);
        assert!((far.length() - t.max_force_n * STEP / t.body_mass_kg).abs() < 1e-4);
    }

    #[test]
    fn the_smoothed_push_approaches_the_push_and_is_capped() {
        let mut smoothed = Vec2::ZERO;
        for _ in 0..100 {
            smoothed = smooth_push(smoothed, Vec2::X * 5.0, STEP, 0.1, 2.5);
        }
        assert!((smoothed - Vec2::X * 2.5).length() < 1e-3);
        let once = smooth_push(Vec2::ZERO, Vec2::X, STEP, 0.1, 2.5);
        assert!(once.x > 0.0 && once.x < 1.0);
    }
}
