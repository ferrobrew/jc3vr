//! The drive of a hand body, pure: the velocities that carry a body to where its hand should be at
//! the end of the next physics step, within the hand's strength, and the push the hand's lag gives
//! the player.
//!
//! The drive is a velocity target solved once per step: the body is asked to close a share of its
//! error in one step (all of it at a gain of one, which reaches the goal without overshooting or
//! ringing: the discrete critically damped drive), and the change of velocity that asks for it is
//! capped by the hand's force and twist over the step. The speed cap applies to the body's motion
//! relative to the frame that carries the hand (the player, on a moving platform or falling), whose
//! velocity is fed forward, so a hand keeps up with a fast carrier while a tracking glitch still
//! cannot fling it. The physics step then integrates the
//! velocities and resolves the body's contacts, so a hand that meets a wall stops at it, and the
//! next step asks again, no harder than its strength. A light hand body meets a light prop without
//! throwing it, since the solver shares a contact's impulse by mass, while the force cap still lets
//! it shove a heavy one over a few steps; a hand held back by what it meets is driven more softly
//! ([`contact_gain`]), so it rests on a wall rather than pounding it every step.

use body_rig::{rotation::rotation_error, smooth::follow_factor};
use glam::{Mat3, Quat, Vec2, Vec3};

use crate::controllers::clock::TimeScale;

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
    /// The fastest a body is driven relative to its carrier, metres per second.
    pub max_speed_mps: f32,
    pub max_angular_speed_rps: f32,
}

impl DriveTuning {
    /// This drive in a world running at `time_scale`. The palm a hand body follows moves in real
    /// time, so in the game's seconds it moves `1 / time_scale` as fast, and to keep up the body's
    /// top speeds rise by that and its strength, an acceleration's worth, by the square. The gains
    /// close a share of the error per step and stay. A hand body in slow motion follows the hand
    /// as at full speed, instead of lagging as far as the world is slowed.
    pub fn in_real_time(self, time_scale: TimeScale) -> Self {
        let rate = 1.0 / time_scale.get();
        Self {
            max_force_n: self.max_force_n * rate * rate,
            max_torque_nm: self.max_torque_nm * rate * rate,
            max_speed_mps: self.max_speed_mps * rate,
            max_angular_speed_rps: self.max_angular_speed_rps * rate,
            ..self
        }
    }
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

/// The velocities that carry `body` to `goal` over a step of `step_s` seconds, within `tuning`,
/// with `carrier` the world velocity of the frame that carries the goal (zero for a goal fixed in
/// the world, such as a grip's pin). A body or goal that is not finite keeps the body's velocities;
/// a carrier that is not finite counts as none.
pub fn drive(
    body: &BodyMotion,
    goal: &Goal,
    carrier: Vec3,
    tuning: &DriveTuning,
    step_s: f32,
) -> Drive {
    let kept = Drive {
        linear: body.linear,
        angular: body.angular,
        force_share: 0.0,
        torque_share: 0.0,
    };
    if !(step_s.is_finite() && step_s > 0.0 && finite(body) && finite_goal(goal)) {
        return kept;
    }
    let carrier = if carrier.is_finite() {
        carrier.clamp_length_max(MAX_CARRIER_SPEED_MPS)
    } else {
        Vec3::ZERO
    };
    let (linear, force_share) = drive_linear(body, goal, carrier, tuning, step_s);
    let (angular, torque_share) = drive_angular(body, goal, tuning, step_s);
    Drive {
        linear,
        angular,
        force_share,
        torque_share,
    }
}

/// The drive's gain for a hand whose body the last step held back by `press`, the share of its
/// strength it pressed with (`0..=1`): `gain` for a free hand, falling to `gain` times
/// `contact_share` for one pressing as hard as it can.
pub fn contact_gain(gain: f32, contact_share: f32, press: f32) -> f32 {
    let press = if press.is_finite() {
        press.clamp(0.0, 1.0)
    } else {
        0.0
    };
    gain * (1.0 - press * (1.0 - contact_share.clamp(0.0, 1.0)))
}

/// How a hand's lag pushes the player.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PushTuning {
    /// The stiffness of the spring between a held-back hand and its controller, newtons per metre.
    pub stiffness_n_per_m: f32,
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
/// the controller pulls (independent of the hand body's own mass), within the hand's strength,
/// over a step of `step_s` seconds. A lag that is
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
    let force = (tuning.stiffness_n_per_m.max(0.0) * pressed).min(tuning.max_force_n);
    let speed = force * step_s / tuning.body_mass_kg;
    -horizontal.normalize_or_zero() * speed
}

/// Fold `push` into the smoothed push `smoothed` over `dt` seconds with the time constant
/// `smoothing_s`, and cap the result at `max_speed`.
pub fn smooth_push(smoothed: Vec2, push: Vec2, dt: f32, smoothing_s: f32, max_speed: f32) -> Vec2 {
    if !push.is_finite() || !smoothed.is_finite() {
        return Vec2::ZERO;
    }
    let blend = follow_factor(dt, smoothing_s);
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

/// The fastest carrier the drive feeds forward, metres per second: past a fall's terminal speed,
/// so an estimate thrown by a jump of the rig cannot fling a hand.
pub const MAX_CARRIER_SPEED_MPS: f32 = 80.0;

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

/// The linear velocity toward the goal, with the speed relative to `carrier` capped, and the share
/// of the strongest pull it took.
fn drive_linear(
    body: &BodyMotion,
    goal: &Goal,
    carrier: Vec3,
    tuning: &DriveTuning,
    step_s: f32,
) -> (Vec3, f32) {
    let toward = (goal.position - body.position) * tuning.linear_gain.clamp(0.0, 1.0) / step_s;
    let wanted = carrier + (toward - carrier).clamp_length_max(tuning.max_speed_mps.max(0.0));
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
    let error = rotation_error(body.orientation, goal.orientation);
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
        let d = drive(
            &at_rest(Vec3::ZERO),
            &goal(Vec3::X * 0.05),
            Vec3::ZERO,
            &tuning(),
            STEP,
        );
        assert!((d.linear - Vec3::X * 0.05 / STEP).length() < 1e-4);
        assert!(d.force_share > 0.0 && d.force_share < 1.0);
    }

    #[test]
    fn the_pull_is_capped_by_the_hand_strength() {
        let t = tuning();
        // Far enough that the wanted speed is past what the strength gives in one step, but under
        // the speed cap.
        let d = drive(
            &at_rest(Vec3::ZERO),
            &goal(Vec3::X * 0.6),
            Vec3::ZERO,
            &t,
            STEP,
        );
        let max_change = t.max_force_n * STEP / t.mass_kg;
        assert!((d.linear.length() - max_change).abs() < 1e-3);
        assert_eq!(d.force_share, 1.0);
    }

    #[test]
    fn a_hand_carried_faster_than_its_cap_keeps_up_with_its_carrier() {
        let t = tuning();
        let carrier = Vec3::NEG_Y * 50.0;
        // Falling with the player at past the cap, a body on its moving goal keeps the fall.
        let body = BodyMotion {
            linear: carrier,
            ..at_rest(Vec3::ZERO)
        };
        let d = drive(&body, &goal(carrier * STEP), carrier, &t, STEP);
        assert!(d.linear.abs_diff_eq(carrier, 1e-3), "{:?}", d.linear);
        // Without the carrier fed forward, the cap would brake it toward the cap.
        let capped = drive(&body, &goal(carrier * STEP), Vec3::ZERO, &t, STEP);
        assert!(capped.linear.length() < carrier.length() - 1.0);
        // A glitch's jump is still capped, relative to the carrier.
        let glitch = drive(
            &body,
            &goal(carrier * STEP + Vec3::X * 10.0),
            carrier,
            &t,
            STEP,
        );
        assert!((glitch.linear - carrier).length() <= t.max_speed_mps + 1e-3);
        // A carrier estimate that is not finite, or absurdly fast, is held.
        let wild = drive(&body, &goal(Vec3::ZERO), Vec3::splat(f32::NAN), &t, STEP);
        assert!(wild.linear.is_finite());
        let fast = drive(&body, &goal(Vec3::ZERO), Vec3::X * 1e6, &t, STEP);
        assert!(fast.linear.length() <= MAX_CARRIER_SPEED_MPS + t.max_speed_mps + 1e-3);
    }

    #[test]
    fn a_body_on_its_goal_moving_with_it_keeps_its_velocity() {
        let mut body = at_rest(Vec3::ZERO);
        body.linear = Vec3::Z * 2.0;
        let d = drive(
            &body,
            &goal(Vec3::Z * 2.0 * STEP),
            Vec3::ZERO,
            &tuning(),
            STEP,
        );
        assert!((d.linear - body.linear).length() < 1e-4);
        assert!(d.force_share < 1e-3);
    }

    #[test]
    fn the_speed_is_capped() {
        let mut t = tuning();
        t.max_speed_mps = 1.0;
        t.max_force_n = 1.0e9;
        let d = drive(
            &at_rest(Vec3::ZERO),
            &goal(Vec3::Y * 10.0),
            Vec3::ZERO,
            &t,
            STEP,
        );
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
        let d = drive(&at_rest(Vec3::ZERO), &g, Vec3::ZERO, &t, STEP);
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
        let d = drive(&body, &g, Vec3::ZERO, &t, STEP);
        let torque = body.inverse_inertia.inverse() * d.angular / STEP;
        assert!((torque.length() - 1.0).abs() < 1e-3);
        assert_eq!(d.torque_share, 1.0);
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
        let d = drive(&body, &goal(Vec3::ZERO), Vec3::ZERO, &tuning(), STEP);
        assert_eq!(d.linear, Vec3::X);
        assert_eq!(d.force_share, 0.0);
    }

    fn push_tuning() -> PushTuning {
        PushTuning {
            stiffness_n_per_m: 9000.0,
            max_force_n: 5000.0,
            body_mass_kg: 90.0,
            deadband_m: 0.02,
            min_horizontal: 0.3,
        }
    }

    #[test]
    fn a_held_back_hand_is_driven_softly() {
        assert_eq!(contact_gain(1.0, 0.4, 0.0), 1.0);
        assert!((contact_gain(1.0, 0.4, 1.0) - 0.4).abs() < 1e-6);
        assert!((contact_gain(1.0, 0.4, 0.5) - 0.7).abs() < 1e-6);
        assert_eq!(contact_gain(0.8, 0.4, f32::NAN), 0.8);
    }

    #[test]
    fn a_light_hand_reaches_its_goal_and_holds_still_there() {
        // A three-kilogram hand driven at a goal 20 cm away settles on it without overshooting,
        // however its error is closed.
        let t = DriveTuning {
            mass_kg: 3.0,
            ..tuning()
        };
        let goal = goal(Vec3::X * 0.2);
        for gain in [0.5, 1.0] {
            let t = DriveTuning {
                linear_gain: gain,
                ..t
            };
            let mut body = at_rest(Vec3::ZERO);
            for _ in 0..60 {
                let d = drive(&body, &goal, Vec3::ZERO, &t, STEP);
                body.linear = d.linear;
                body.position += d.linear * STEP;
                assert!(body.position.x <= 0.2 + 1e-5, "overshot at gain {gain}");
            }
            assert!((body.position.x - 0.2).abs() < 1e-4);
            assert!(body.linear.length() < 1e-2);
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
        // A step of no time does not move it.
        assert_eq!(smooth_push(once, Vec2::X, 0.0, 0.1, 2.5), once);
    }

    #[test]
    fn a_hand_body_keeps_up_with_a_real_swing_in_slow_motion() {
        // A real 6 m/s swing is 24 m/s through a world at a quarter, past the drive's top speed;
        // steps of a quarter of the length come between the same real frames.
        let slow = TimeScale::new(0.25);
        let step = STEP * slow.get();
        let swing_game_mps = 24.0;
        let goal_at = goal(Vec3::X * swing_game_mps * step);
        let scaled = drive(
            &at_rest(Vec3::ZERO),
            &goal_at,
            Vec3::ZERO,
            &tuning().in_real_time(slow),
            step,
        );
        assert!(
            (scaled.linear.x - swing_game_mps).abs() < 1e-3,
            "{scaled:?}"
        );
        let unscaled = drive(&at_rest(Vec3::ZERO), &goal_at, Vec3::ZERO, &tuning(), step);
        assert!(unscaled.linear.x < swing_game_mps * 0.5, "{unscaled:?}");
        assert_eq!(tuning().in_real_time(TimeScale::REAL), tuning());
    }
}
