//! Fixed-budget deterministic coordinate search with anatomical rejection before tracking costs.

use glam::{Quat, Vec3};

use crate::controllers::{
    body::{
        procedural::solver::two_bone::solve,
        rig::{
            estimate::{
                Anatomy, Body, EstimateError, Stance,
                anatomy::Segments,
                contacts::{Contact, rest_feet, yaw},
                temporal::{self, Envelope},
                work::Work,
            },
            torso::{wrap, yaw_forward},
        },
    },
    frame::Pose,
};

/// Stance X/Z/yaw, pelvis offset X/Z/height, hip bend, spine bend, and chest twist.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Parameters(pub [f32; PARAMETERS]);

#[derive(Clone)]
pub(crate) struct Context<'a> {
    pub anatomy: &'a Anatomy,
    pub neck: Vec3,
    pub head_yaw: f32,
    /// Completed root facing anchors weak inferred heading, independently of head and wrist axes.
    pub root_yaw: f32,
    pub hands: [Option<Pose>; 2],
    /// Tracking floor in world space. It is not virtual terrain evidence.
    pub tracking_floor: Vec3,
    pub world_scale: f32,
    pub predicted: Parameters,
    pub previous: Parameters,
    pub rates: [f32; PARAMETERS],
    pub dt: f32,
    pub continuity: bool,
    pub floor_y: f32,
    pub contacts: [Contact; 2],
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct Candidate {
    pub parameters: Parameters,
    pub body: Body,
    pub player_shoulders: [Vec3; 2],
    pub remapped_hands: [Option<Vec3>; 2],
    pub neck_error: f32,
    pub arm_errors: [Option<f32>; 2],
    pub neck_limit_error: f32,
    pub contact_errors: [f32; 2],
    pub tracking_cost: f32,
    pub secondary_cost: f32,
}

impl Parameters {
    pub(crate) fn neutral(stance: Stance, anatomy: &Anatomy) -> Self {
        Self([
            stance.origin.x,
            stance.origin.z,
            stance.yaw,
            0.0,
            0.0,
            anatomy.character.neutral_hip_height * 0.995,
            0.0,
            0.0,
            0.0,
        ])
    }

    pub(crate) fn stance(self, floor_y: f32) -> Stance {
        Stance {
            origin: Vec3::new(self.0[0], floor_y, self.0[1]),
            yaw: self.0[2],
        }
    }

    pub(crate) fn translated(mut self, displacement: Vec3, turn: f32) -> Self {
        self.0[0] += displacement.x;
        self.0[1] += displacement.z;
        self.0[2] = wrap(self.0[2] + turn);
        self
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SearchStatus {
    Admissible,
    ResidualAfterBudget,
}

/// The four seed families are generic geometry, not activity-specific posture branches.
pub(crate) fn recovery_seeds(context: &Context<'_>) -> [Parameters; SEEDS] {
    let neutral = Parameters::neutral(context.previous.stance(context.floor_y), context.anatomy);
    let mut projected = neutral;
    let player = context.anatomy.player_segments;
    let ratio = context.anatomy.character.neutral_hip_height
        / (player.neutral_hip_height * context.world_scale);
    projected.0[5] =
        (context.neck.y - context.tracking_floor.y - player.torso() * context.world_scale) * ratio;
    projected.0[0] = context.neck.x;
    projected.0[1] = context.neck.z;
    let mut coupled = context.previous;
    let delta = Quat::from_rotation_y(-coupled.0[2]) * (context.neck - context.tracking_floor);
    let bend = (-delta.z / (player.torso() * context.world_scale))
        .clamp(-0.14, 0.999)
        .asin();
    coupled.0[6] = bend;
    coupled.0[7] = bend * 0.15;
    coupled.0[4] = 0.30 * context.anatomy.character.torso() * bend.sin();
    coupled.0[5] = (context.neck.y
        - context.tracking_floor.y
        - (player.lower_torso * bend.cos() + player.upper_torso * (bend * 1.15).cos())
            * context.world_scale)
        * ratio;
    [context.predicted, neutral, projected, coupled]
}

pub(crate) fn search(context: &Context<'_>) -> Result<(Candidate, usize), EstimateError> {
    let mut work = Work::default();
    let envelope = Envelope::new(context, &mut work);
    let result = search_counted(context, Some(&envelope), &mut work);
    #[cfg(test)]
    {
        let expected = search_counted(context, None, &mut Work::default());
        crate::controllers::body::rig::estimate::tests::cost::assert_same_search(result, expected);
    }
    result.map(|candidate| (candidate, work.evaluations))
}

pub(crate) fn search_counted(
    context: &Context<'_>,
    envelope: Option<&Envelope>,
    work: &mut Work,
) -> Result<Candidate, EstimateError> {
    work.searches += 1;
    let mut best = None;
    for mut seed in recovery_seeds(context) {
        work.check();
        temporal::constrain_counted(&mut seed, context, envelope, work)?;
        let mut local = temporal_candidate(seed, context, envelope, work);
        for sweep in 0..SWEEPS {
            let shrink = 0.5_f32.powi(sweep as i32 / 2);
            for axis in 0..PARAMETERS {
                let centre = local.map_or(seed, |candidate: Candidate| candidate.parameters);
                for sign in [-1.0, 1.0] {
                    let mut next = centre;
                    let step = if context.continuity {
                        SEARCH_STEPS[axis].min(MAX_SPEED[axis] * context.dt * 0.5)
                    } else {
                        SEARCH_STEPS[axis]
                    };
                    let amount = step * shrink * sign;
                    if axis < 2 {
                        let direction = Quat::from_rotation_y(context.root_yaw)
                            * if axis == 0 { Vec3::X } else { Vec3::Z };
                        next.0[0] += direction.x * amount;
                        next.0[1] += direction.z * amount;
                    } else {
                        next.0[axis] += amount;
                    }
                    work.check();
                    temporal::constrain_counted(&mut next, context, envelope, work)?;
                    let candidate = temporal_candidate(next, context, envelope, work);
                    if let Some(candidate) = candidate
                        && local.is_none_or(|held| better(candidate, held))
                    {
                        local = Some(candidate);
                    }
                }
            }
        }
        if let Some(candidate) = local
            && best.is_none_or(|held| better(candidate, held))
        {
            best = Some(candidate);
        }
    }
    best.ok_or(EstimateError::SearchExhausted)
}

/// Probe the reported chest heading, with compensating stance/twist allocations.
/// This finite coverage can establish ambiguity, but cannot establish uniqueness.
pub(crate) fn ambiguity(candidate: Candidate, context: &Context<'_>) -> (f32, usize) {
    ambiguity_counted(candidate, context, &mut Work::default())
}

pub(crate) fn ambiguity_counted(
    candidate: Candidate,
    context: &Context<'_>,
    work: &mut Work,
) -> (f32, usize) {
    let mut admissible = 0;
    for turn in [-0.7, -0.35, 0.35, 0.7] {
        let mut admitted = false;
        for stance_share in [0.0, 0.5, 1.0] {
            let mut parameters = candidate.parameters;
            parameters.0[2] = wrap(parameters.0[2] + turn * stance_share);
            parameters.0[8] += turn * (1.0 - stance_share);
            work.check();
            admitted |= evaluate_counted(parameters, context, work).is_some_and(measurement_admissible);
        }
        if admitted {
            admissible += 1;
        }
    }
    (admissible as f32 / 4.0, HEADING_PROBES)
}

pub(crate) fn measurement_admissible(candidate: Candidate) -> bool {
    candidate.tracking_cost <= 1.0e-5
}

/// Final publishing uses realized ankles; residuals retain the error against the final request.
pub(crate) fn publish_feet(candidate: &mut Candidate, contacts: &mut [Contact; 2]) {
    for (side, contact) in contacts.iter_mut().enumerate() {
        contact.pose.position = candidate.body.ankles[side];
        if !contact.active() {
            candidate.contact_errors[side] = 0.0;
        }
    }
}

pub(crate) fn evaluate(parameters: Parameters, context: &Context<'_>) -> Option<Candidate> {
    evaluate_counted(parameters, context, &mut Work::default())
}

pub(crate) fn evaluate_counted(
    parameters: Parameters,
    context: &Context<'_>,
    work: &mut Work,
) -> Option<Candidate> {
    let q = parameters.0;
    if q.iter().any(|v| !v.is_finite()) || !within_anatomy(parameters, context.anatomy) {
        return None;
    }
    let segments = context.anatomy.character;
    let stance = parameters.stance(context.floor_y);
    let rotation = Quat::from_rotation_y(q[2]);
    let pelvis =
        stance.origin + rotation * Vec3::new(q[3], q[5] + context.anatomy.ankle_height, q[4]);
    let chest_rotation = Quat::from_rotation_y(q[2] + q[8]);
    let chest = pelvis + rotation * Quat::from_rotation_x(-q[6]) * Vec3::Y * segments.lower_torso;
    let neck = chest
        + chest_rotation * Quat::from_rotation_x(-q[6] - q[7]) * Vec3::Y * segments.upper_torso;
    let shoulders = [-1.0, 1.0]
        .map(|sign| chest + chest_rotation * Vec3::X * (sign * segments.shoulder_width * 0.5));
    let hips =
        [-1.0, 1.0].map(|sign| pelvis + rotation * Vec3::X * (sign * segments.hip_width * 0.5));
    let rest = rest_feet(stance, segments, context.anatomy.ankle_height);
    let mut knees = [Vec3::ZERO; 2];
    let mut ankles = [Vec3::ZERO; 2];
    let mut knee_flexion = [0.0; 2];
    let mut ankle_flexion = [0.0; 2];
    let mut contact_errors = [0.0; 2];
    let mut active_contact_cost = 0.0;
    let mut hip_cost = 0.0;
    let forward = yaw_forward(q[2]);
    for side in 0..2 {
        let contact = context.contacts[side];
        let active = !matches!(
            contact.state,
            crate::controllers::body::rig::estimate::ContactState::Released(_)
        );
        let target = if active {
            contact.pose.position
        } else {
            rest[side].position
        };
        let leg = leg(hips[side], target, forward, segments, side, work);
        knees[side] = leg.knee;
        ankles[side] = leg.ankle;
        knee_flexion[side] = leg.knee_flexion;
        ankle_flexion[side] = leg.ankle_flexion;
        contact_errors[side] = if active {
            leg.ankle.distance(target)
        } else {
            0.0
        };
        active_contact_cost += (contact_errors[side] / 0.03).powi(2);
        let thigh = (leg.knee - hips[side]).normalize_or(Vec3::NEG_Y);
        let sagittal = thigh.dot(forward).atan2(-thigh.y);
        let hip_joint_flexion = q[6] + sagittal;
        if !full_leg_valid(&leg, hips[side], parameters) {
            return None;
        }
        hip_cost += 0.15 * (hip_joint_flexion - 0.55 * leg.knee_flexion - 0.65 * q[6]).powi(2);
        if contact.planted()
            && (contact_errors[side] > CONTACT_TOLERANCE
                || leg.knee_flexion > KNEE_LIMIT
                || leg.ankle_flexion.abs() > ANKLE_LIMIT
                || wrap(
                    crate::controllers::body::rig::estimate::contacts::yaw(contact.pose.rotation)
                        - q[2],
                )
                .abs()
                    > LEG_TWIST_LIMIT)
        {
            return None;
        }
        if leg.knee_flexion > KNEE_LIMIT || leg.ankle_flexion.abs() > ANKLE_LIMIT {
            return None;
        }
    }
    if !realized_separation(ankles, q[2], segments.hip_width) {
        return None;
    }
    for (side, hip) in hips.iter().enumerate() {
        if context.contacts[side].planted()
            && hip.distance(context.contacts[side].pose.position) > segments.leg(side) * 0.995
        {
            return None;
        }
    }
    let player = context.anatomy.player_segments;
    let player_scale = context.world_scale;
    let leg_ratio = player.neutral_hip_height * player_scale / segments.neutral_hip_height;
    let player_pelvis = context.tracking_floor
        + (stance.origin - context.tracking_floor) * Vec3::new(1.0, 0.0, 1.0)
        + rotation * Vec3::new(q[3] * leg_ratio, q[5] * leg_ratio, q[4] * leg_ratio);
    let player_chest = player_pelvis
        + rotation * Quat::from_rotation_x(-q[6]) * Vec3::Y * (player.lower_torso * player_scale);
    let player_neck = player_chest
        + chest_rotation
            * Quat::from_rotation_x(-q[6] - q[7])
            * Vec3::Y
            * (player.upper_torso * player_scale);
    let player_shoulders = [-1.0, 1.0].map(|sign| {
        player_chest
            + chest_rotation * Vec3::X * (sign * player.shoulder_width * player_scale * 0.5)
    });
    let neck_error = player_neck.distance(context.neck);
    let mut arm_errors = [None; 2];
    let mut remapped_hands = [None; 2];
    let mut tracking_cost = tolerance_cost(neck_error, NECK_TOLERANCE * player_scale);
    for side in 0..2 {
        if let Some(hand) = context.hands[side] {
            let delta = hand.position - player_shoulders[side];
            let reach = player.arms[side] * player_scale;
            let minimum = (context.anatomy.player.upper_arm - context.anatomy.player.forearm).abs()
                * player_scale;
            let residual =
                (delta.length() - reach * 0.985).max(0.0) + (minimum - delta.length()).max(0.0);
            arm_errors[side] = Some(residual);
            tracking_cost += tolerance_cost(residual, HAND_TOLERANCE * player_scale);
            remapped_hands[side] = Some(shoulders[side] + delta * (segments.arms[side] / reach));
        }
    }
    let neck_twist = wrap(context.head_yaw - q[2] - q[8]);
    let neck_limit_error = (neck_twist.abs() - NECK_LIMIT).max(0.0);
    tracking_cost += (neck_limit_error / 0.1).powi(2);
    let knee_mean = (knee_flexion[0] + knee_flexion[1]) * 0.5;
    let ankle_mean = (ankle_flexion[0] + ankle_flexion[1]) * 0.5;
    let mean_foot = (ankles[0] + ankles[1]) * 0.5;
    let mass = pelvis * 0.55 + chest * 0.35 + neck * 0.1;
    let support_balance = horizontal(mass - mean_foot).length() / segments.leg(0);
    // The comfortable region couples flexion and rearward pelvis compensation continuously.
    let preferred_back =
        0.30 * segments.torso() * q[6].sin() + 0.12 * segments.leg(0) * knee_mean.sin();
    let coupling = ((q[4] - preferred_back) / segments.leg(0)).powi(2)
        + 0.12 * (q[6] - 0.28 * knee_mean - 0.45 * ankle_mean).powi(2)
        + 0.15 * (ankle_mean - 0.25 * knee_mean).powi(2);
    let held_heading = wrap(q[2] - context.root_yaw) / LEG_TWIST_LIMIT;
    let carried_heading = wrap(q[2] - context.previous.0[2]) / LEG_TWIST_LIMIT;
    let mut secondary_cost = active_contact_cost
        + hip_cost
        + 3.5 * coupling
        + 0.22 * knee_mean.powi(2)
        + 0.12 * q[6].powi(2)
        + 0.8 * q[7].powi(2)
        + 0.4 * q[8].powi(2)
        + 2.0 * support_balance.powi(2)
        + 2.0 * (q[3] / segments.leg(0)).powi(2)
        + 5.0 * held_heading.powi(2)
        + 5.0 * carried_heading.powi(2);
    for angle in knee_flexion {
        secondary_cost += 0.4 * ((angle / KNEE_LIMIT - 0.75).max(0.0) / 0.25).powi(2);
    }
    if context.continuity {
        let inverse = Quat::from_rotation_y(-context.root_yaw);
        let planar_delta = inverse
            * Vec3::new(
                q[0] - context.predicted.0[0],
                0.0,
                q[1] - context.predicted.0[1],
            );
        let planar_step = inverse
            * Vec3::new(
                q[0] - context.previous.0[0],
                0.0,
                q[1] - context.previous.0[1],
            );
        let planar_rate = inverse * Vec3::new(context.rates[0], 0.0, context.rates[1]);
        for (axis, scale) in CONTINUITY_SCALES.into_iter().enumerate() {
            let (delta, step, rate) = match axis {
                0 => (planar_delta.x, planar_step.x, planar_rate.x),
                1 => (planar_delta.z, planar_step.z, planar_rate.z),
                _ => (
                    difference(q[axis], context.predicted.0[axis], axis),
                    difference(q[axis], context.previous.0[axis], axis),
                    context.rates[axis],
                ),
            };
            let velocity = step / context.dt;
            secondary_cost += 0.20 * (delta / scale).powi(2)
                + 0.002
                    * ((velocity - rate) / (MAX_ACCELERATION[axis] * context.dt).max(0.1)).powi(2);
        }
    }
    // Stance transport competes with articulation inside the admissible tracking region.
    let retained = context.contacts.iter().filter(|contact| contact.planted());
    let (sum, count) = retained.fold((Vec3::ZERO, 0_u32), |(sum, count), contact| {
        (sum + contact.pose.position, count + 1)
    });
    if count > 0 {
        secondary_cost += 7.0 * horizontal(stance.origin - sum / count as f32).length_squared()
            / segments.leg(0).powi(2);
    }
    secondary_cost += 5.0
        * horizontal(stance.origin - context.previous.stance(context.floor_y).origin)
            .length_squared()
        / segments.leg(0).powi(2);
    let body = Body {
        pelvis,
        pelvis_rotation: rotation,
        chest,
        neck,
        shoulders,
        hips,
        knees,
        ankles,
        chest_yaw: wrap(q[2] + q[8]),
        hip_flexion: q[6],
        spine_flexion: q[7],
        knee_flexion,
        ankle_flexion,
        neck_twist,
    };
    (tracking_cost.is_finite() && secondary_cost.is_finite()).then_some(Candidate {
        parameters,
        body,
        player_shoulders,
        remapped_hands,
        neck_error,
        arm_errors,
        neck_limit_error,
        contact_errors,
        tracking_cost,
        secondary_cost,
    })
}

pub(crate) fn constrain(
    parameters: &mut Parameters,
    context: &Context<'_>,
) -> Result<(), EstimateError> {
    temporal::constrain(parameters, context)
}

/// Separation is checked after both legs realize, including released unconstrained legs.
pub(crate) fn realized_separation(ankles: [Vec3; 2], heading: f32, hip_width: f32) -> bool {
    ankles.iter().all(|ankle| ankle.is_finite())
        && (ankles[1] - ankles[0]).dot(Quat::from_rotation_y(heading) * Vec3::X) >= hip_width * 0.5
}

pub(crate) fn endpoint_feasible(
    candidate: Candidate,
    foot: crate::controllers::body::rig::legs::swing::FootPose,
    other: Vec3,
    segments: Segments,
    side: usize,
) -> bool {
    endpoint_feasible_counted(candidate, foot, other, segments, side, &mut Work::default())
}

pub(crate) fn endpoint_feasible_counted(
    candidate: Candidate,
    foot: crate::controllers::body::rig::legs::swing::FootPose,
    other: Vec3,
    segments: Segments,
    side: usize,
    work: &mut Work,
) -> bool {
    let hip = candidate.body.hips[side];
    if !hip.is_finite()
        || !foot.position.is_finite()
        || !foot.rotation.is_finite()
        || !other.is_finite()
        || (foot.rotation.length_squared() - 1.0).abs() > 1.0e-4
    {
        return false;
    }
    let distance = hip.distance(foot.position);
    let minimum = (segments.thighs[side] - segments.shins[side]).abs() + 0.01;
    let heading = candidate.parameters.0[2];
    let right = Quat::from_rotation_y(heading) * Vec3::X;
    let sign = if side == 0 { -1.0 } else { 1.0 };
    if distance < minimum
        || distance > segments.leg(side) * 0.995
        || (foot.position - other).dot(right) * sign < segments.hip_width * 0.5
        || wrap(yaw(foot.rotation) - heading).abs() > LEG_TWIST_LIMIT
    {
        return false;
    }
    let leg = leg(hip, foot.position, yaw_forward(heading), segments, side, work);
    full_leg_valid(&leg, hip, candidate.parameters)
        && leg.ankle.distance(foot.position) <= CONTACT_TOLERANCE
}

fn full_leg_valid(leg: &Leg, hip: Vec3, parameters: Parameters) -> bool {
    let heading = parameters.0[2];
    let thigh = (leg.knee - hip).normalize_or(Vec3::NEG_Y);
    let sagittal = thigh.dot(yaw_forward(heading)).atan2(-thigh.y);
    let flexion = parameters.0[6] + sagittal;
    let abduction = thigh
        .dot(Quat::from_rotation_y(heading) * Vec3::X)
        .clamp(-1.0, 1.0)
        .asin();
    (-0.65..=2.5).contains(&flexion)
        && abduction.abs() <= 0.9
        && leg.knee_flexion <= KNEE_LIMIT
        && leg.ankle_flexion.abs() <= ANKLE_LIMIT
}

pub(crate) fn difference(value: f32, previous: f32, axis: usize) -> f32 {
    if axis == 2 || axis == 8 {
        wrap(value - previous)
    } else {
        value - previous
    }
}

pub(crate) fn horizontal(value: Vec3) -> Vec3 {
    Vec3::new(value.x, 0.0, value.z)
}

fn temporal_candidate(
    parameters: Parameters,
    context: &Context<'_>,
    envelope: Option<&Envelope>,
    work: &mut Work,
) -> Option<Candidate> {
    temporal::accepts_counted(parameters, context, envelope, work)
        .then(|| evaluate_counted(parameters, context, work))
        .flatten()
}

fn within_anatomy(parameters: Parameters, anatomy: &Anatomy) -> bool {
    let q = parameters.0;
    (-0.45..=0.45).contains(&q[3])
        && (-0.40..=0.65).contains(&q[4])
        && (anatomy.character.leg(0) * 0.12..=anatomy.character.leg(0) * 0.998).contains(&q[5])
        && (-0.15..=HIP_LIMIT).contains(&q[6])
        && (-0.35..=SPINE_LIMIT).contains(&q[7])
        && q[8].abs() <= CHEST_LIMIT
}

fn better(candidate: Candidate, held: Candidate) -> bool {
    if (candidate.tracking_cost - held.tracking_cost).abs() > 1.0e-5 {
        candidate.tracking_cost < held.tracking_cost
    } else {
        candidate.secondary_cost < held.secondary_cost - 1.0e-6
    }
}

fn tolerance_cost(error: f32, tolerance: f32) -> f32 {
    ((error - tolerance).max(0.0) / tolerance).powi(2)
}

struct Leg {
    knee: Vec3,
    ankle: Vec3,
    knee_flexion: f32,
    ankle_flexion: f32,
}

fn leg(
    hip: Vec3,
    target: Vec3,
    forward: Vec3,
    segments: Segments,
    side: usize,
    work: &mut Work,
) -> Leg {
    work.analytic_solves += 1;
    let thigh = segments.thighs[side];
    let shin = segments.shins[side];
    let upper = Vec3::NEG_Y * thigh;
    let lower = Vec3::new(0.0, -0.9998, 0.02).normalize() * shin;
    let middle = hip + upper;
    let end = middle + lower;
    let rotations = solve(hip, middle, end, target, forward);
    let knee = hip + rotations.upper * upper;
    let moved_end = hip + rotations.upper * (end - hip);
    let ankle = knee + rotations.lower * (moved_end - knee);
    let thigh_down = (knee - hip).normalize_or(Vec3::NEG_Y);
    let shin_down = (ankle - knee).normalize_or(Vec3::NEG_Y);
    Leg {
        knee,
        ankle,
        knee_flexion: thigh_down.dot(shin_down).clamp(-1.0, 1.0).acos(),
        ankle_flexion: shin_down.dot(forward).atan2(-shin_down.y),
    }
}

pub(crate) const PARAMETERS: usize = 9;
pub(crate) const SEARCH_EVALUATIONS: usize = SEEDS * (1 + SWEEPS * PARAMETERS * 2);
pub(crate) const HEADING_PROBES: usize = 4 * 3;
const SEEDS: usize = 4;
pub(crate) const MAX_SPEED: [f32; PARAMETERS] = [2.5, 2.5, 2.5, 1.8, 1.8, 2.0, 3.5, 2.0, 2.8];
pub(crate) const MAX_ACCELERATION: [f32; PARAMETERS] =
    [15.0, 15.0, 14.0, 16.0, 16.0, 18.0, 20.0, 15.0, 18.0];
pub(crate) const CONTACT_TOLERANCE: f32 = 0.004;
pub(crate) const NECK_TOLERANCE: f32 = 0.025;
pub(crate) const HAND_TOLERANCE: f32 = 0.025;
pub(crate) const NECK_LIMIT: f32 = 1.22;
const SWEEPS: usize = 12;
const HIP_LIMIT: f32 = 1.75;
const SPINE_LIMIT: f32 = 0.75;
const CHEST_LIMIT: f32 = 0.70;
const KNEE_LIMIT: f32 = 2.65;
const ANKLE_LIMIT: f32 = 0.95;
const LEG_TWIST_LIMIT: f32 = 1.05;
const SEARCH_STEPS: [f32; PARAMETERS] = [0.12, 0.12, 0.16, 0.08, 0.10, 0.12, 0.18, 0.10, 0.14];
const CONTINUITY_SCALES: [f32; PARAMETERS] = [0.12, 0.12, 0.3, 0.15, 0.15, 0.2, 0.4, 0.3, 0.3];
