//! The hold, pure: the velocities that carry a held body where the hands hold it, within the
//! hands' strength.
//!
//! Each holding hand is a soft constraint between the palm, which nothing moves, and the body: the
//! body's grab point should be where the palm carries it ([`GripTarget::point`]), moving with the
//! palm, and the body should be turned as the palm holds it ([`GripTarget::orientation`]). Each
//! asks for a velocity that closes its error (a stiffness per metre and per radian, on top of the
//! palm's own motion), and the impulses that give those velocities are solved together, as a
//! physics engine solves its contacts: a few passes over the hands, each hand's impulse at its grab
//! point through the point's mass matrix ([`RigidBody::point_mass`]) and its turning impulse
//! through the body's inertia, each accumulated and capped by the hand's strength over the tick
//! ([`HoldTuning::hand_force_n`], [`HoldTuning::hand_torque_nm`]).
//!
//! The solve starts from where gravity will have the body after the next step, so the hands'
//! impulses include holding it up: a body heavier than the hands can lift sinks, one held at the
//! end of a long lever droops and swings from the hand, and a heavy one dragged by a hand follows
//! slowly. Two hands on one body each pull their own grab point, so the body hangs between them
//! and turns as the hands move apart; their turning drives are weakened by
//! [`HoldTuning::two_hand_angular_share`], so the two points, not the two wrists, set its line.
//!
//! The body stays dynamic: the velocities are set before the step, which then integrates them,
//! resolves the body's contacts, and resolves any constraint it hangs from, such as a hinge.

use glam::{Vec2, Vec3};

use crate::controllers::{
    body::procedural::solver::arms::TorsoKeepOut,
    physical_hands::{
        config::{PhysicalHandsConfig, PickupConfig},
        grasp::{GripTarget, rotation_vector},
        rigid::{RigidBody, Velocities},
    },
};

/// The hold's tuning.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HoldTuning {
    /// The grab point's speed toward its target per metre it is off, per second.
    pub stiffness_per_s: f32,
    /// The body's turn rate toward its target per radian it is off, per second.
    pub angular_stiffness_per_s: f32,
    pub max_speed_mps: f32,
    pub max_angular_speed_rps: f32,
    /// The strongest pull of one hand, newtons.
    pub hand_force_n: f32,
    /// The strongest twist of one hand, newton metres.
    pub hand_torque_nm: f32,
    /// The share of each hand's turning drive while both hands hold the body, `0..=1`.
    pub two_hand_angular_share: f32,
    /// Gravity's acceleration, world space.
    pub gravity: Vec3,
}

impl HoldTuning {
    pub fn of(config: &PickupConfig) -> Self {
        Self {
            stiffness_per_s: config.stiffness_per_s.max(0.0),
            angular_stiffness_per_s: config.angular_stiffness_per_s.max(0.0),
            max_speed_mps: config.max_speed_mps.max(0.0),
            max_angular_speed_rps: config.max_angular_speed_rps.max(0.0),
            hand_force_n: config.hand_force_n.max(0.0),
            hand_torque_nm: config.hand_torque_nm.max(0.0),
            two_hand_angular_share: config.two_hand_angular_share.clamp(0.0, 1.0),
            gravity: Vec3::new(0.0, -config.gravity_mps2.max(0.0), 0.0),
        }
    }
}

/// One hand's hold on a body this tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandHold {
    /// The grab point on the body now, world space.
    pub anchor: Vec3,
    /// Where the hand would have the body.
    pub target: GripTarget,
    /// The target grab point's velocity, carried by the palm.
    pub velocity: Vec3,
    /// The palm's angular velocity.
    pub angular_velocity: Vec3,
}

/// What the hold asks of the body.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HoldSolution {
    /// The velocities to set on the body before the step.
    pub velocities: Velocities,
    /// Each hand's share of its strongest pull used, `0..=1`; one when it pulls as hard as it can.
    pub loads: [f32; 2],
    /// Each hand's share of its strongest twist used, `0..=1`.
    pub torque_loads: [f32; 2],
}

/// Solve the holds `holds` (indexed by hand) on `body` over a tick of `dt` seconds.
pub fn solve(
    body: &RigidBody,
    holds: [Option<HandHold>; 2],
    dt: f32,
    tuning: &HoldTuning,
) -> HoldSolution {
    if !(dt.is_finite() && dt > 0.0) {
        return HoldSolution {
            velocities: body.velocities,
            ..HoldSolution::default()
        };
    }
    let hands = holds.iter().flatten().count();
    let angular_share = if hands > 1 {
        tuning.two_hand_angular_share
    } else {
        1.0
    };
    let stiffness = tuning.stiffness_per_s.min(MAX_CORRECTION / dt);
    let angular_stiffness = tuning.angular_stiffness_per_s.min(MAX_CORRECTION / dt);
    let wanted = holds.map(|hold| {
        hold.map(|h| {
            let linear = (finite(h.velocity) + (h.target.point - h.anchor) * stiffness)
                .clamp_length_max(tuning.max_speed_mps);
            let turn = rotation_vector(h.target.orientation * body.pose.orientation.inverse());
            let angular = (finite(h.angular_velocity) + turn * angular_stiffness)
                .clamp_length_max(tuning.max_angular_speed_rps);
            (linear, angular)
        })
    });

    let force_cap = tuning.hand_force_n * dt;
    let torque_cap = tuning.hand_torque_nm * dt;
    let mut solved = *body;
    let gravity_step = finite(tuning.gravity) * dt;
    solved.velocities.linear += gravity_step;
    let inertia = solved.inertia_world();
    let mut impulses = [Vec3::ZERO; 2];
    let mut twists = [Vec3::ZERO; 2];
    for _ in 0..ITERATIONS {
        for (i, hold) in holds.iter().enumerate() {
            let (Some(hold), Some((linear, angular))) = (hold, wanted[i]) else {
                continue;
            };
            if angular_share > 0.0 && torque_cap > 0.0 {
                let twist = inertia * (angular - solved.velocities.angular) * angular_share;
                let total = (twists[i] + twist).clamp_length_max(torque_cap);
                solved.apply_angular_impulse(total - twists[i]);
                twists[i] = total;
            }
            if force_cap > 0.0 {
                let lacking = linear - solved.point_velocity(hold.anchor);
                let impulse = solved.point_mass(hold.anchor) * lacking;
                let total = (impulses[i] + impulse).clamp_length_max(force_cap);
                solved.apply_impulse(total - impulses[i], hold.anchor);
                impulses[i] = total;
            }
        }
    }
    solved.velocities.linear -= gravity_step;
    let share = |impulse: Vec3, cap: f32| {
        if cap > 0.0 {
            (impulse.length() / cap).clamp(0.0, 1.0)
        } else {
            1.0
        }
    };
    let velocities = Velocities {
        linear: finite(solved.velocities.linear)
            .clamp_length_max(tuning.max_speed_mps + gravity_step.length()),
        angular: finite(solved.velocities.angular).clamp_length_max(tuning.max_angular_speed_rps),
    };
    HoldSolution {
        velocities,
        loads: [0, 1].map(|i| {
            if holds[i].is_some() {
                share(impulses[i], force_cap)
            } else {
                0.0
            }
        }),
        torque_loads: [0, 1].map(|i| {
            if holds[i].is_some() {
                share(twists[i], torque_cap)
            } else {
                0.0
            }
        }),
    }
}

/// The push of a hand that holds what it cannot move: the body is moved toward where the grab
/// point is, as it is pushed off a wall, once the hand pulls at near its strength. `anchor` is the
/// grab point, `target` where the hand would have it, and `load` the hand's
/// [`HoldSolution::loads`]. World `(x, z)` metres per second.
pub fn resist_push(anchor: Vec3, target: Vec3, load: f32, config: &PhysicalHandsConfig) -> Vec2 {
    let lag = anchor - target;
    let horizontal = Vec2::new(lag.x, lag.z);
    let Some(direction) = horizontal.try_normalize() else {
        return Vec2::ZERO;
    };
    let engage = ((load - RESIST_ONSET_LOAD) / (1.0 - RESIST_ONSET_LOAD)).clamp(0.0, 1.0);
    let press = horizontal.length() - config.push_deadband_m.max(0.0);
    if !(press > 0.0 && engage > 0.0) {
        return Vec2::ZERO;
    }
    let speed =
        (press * config.push_stiffness_per_s.max(0.0)).min(config.max_push_speed_mps.max(0.0));
    direction * speed * engage
}

/// The space the player's body takes, as a held body's centre sees it: an upright cylinder.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodySpace {
    /// The bottom of the cylinder's axis, world space.
    pub base: Vec3,
    pub height_m: f32,
    pub radius_m: f32,
    /// The way out of the cylinder for a point on its axis.
    pub forward: Vec3,
}

/// Where a carried object's centre is kept out of the player's own body: the procedural body's
/// torso capsules while it poses the torso this tick, or the plain cylinder otherwise. Sharing the
/// torso's own capsules, rather than a cylinder loose enough to cover any pose, keeps a held
/// object off the chest and the belly as closely as the hand IK does.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum PlayerKeepOut {
    Torso(TorsoKeepOut),
    Cylinder(BodySpace),
}

impl PlayerKeepOut {
    /// The offset that moves `point` out of the keep-out; zero for a point outside it.
    pub fn offset(&self, point: Vec3) -> Vec3 {
        match self {
            Self::Torso(torso) => torso.push_out(point) - point,
            Self::Cylinder(space) => keep_out(point, space),
        }
    }
}

/// The offset that moves `point` out of `space` horizontally; zero for a point outside it.
pub fn keep_out(point: Vec3, space: &BodySpace) -> Vec3 {
    if !(point.is_finite() && space.radius_m > 0.0)
        || point.y < space.base.y
        || point.y > space.base.y + space.height_m.max(0.0)
    {
        return Vec3::ZERO;
    }
    let horizontal = Vec3::new(point.x - space.base.x, 0.0, point.z - space.base.z);
    let distance = horizontal.length();
    if distance >= space.radius_m {
        return Vec3::ZERO;
    }
    let direction = horizontal.try_normalize().unwrap_or_else(|| {
        Vec3::new(space.forward.x, 0.0, space.forward.z).normalize_or(Vec3::NEG_Z)
    });
    direction * (space.radius_m - distance)
}

/// The most of its error a constraint closes in one tick: below one, so that a stiff hold settles
/// rather than overshoots at a slow tick.
const MAX_CORRECTION: f32 = 0.8;
/// The solver's passes over the hands.
const ITERATIONS: usize = 8;
/// The load from which a hold that cannot move its body starts pushing the player.
const RESIST_ONSET_LOAD: f32 = 0.85;

fn finite(v: Vec3) -> Vec3 {
    if v.is_finite() { v } else { Vec3::ZERO }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use glam::Quat;

    use super::*;
    use crate::controllers::{
        body::procedural::{
            rig::{Rig, tests::skeleton},
            solver::pose::RigPose,
        },
        physical_hands::rigid::tests::crate_body,
    };

    const DT: f32 = 1.0 / 60.0;

    /// The target that holds `body` as it is, by the grab point `anchor`.
    fn as_is(body: &RigidBody, anchor: Vec3) -> GripTarget {
        GripTarget {
            point: anchor,
            orientation: body.pose.orientation,
        }
    }

    fn tuning() -> HoldTuning {
        HoldTuning::of(&PickupConfig::new())
    }

    fn hold(anchor: Vec3, point: Vec3, orientation: Quat) -> HandHold {
        HandHold {
            anchor,
            target: GripTarget { point, orientation },
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        }
    }

    /// Integrate `body` as the physics step would, without contacts.
    fn step(body: &mut RigidBody, velocities: Velocities, tuning: &HoldTuning) {
        body.velocities = velocities;
        body.velocities.linear += tuning.gravity * DT;
        body.pose.centre += body.velocities.linear * DT;
        let spin = body.velocities.angular * DT;
        if let Some(axis) = spin.try_normalize() {
            body.pose.orientation =
                (Quat::from_axis_angle(axis, spin.length()) * body.pose.orientation).normalize();
        }
    }

    /// Hold `body` by its local point `on_body` with the hand still at `point` for `ticks`.
    fn settle(
        body: &mut RigidBody,
        on_body: Vec3,
        point: Vec3,
        orientation: Quat,
        ticks: usize,
    ) -> HoldSolution {
        let tuning = tuning();
        let mut last = HoldSolution::default();
        for _ in 0..ticks {
            let anchor = body.pose.point(on_body);
            last = solve(
                body,
                [Some(hold(anchor, point, orientation)), None],
                DT,
                &tuning,
            );
            step(body, last.velocities, &tuning);
        }
        last
    }

    #[test]
    fn a_light_crate_is_held_up_at_the_hand() {
        let mut body = crate_body(3.0, Vec3::splat(0.3), Vec3::ZERO, Quat::IDENTITY);
        let on_body = Vec3::new(0.0, 0.15, 0.0);
        let point = Vec3::new(0.2, 0.25, 0.1);
        let solution = settle(&mut body, on_body, point, Quat::IDENTITY, 240);
        assert!(body.pose.point(on_body).distance(point) < 0.01);
        assert!(body.pose.orientation.angle_between(Quat::IDENTITY) < 0.02);
        // Holding it up takes its weight and no more.
        let weight = 3.0 * PickupConfig::new().gravity_mps2;
        let expected = weight / PickupConfig::new().hand_force_n;
        assert!((solution.loads[0] - expected).abs() < 0.05);
    }

    #[test]
    fn a_body_too_heavy_to_lift_sinks() {
        let config = PickupConfig::new();
        let mass = 2.0 * config.hand_force_n / config.gravity_mps2;
        let mut body = crate_body(mass, Vec3::splat(0.6), Vec3::ZERO, Quat::IDENTITY);
        let on_body = Vec3::new(0.0, 0.3, 0.0);
        let point = body.pose.point(on_body);
        let solution = settle(&mut body, on_body, point, Quat::IDENTITY, 30);
        assert!(body.pose.centre.y < -0.1, "it falls away from the hand");
        assert_eq!(solution.loads[0], 1.0);
    }

    #[test]
    fn a_plank_held_by_its_end_droops_from_the_hand() {
        // A 1.2 m plank of 8 kg held level at one end: the wrist cannot hold the lever's torque,
        // so the far end drops while the near end stays at the hand.
        let mut plank = crate_body(8.0, Vec3::new(0.1, 0.05, 1.2), Vec3::ZERO, Quat::IDENTITY);
        let on_body = Vec3::new(0.0, 0.0, 0.58);
        let point = plank.pose.point(on_body);
        let tuning = tuning();
        let mut lowest = f32::INFINITY;
        let mut twisted = false;
        for _ in 0..120 {
            let anchor = plank.pose.point(on_body);
            assert!(
                anchor.distance(point) < 0.05,
                "the near end leaves the hand"
            );
            let solution = solve(
                &plank,
                [Some(hold(anchor, point, Quat::IDENTITY)), None],
                DT,
                &tuning,
            );
            twisted |= solution.torque_loads[0] >= 1.0;
            step(&mut plank, solution.velocities, &tuning);
            lowest = lowest.min(plank.pose.point(Vec3::new(0.0, 0.0, -0.6)).y);
        }
        assert!(lowest < -0.4, "the far end droops: {lowest}");
        assert!(twisted, "the wrist twists as hard as it can");
    }

    #[test]
    fn a_wrist_turn_turns_a_light_body() {
        let mut body = crate_body(1.0, Vec3::splat(0.2), Vec3::ZERO, Quat::IDENTITY);
        let on_body = Vec3::new(0.0, 0.1, 0.0);
        let point = body.pose.point(on_body);
        let turned = Quat::from_rotation_y(FRAC_PI_2);
        settle(&mut body, on_body, point, turned, 240);
        assert!(body.pose.orientation.angle_between(turned) < 0.05);
    }

    #[test]
    fn two_hands_carry_a_long_body_between_them_and_tilt_it() {
        let config = PickupConfig::new();
        let tuning = tuning();
        let mass = 1.5 * config.hand_force_n / config.gravity_mps2;
        let mut pole = crate_body(mass, Vec3::new(2.0, 0.1, 0.1), Vec3::ZERO, Quat::IDENTITY);
        let ends = [Vec3::new(-0.9, 0.0, 0.0), Vec3::new(0.9, 0.0, 0.0)];
        // One hand raised: the pole should tilt to join the hands.
        let points = [Vec3::new(-0.9, 0.0, 0.0), Vec3::new(0.85, 0.4, 0.0)];
        let mut last = HoldSolution::default();
        for _ in 0..360 {
            let holds = [0, 1].map(|i| {
                Some(hold(
                    pole.pose.point(ends[i]),
                    points[i],
                    pole.pose.orientation,
                ))
            });
            last = solve(&pole, holds, DT, &tuning);
            step(&mut pole, last.velocities, &tuning);
        }
        for i in 0..2 {
            assert!(
                pole.pose.point(ends[i]).distance(points[i]) < 0.06,
                "end {i} at {} not {}",
                pole.pose.point(ends[i]),
                points[i]
            );
        }
        // Too heavy for one hand, the pair shares it.
        let weight = mass * config.gravity_mps2 / config.hand_force_n;
        assert!(last.loads.iter().all(|&l| l > 0.3), "{last:?}");
        assert!(last.loads[0] + last.loads[1] >= weight * 0.95, "{last:?}");
    }

    #[test]
    fn the_solution_is_capped_and_finite() {
        let body = crate_body(2.0, Vec3::splat(0.3), Vec3::ZERO, Quat::IDENTITY);
        let tuning = tuning();
        let far = solve(
            &body,
            [
                Some(hold(Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0), Quat::IDENTITY)),
                None,
            ],
            DT,
            &tuning,
        );
        assert!(far.velocities.linear.length() <= tuning.max_speed_mps + 1.0);
        let bad = solve(
            &body,
            [Some(hold(Vec3::NAN, Vec3::ZERO, Quat::IDENTITY)), None],
            DT,
            &tuning,
        );
        assert!(bad.velocities.linear.is_finite() && bad.velocities.angular.is_finite());
        let none = solve(&body, [None, None], DT, &tuning);
        assert_eq!(none.loads, [0.0; 2]);
        let zero_dt = solve(&body, [None, None], 0.0, &tuning);
        assert_eq!(zero_dt.velocities, body.velocities);
    }

    #[test]
    fn a_stuck_hold_pushes_the_player_toward_the_grab_point() {
        let config = PhysicalHandsConfig::new();
        // The hand pushed 20 cm past a heavy crate's grab point along -Z: the body moves back +Z.
        let push = resist_push(Vec3::ZERO, Vec3::new(0.0, 0.0, -0.2), 1.0, &config);
        assert!(push.y > 0.0 && push.x.abs() < 1e-6);
        // Below the onset load the hand is still moving the body: no push.
        assert_eq!(
            resist_push(Vec3::ZERO, Vec3::new(0.0, 0.0, -0.2), 0.5, &config),
            Vec2::ZERO
        );
        // Within the deadband: no push.
        assert_eq!(
            resist_push(Vec3::ZERO, Vec3::new(0.0, 0.0, -0.01), 1.0, &config),
            Vec2::ZERO
        );
        let hard = resist_push(Vec3::ZERO, Vec3::new(0.0, 0.0, -5.0), 1.0, &config);
        assert!((hard.length() - config.max_push_speed_mps).abs() < 1e-5);
    }

    #[test]
    fn a_held_body_is_kept_out_of_the_players_space() {
        let space = BodySpace {
            base: Vec3::ZERO,
            height_m: 1.8,
            radius_m: 0.3,
            forward: Vec3::NEG_Z,
        };
        let offset = keep_out(Vec3::new(0.1, 1.0, 0.0), &space);
        assert!((offset - Vec3::new(0.2, 0.0, 0.0)).length() < 1e-5);
        assert_eq!(keep_out(Vec3::new(0.5, 1.0, 0.0), &space), Vec3::ZERO);
        assert_eq!(keep_out(Vec3::new(0.1, 2.5, 0.0), &space), Vec3::ZERO);
        let on_axis = keep_out(Vec3::new(0.0, 1.0, 0.0), &space);
        assert!((on_axis - Vec3::new(0.0, 0.0, -0.3)).length() < 1e-5);
    }

    #[test]
    fn the_player_keep_out_dispatches_to_the_torso_or_the_cylinder() {
        let cylinder = PlayerKeepOut::Cylinder(BodySpace {
            base: Vec3::ZERO,
            height_m: 1.8,
            radius_m: 0.3,
            forward: Vec3::NEG_Z,
        });
        assert_eq!(
            cylinder.offset(Vec3::new(0.1, 1.0, 0.0)),
            keep_out(
                Vec3::new(0.1, 1.0, 0.0),
                &BodySpace {
                    base: Vec3::ZERO,
                    height_m: 1.8,
                    radius_m: 0.3,
                    forward: Vec3::NEG_Z,
                }
            )
        );
        assert_eq!(cylinder.offset(Vec3::new(5.0, 1.0, 0.0)), Vec3::ZERO);

        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let torso = PlayerKeepOut::Torso(TorsoKeepOut::around(
            &RigPose {
                bones: rig.reference,
            },
            0.14,
            0.13,
            0.03,
        ));
        let inside = Vec3::new(0.02, 1.1, -0.05);
        let offset = torso.offset(inside);
        assert!(offset.length() > 0.0, "{offset:?}");
        assert!((inside + offset - inside).length() > 0.0);
        let outside = Vec3::new(0.0, 1.1, -0.4);
        assert_eq!(torso.offset(outside), Vec3::ZERO);
    }

    #[test]
    fn holding_as_is_asks_for_nothing_but_support() {
        let body = crate_body(1.0, Vec3::splat(0.2), Vec3::ZERO, Quat::IDENTITY);
        let anchor = Vec3::new(0.0, 0.1, 0.0);
        let h = HandHold {
            anchor,
            target: as_is(&body, anchor),
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        };
        let s = solve(&body, [Some(h), None], DT, &tuning());
        // The velocity set cancels the coming gravity step.
        let after = s.velocities.linear + tuning().gravity * DT;
        assert!(after.length() < 1e-4 && s.velocities.angular.length() < 1e-4);
    }
}
