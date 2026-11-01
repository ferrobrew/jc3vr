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
        grasp::{GripTarget, rotation_vector},
        pickup::PickupTuning,
        rigid::{RigidBody, Velocities},
        tuning::PalmTuning,
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
    pub fn of(config: &PickupTuning) -> Self {
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

    /// This tuning for a world running at `time_scale` times real time (the game's slow motion
    /// below one). The palm moves in real time, so in the game's seconds it moves `1 / time_scale`
    /// times as fast and turns as fast, and to follow it a held body must be accelerated
    /// `1 / time_scale²` times as hard. The hold therefore closes its error and caps its speeds per
    /// real second, and its strength (an impulse over the tick, so an acceleration's worth) is
    /// raised by the square: a held body follows the hand in slow motion as it does at full speed,
    /// rather than falling behind as far as the world is slowed and being torn away at
    /// [`PickupTuning::break_distance_m`]. Gravity is the world's, on the world's clock, so in slow
    /// motion a heavy body is easier to hold up, as it falls slower. A `time_scale` that is not
    /// finite and positive is taken as real time.
    pub fn in_real_time(self, time_scale: f32) -> Self {
        let rate = 1.0 / real_time_scale(time_scale);
        Self {
            stiffness_per_s: self.stiffness_per_s * rate,
            angular_stiffness_per_s: self.angular_stiffness_per_s * rate,
            max_speed_mps: self.max_speed_mps * rate,
            max_angular_speed_rps: self.max_angular_speed_rps * rate,
            hand_force_n: self.hand_force_n * rate * rate,
            hand_torque_nm: self.hand_torque_nm * rate * rate,
            ..self
        }
    }

    /// Add bounded lift reserve for a loose prop deliberately held at two separated points.
    /// One hand retains its configured strength; attached bodies never use this adapter.
    pub(crate) fn carrying(
        config: &PickupTuning,
        mass_kg: f32,
        holds: &[Option<HandHold>; 2],
    ) -> Self {
        let mut tuning = Self::of(config);
        let [Some(left), Some(right)] = *holds else {
            return tuning;
        };
        let span = left.anchor.distance(right.anchor);
        let target_span = left.target.point.distance(right.target.point);
        if !mass_kg.is_finite()
            || mass_kg <= 0.0
            || mass_kg > ASSIST_MAX_MASS_KG.min(config.max_mass_kg)
            || !span.is_finite()
            || !target_span.is_finite()
            || span < ASSIST_MIN_SPAN_M
            || target_span < ASSIST_MIN_SPAN_M
        {
            return tuning;
        }
        let weight_per_hand = mass_kg * tuning.gravity.length() * 0.5;
        tuning.hand_force_n = tuning.hand_force_n.max(
            (weight_per_hand * ASSIST_LIFT_RESERVE)
                .min(tuning.hand_force_n * ASSIST_FORCE_MULTIPLIER),
        );
        tuning
    }
}

/// The game's time scale `update_speed` (game seconds per real second) as the hold uses it: one
/// for a value that is not finite and positive, and otherwise bounded to the range the game's slow
/// motion and fast-forward reach.
pub fn real_time_scale(update_speed: f32) -> f32 {
    if update_speed.is_finite() && update_speed > 0.0 {
        update_speed.clamp(MIN_TIME_SCALE, MAX_TIME_SCALE)
    } else {
        1.0
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
/// point is, as it is pushed off a wall, once the hand pulls at near its strength and the grab
/// point lags the hand by more than `deadband_m` ([`resist_deadband_m`]). `anchor` is the grab
/// point, `target` where the hand would have it, and `load` the hand's [`HoldSolution::loads`].
/// World `(x, z)` metres per second.
pub fn resist_push(
    anchor: Vec3,
    target: Vec3,
    load: f32,
    deadband_m: f32,
    config: &PalmTuning,
) -> Vec2 {
    let lag = anchor - target;
    let horizontal = Vec2::new(lag.x, lag.z);
    let Some(direction) = horizontal.try_normalize() else {
        return Vec2::ZERO;
    };
    let engage = ((load - RESIST_ONSET_LOAD) / (1.0 - RESIST_ONSET_LOAD)).clamp(0.0, 1.0);
    let press = horizontal.length() - deadband_m.max(0.0);
    if !(press > 0.0 && engage > 0.0) {
        return Vec2::ZERO;
    }
    let speed =
        (press * config.push_stiffness_per_s.max(0.0)).min(config.max_push_speed_mps.max(0.0));
    direction * speed * engage
}

/// The lag a hold leaves alone before it pushes the player ([`resist_push`]), metres. A hand on
/// something hinged, a door, or a character pushes past the palms' own deadband, as a wall does.
/// A carried loose object (`prop`) trails the hand a little whenever it is carried near the hands'
/// strength, which must not drift the player, so it pushes only once it lags by
/// [`CARRY_RESIST_DEADBAND_M`]: a heavy object the hands cannot keep up with holds the player back
/// before it trails far enough to be torn away at [`PickupTuning::break_distance_m`].
pub fn resist_deadband_m(prop: bool, config: &PalmTuning) -> f32 {
    if prop {
        CARRY_RESIST_DEADBAND_M
    } else {
        config.push_deadband_m
    }
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
/// The slowest time scale the hold scales its rates for: slower clocks hold at this one's rates,
/// so a near-stopped world does not ask a body for near-infinite speeds.
const MIN_TIME_SCALE: f32 = 0.05;
/// The fastest time scale the hold scales its rates for.
const MAX_TIME_SCALE: f32 = 4.0;
const ASSIST_MAX_MASS_KG: f32 = 150.0;
const ASSIST_MIN_SPAN_M: f32 = 0.12;
const ASSIST_LIFT_RESERVE: f32 = 1.25;
const ASSIST_FORCE_MULTIPLIER: f32 = 3.0;
/// The solver's passes over the hands.
const ITERATIONS: usize = 8;
/// The lag a carried loose object leaves alone before it pushes the player, metres: past what a
/// carry near the hands' strength trails by, and well short of the break distance.
const CARRY_RESIST_DEADBAND_M: f32 = 0.15;
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
        physical_hands::{pickup::PICKUP, rigid::tests::crate_body, tuning::PALMS},
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
        HoldTuning::of(&PICKUP)
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
    fn separated_two_hand_grips_lift_a_barrel_with_bounded_assistance() {
        let config = PICKUP;
        let mut body = crate_body(100.0, Vec3::new(0.6, 0.9, 0.6), Vec3::ZERO, Quat::IDENTITY);
        let ends = [Vec3::new(-0.3, 0.0, 0.0), Vec3::new(0.3, 0.0, 0.0)];
        let goals = ends.map(|p| p + Vec3::Y * 0.3);
        for _ in 0..180 {
            let holds =
                [0, 1].map(|i| Some(hold(body.pose.point(ends[i]), goals[i], Quat::IDENTITY)));
            let tuning = HoldTuning::carrying(&config, 100.0, &holds);
            assert!(tuning.hand_force_n <= config.hand_force_n * ASSIST_FORCE_MULTIPLIER);
            let solution = solve(&body, holds, DT, &tuning);
            step(&mut body, solution.velocities, &tuning);
        }
        assert!(body.pose.centre.y > 0.25, "{body:?}");
        assert!(body.pose.orientation.angle_between(Quat::IDENTITY) < 0.1);
    }

    /// How far a light crate held by its centre is still off a hand that came to rest 20 cm to its
    /// side, a tenth of a real second after the hand stopped, with the world at `time_scale` and
    /// its tuning scaled for that time scale when `scaled`.
    fn left_after_a_step(time_scale: f32, scaled: bool) -> f32 {
        let base = tuning();
        let tuning = if scaled {
            base.in_real_time(time_scale)
        } else {
            base
        };
        let mut body = crate_body(3.0, Vec3::splat(0.3), Vec3::ZERO, Quat::IDENTITY);
        let game_dt = DT * time_scale;
        let point = Vec3::X * 0.2;
        for _ in 0..6 {
            let hold = hold(body.pose.centre, point, Quat::IDENTITY);
            let solution = solve(&body, [Some(hold), None], game_dt, &tuning);
            body.velocities = solution.velocities;
            body.velocities.linear += tuning.gravity * game_dt;
            body.pose.centre += body.velocities.linear * game_dt;
        }
        body.pose.centre.distance(point)
    }

    #[test]
    fn a_held_body_closes_on_the_hand_as_fast_in_slow_motion_as_at_full_speed() {
        let full = left_after_a_step(1.0, true);
        let slow = left_after_a_step(0.25, true);
        assert!(full < 0.15, "full {full}");
        assert!((full - slow).abs() < 0.01, "full {full} slow {slow}");
        // Without the scaling, the slowed world leaves the body behind for longer.
        let unscaled = left_after_a_step(0.25, false);
        assert!(unscaled > full + 0.03, "unscaled {unscaled} full {full}");
    }

    /// The farthest a barrel just under the assisted carry's limit, held at both sides, trails
    /// the hands as they lift it 40 cm over a real second, with the world at `time_scale`.
    fn barrel_trail(time_scale: f32) -> f32 {
        let mass = ASSIST_MAX_MASS_KG - 1.0;
        let mut body = crate_body(mass, Vec3::new(0.6, 0.9, 0.6), Vec3::ZERO, Quat::IDENTITY);
        let ends = [Vec3::new(-0.3, 0.2, 0.0), Vec3::new(0.3, 0.2, 0.0)];
        let game_dt = DT * time_scale;
        let lift_mps = 0.4;
        let mut worst: f32 = 0.0;
        for tick in 1..=60 {
            let raised = Vec3::Y * lift_mps * DT * tick as f32;
            let holds = [0, 1].map(|i| {
                Some(HandHold {
                    velocity: Vec3::Y * lift_mps / time_scale,
                    ..hold(body.pose.point(ends[i]), ends[i] + raised, Quat::IDENTITY)
                })
            });
            let tuning = HoldTuning::carrying(&PICKUP, mass, &holds).in_real_time(time_scale);
            let solution = solve(&body, holds, game_dt, &tuning);
            body.velocities = solution.velocities;
            body.velocities.linear += tuning.gravity * game_dt;
            body.pose.centre += body.velocities.linear * game_dt;
            let lag = (body.pose.centre - raised).length();
            worst = worst.max(lag);
        }
        worst
    }

    #[test]
    fn a_heavy_barrel_follows_a_lift_in_slow_motion_as_at_full_speed() {
        let full = barrel_trail(1.0);
        let slow = barrel_trail(0.25);
        assert!(full < PICKUP.break_distance_m * 0.5, "full {full}");
        assert!(slow <= full + 0.01, "full {full} slow {slow}");
    }

    #[test]
    fn the_time_scale_raises_the_rates_and_the_strength_and_takes_a_broken_clock_as_real_time() {
        let base = tuning();
        let slow = base.in_real_time(0.25);
        assert_eq!(slow.stiffness_per_s, base.stiffness_per_s * 4.0);
        assert_eq!(slow.max_speed_mps, base.max_speed_mps * 4.0);
        assert_eq!(
            slow.angular_stiffness_per_s,
            base.angular_stiffness_per_s * 4.0
        );
        assert_eq!(slow.max_angular_speed_rps, base.max_angular_speed_rps * 4.0);
        assert_eq!(slow.hand_force_n, base.hand_force_n * 16.0);
        assert_eq!(slow.hand_torque_nm, base.hand_torque_nm * 16.0);
        assert_eq!(slow.gravity, base.gravity);
        for broken in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(base.in_real_time(broken), base);
        }
        assert_eq!(real_time_scale(0.001), MIN_TIME_SCALE);
        assert_eq!(real_time_scale(100.0), MAX_TIME_SCALE);
        assert_eq!(real_time_scale(1.0), 1.0);
    }

    #[test]
    fn assistance_does_not_strengthen_one_hand_near_coincident_grips_or_large_bodies() {
        let config = PICKUP;
        let left = hold(Vec3::NEG_X * 0.3, Vec3::NEG_X * 0.3, Quat::IDENTITY);
        let right = hold(Vec3::X * 0.3, Vec3::X * 0.3, Quat::IDENTITY);
        let baseline = HoldTuning::of(&config);
        assert_eq!(
            HoldTuning::carrying(&config, 100.0, &[Some(left), None]),
            baseline
        );
        assert_eq!(
            HoldTuning::carrying(&config, 100.0, &[Some(left), Some(left)]),
            baseline
        );
        assert_eq!(
            HoldTuning::carrying(
                &config,
                ASSIST_MAX_MASS_KG + 1.0,
                &[Some(left), Some(right)]
            ),
            baseline
        );
        assert_eq!(
            HoldTuning::carrying(&config, f32::NAN, &[Some(left), Some(right)]),
            baseline
        );
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
        let weight = 3.0 * PICKUP.gravity_mps2;
        let expected = weight / PICKUP.hand_force_n;
        assert!((solution.loads[0] - expected).abs() < 0.05);
    }

    #[test]
    fn a_body_too_heavy_to_lift_sinks() {
        let config = PICKUP;
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
        let config = PICKUP;
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
        let config = PALMS;
        let deadband = config.push_deadband_m;
        // The hand pushed 20 cm past a heavy crate's grab point along -Z: the body moves back +Z.
        let push = resist_push(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -0.2),
            1.0,
            deadband,
            &config,
        );
        assert!(push.y > 0.0 && push.x.abs() < 1e-6);
        // Below the onset load the hand is still moving the body: no push.
        assert_eq!(
            resist_push(
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, -0.2),
                0.5,
                deadband,
                &config
            ),
            Vec2::ZERO
        );
        // Within the deadband: no push.
        assert_eq!(
            resist_push(
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, -0.01),
                1.0,
                deadband,
                &config
            ),
            Vec2::ZERO
        );
        let hard = resist_push(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -5.0),
            1.0,
            deadband,
            &config,
        );
        assert!((hard.length() - config.max_push_speed_mps).abs() < 1e-5);
    }

    #[test]
    fn a_carried_prop_holds_the_player_back_before_it_is_torn_away() {
        let config = PALMS;
        let carry = resist_deadband_m(true, &config);
        assert_eq!(resist_deadband_m(false, &config), config.push_deadband_m);
        assert!(carry > config.push_deadband_m && carry < PICKUP.break_distance_m);
        // A heavy barrel carried at full strength, trailing the hands by a few centimetres: the
        // player is left alone.
        assert_eq!(
            resist_push(Vec3::ZERO, Vec3::new(0.0, 0.0, -0.1), 1.0, carry, &config),
            Vec2::ZERO
        );
        // The player walks away from it: well before the break distance, the player is held back
        // toward it at full speed.
        let walked = resist_push(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -PICKUP.break_distance_m * 0.8),
            1.0,
            carry,
            &config,
        );
        assert!(walked.y > 0.0);
        assert!((walked.length() - config.max_push_speed_mps).abs() < 1e-5);
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
