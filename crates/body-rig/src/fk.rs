//! The FK pelvis: where the player's pelvis, hips, knees, and ankles are, from the tracked neck
//! pivot and the learned estimate's joint rotations (`docs/mod/body/ik-body.md`, "The FK pelvis").
//!
//! The estimate carries rotations but no translation: the network places the body by aligning its
//! head joint with the tracked head. So the pelvis is found by forward kinematics down from the
//! head: on an SMPL skeleton scaled to the player's calibrated proportions, the pelvis is the neck
//! pivot less the chain from the pelvis up to the head under the estimated spine and neck
//! rotations, and the hips, knees, and ankles hang from the pelvis under the estimated hip and knee
//! rotations. A player who bends over gets a pelvis behind and below the head; one who crouches
//! gets a pelvis lower under it.
//!
//! The estimate arrives a few times a second faster than it changes and jitters a little, so the
//! points are followed by a critically damped spring relative to the neck ([`FkFollower`]). When
//! the estimate goes stale, the points ease over to a fallback pelvis (the grounded rig's waist
//! pivot, or straight below the neck without one), so nothing snaps as the estimate comes and goes.
//!
//! Joint frames are SMPL's: +X to the body's left, +Y up, +Z forward at rest, given as global
//! rotations into the room frame's axes.

use glam::{Quat, Vec3};

use crate::{
    body::PlayerBody,
    math::{
        angle::{line_yaw, wrap},
        smooth::{critically_damped, smoothstep},
    },
};

/// The estimate's global rotations of the joints the FK reads, in the room frame's axes.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyRotations {
    pub pelvis: Quat,
    pub spine1: Quat,
    pub spine2: Quat,
    pub spine3: Quat,
    pub neck: Quat,
    /// The hips, left then right: they turn the thighs.
    pub hips: [Quat; 2],
    /// The knees, left then right: they turn the shins.
    pub knees: [Quat; 2],
}

impl BodyRotations {
    /// Every joint at rest, the body facing +Z (backward in the room frame).
    pub const REST: Self = Self {
        pelvis: Quat::IDENTITY,
        spine1: Quat::IDENTITY,
        spine2: Quat::IDENTITY,
        spine3: Quat::IDENTITY,
        neck: Quat::IDENTITY,
        hips: [Quat::IDENTITY; 2],
        knees: [Quat::IDENTITY; 2],
    };

    /// Every joint turned by `rotation` from where it is, as the whole body turning or leaning
    /// rigidly.
    pub fn turned(&self, rotation: Quat) -> Self {
        let turn = |q: Quat| (rotation * q).normalize();
        Self {
            pelvis: turn(self.pelvis),
            spine1: turn(self.spine1),
            spine2: turn(self.spine2),
            spine3: turn(self.spine3),
            neck: turn(self.neck),
            hips: self.hips.map(turn),
            knees: self.knees.map(turn),
        }
    }

    fn is_finite(&self) -> bool {
        [
            self.pelvis,
            self.spine1,
            self.spine2,
            self.spine3,
            self.neck,
        ]
        .iter()
        .chain(&self.hips)
        .chain(&self.knees)
        .all(|q| q.is_finite() && q.length_squared() > 1e-6)
    }
}

/// The SMPL skeleton's offsets scaled to one player.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FkSkeleton {
    /// The scale of the chain from the pelvis to the head.
    pub spine_scale: f32,
    /// The scale of the legs below the hips.
    pub leg_scale: f32,
}

impl FkSkeleton {
    /// The skeleton of `body` whose neck pivot stands at `standing_neck` metres: the chain from the
    /// pelvis to the head as long as the rig's spine from the waist pivot to the neck, and the legs
    /// long enough to stand the ankles [`ANKLE_HEIGHT_M`] over the floor under hips at the waist
    /// pivot's height.
    pub fn for_body(body: &PlayerBody, standing_neck: f32) -> Self {
        let spine_scale = body.spine_length(standing_neck) / REST_SPINE_M;
        let hip_drop = -HIP_OFFSET.y * spine_scale;
        let legs = (body.hip_height - hip_drop - ANKLE_HEIGHT_M).max(MIN_LEG_M);
        let leg_scale = legs / REST_LEG_M;
        let valid = |s: f32| if s.is_finite() && s > 0.0 { s } else { 1.0 };
        Self {
            spine_scale: valid(spine_scale),
            leg_scale: valid(if legs.is_finite() { leg_scale } else { 1.0 }),
        }
    }

    /// How far from a hip joint the ankle reaches with the knee straight, metres.
    pub fn leg_length(&self) -> f32 {
        (KNEE_OFFSET.length() + ANKLE_OFFSET.length()) * self.leg_scale
    }

    /// The length of the chain from the pelvis to the head at rest, metres.
    pub fn spine_length(&self) -> f32 {
        REST_SPINE_M * self.spine_scale
    }
}

/// The body's points, relative to the neck pivot, room-frame axes.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FkBody {
    pub pelvis: Vec3,
    /// The first spine joint above the pelvis: with the pelvis, the pelvis's up.
    pub spine1: Vec3,
    /// The hip joints, left then right.
    pub hips: [Vec3; 2],
    /// The knees, left then right.
    pub knees: [Vec3; 2],
    /// The ankles, left then right.
    pub ankles: [Vec3; 2],
}

impl FkBody {
    /// The pelvis's heading: square to the line from the left hip to the right.
    pub fn yaw(&self) -> Option<f32> {
        line_yaw(self.hips[0], self.hips[1])
    }

    /// The pelvis's up: from the pelvis toward the first spine joint.
    pub fn up(&self) -> Option<Vec3> {
        (self.spine1 - self.pelvis).try_normalize()
    }

    /// Every point carried by `f`.
    pub fn map(&self, f: impl Fn(Vec3) -> Vec3) -> Self {
        Self {
            pelvis: f(self.pelvis),
            spine1: f(self.spine1),
            hips: self.hips.map(&f),
            knees: self.knees.map(&f),
            ankles: self.ankles.map(&f),
        }
    }

    /// Each point mixed toward `other`'s by `t`.
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mix = |a: Vec3, b: Vec3| a.lerp(b, t);
        Self {
            pelvis: mix(self.pelvis, other.pelvis),
            spine1: mix(self.spine1, other.spine1),
            hips: [
                mix(self.hips[0], other.hips[0]),
                mix(self.hips[1], other.hips[1]),
            ],
            knees: [
                mix(self.knees[0], other.knees[0]),
                mix(self.knees[1], other.knees[1]),
            ],
            ankles: [
                mix(self.ankles[0], other.ankles[0]),
                mix(self.ankles[1], other.ankles[1]),
            ],
        }
    }

    fn is_finite(&self) -> bool {
        [self.pelvis, self.spine1]
            .iter()
            .chain(&self.hips)
            .chain(&self.knees)
            .chain(&self.ankles)
            .all(|p| p.is_finite())
    }
}

/// The body's points under `rotations` on `skeleton`, relative to the neck pivot (the SMPL head
/// joint). `None` for rotations that are not finite.
pub fn solve(rotations: &BodyRotations, skeleton: &FkSkeleton) -> Option<FkBody> {
    if !rotations.is_finite() {
        return None;
    }
    let s = skeleton.spine_scale;
    let l = skeleton.leg_scale;
    let r = rotations;
    let neck = -(r.neck * HEAD_OFFSET * s);
    let spine3 = neck - r.spine3 * NECK_OFFSET * s;
    let spine2 = spine3 - r.spine2 * SPINE3_OFFSET * s;
    let spine1 = spine2 - r.spine1 * SPINE2_OFFSET * s;
    let pelvis = spine1 - r.pelvis * SPINE1_OFFSET * s;
    let side = |i: usize, v: Vec3| if i == 0 { v } else { Vec3::new(-v.x, v.y, v.z) };
    let hips = [0, 1].map(|i| pelvis + r.pelvis * side(i, HIP_OFFSET) * s);
    let knees = [0, 1].map(|i| hips[i] + r.hips[i] * side(i, KNEE_OFFSET) * l);
    let ankles = [0, 1].map(|i| knees[i] + r.knees[i] * side(i, ANKLE_OFFSET) * l);
    let body = FkBody {
        pelvis,
        spine1,
        hips,
        knees,
        ankles,
    };
    body.is_finite().then_some(body)
}

/// The fallback body with the pelvis at `pelvis` relative to the neck pivot, facing `yaw`, the
/// spine straight from it toward the neck and the legs hanging straight down from the hips.
pub fn fallback(pelvis: Vec3, yaw: f32, skeleton: &FkSkeleton) -> FkBody {
    let up = (-pelvis).try_normalize().unwrap_or(Vec3::Y);
    let facing = Quat::from_rotation_y(wrap(yaw));
    let across = facing * Vec3::X;
    let hip_half = HIP_OFFSET.x * skeleton.spine_scale;
    let hip_drop = -HIP_OFFSET.y * skeleton.spine_scale;
    let leg = skeleton.leg_length();
    let hips = [-1.0, 1.0].map(|side| pelvis + across * hip_half * side - Vec3::Y * hip_drop);
    FkBody {
        pelvis,
        spine1: pelvis + up * SPINE1_OFFSET.y * skeleton.spine_scale,
        hips,
        knees: hips.map(|hip| hip - Vec3::Y * leg * KNEE_SHARE),
        ankles: hips.map(|hip| hip - Vec3::Y * leg),
    }
}

/// The follower's tuning.
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default)
)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FkTuning {
    /// The smoothing time of the spring that follows the estimate's points, seconds.
    pub smooth_s: f32,
    /// How long the points take to ease over to the fallback when the estimate goes stale, and
    /// back when it returns, seconds.
    pub fallback_blend_s: f32,
}

impl FkTuning {
    pub const fn new() -> Self {
        Self {
            smooth_s: 0.12,
            fallback_blend_s: 0.3,
        }
    }
}

impl Default for FkTuning {
    fn default() -> Self {
        Self::new()
    }
}

/// Where the followed body comes from this frame.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum PelvisSource {
    /// The estimate, fully.
    Estimate,
    /// Easing between the estimate and the fallback.
    Blending,
    /// The fallback alone: no estimate, or a stale one.
    #[default]
    Fallback,
}

impl PelvisSource {
    pub fn label(self) -> &'static str {
        match self {
            PelvisSource::Estimate => "estimate",
            PelvisSource::Blending => "blending",
            PelvisSource::Fallback => "fallback",
        }
    }
}

/// One frame's inputs to the follower.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FkFrame {
    /// The estimate's rotations, while it is fresh.
    pub rotations: Option<BodyRotations>,
    /// The fallback pelvis relative to the neck pivot: the grounded rig's waist pivot, or `None`
    /// for straight below the neck at the spine's length.
    pub fallback_pelvis: Option<Vec3>,
    /// The heading the fallback faces.
    pub fallback_yaw: f32,
    pub skeleton: FkSkeleton,
    pub dt: f32,
}

/// The followed body, relative to the neck pivot.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FkOutput {
    pub body: FkBody,
    pub source: PelvisSource,
    /// The estimate's share of the body, `0..=1`.
    pub estimate_weight: f32,
}

/// The spring that follows the estimate's body, and its hand-over to the fallback.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct FkFollower {
    /// The followed estimate body and its rate, once an estimate has arrived.
    followed: Option<(FkBody, FkBody)>,
    /// The estimate's share, `0..=1`.
    weight: f32,
}

impl FkFollower {
    pub const fn new() -> Self {
        Self {
            followed: None,
            weight: 0.0,
        }
    }

    /// Forget the followed body: the next estimate is taken whole.
    pub fn forget(&mut self) {
        *self = Self::new();
    }

    /// Advance one frame.
    pub fn update(&mut self, frame: &FkFrame, tuning: &FkTuning) -> FkOutput {
        let dt = if frame.dt.is_finite() {
            frame.dt.max(0.0)
        } else {
            0.0
        };
        let solved = frame
            .rotations
            .as_ref()
            .and_then(|r| solve(r, &frame.skeleton));
        if let Some(target) = solved {
            self.followed = Some(match self.followed {
                Some((body, rate)) => follow(&body, &rate, &target, tuning.smooth_s, dt),
                None => (target, zero_body()),
            });
        }
        let step = if tuning.fallback_blend_s > 0.0 && tuning.fallback_blend_s.is_finite() {
            dt / tuning.fallback_blend_s
        } else {
            1.0
        };
        let goal = if solved.is_some() { 1.0 } else { 0.0 };
        self.weight += (goal - self.weight).clamp(-step, step);
        let below = Vec3::NEG_Y * frame.skeleton.spine_length();
        let pelvis = frame
            .fallback_pelvis
            .filter(|p| p.is_finite())
            .unwrap_or(below);
        let fallback = fallback(pelvis, frame.fallback_yaw, &frame.skeleton);
        if self.weight <= 0.0 {
            self.followed = None;
        }
        let (body, source) = match self.followed {
            Some((followed, _)) if self.weight >= 1.0 => (followed, PelvisSource::Estimate),
            Some((followed, _)) if self.weight > 0.0 => (
                fallback.lerp(&followed, smoothstep(self.weight)),
                PelvisSource::Blending,
            ),
            _ => (fallback, PelvisSource::Fallback),
        };
        FkOutput {
            body,
            source,
            estimate_weight: self.weight,
        }
    }
}

/// How far the ankles stand over the floor, metres: a generic anthropometric figure.
pub const ANKLE_HEIGHT_M: f32 = 0.08;

/// The SMPL neutral body's parent-relative joint offsets at rest, metres, in the joint frame (+X
/// left, +Y up, +Z forward), symmetrized: the published template's regressed joints.
const SPINE1_OFFSET: Vec3 = Vec3::new(0.0, 0.1255, -0.0239);
const SPINE2_OFFSET: Vec3 = Vec3::new(0.0, 0.1381, 0.0099);
const SPINE3_OFFSET: Vec3 = Vec3::new(0.0, 0.0517, 0.0254);
const NECK_OFFSET: Vec3 = Vec3::new(0.0, 0.2130, -0.0546);
const HEAD_OFFSET: Vec3 = Vec3::new(0.0, 0.0838, 0.0492);
/// The left hip from the pelvis; the right mirrors it across +X.
const HIP_OFFSET: Vec3 = Vec3::new(0.0597, -0.0925, -0.0107);
/// The left knee from the left hip.
const KNEE_OFFSET: Vec3 = Vec3::new(0.0457, -0.3840, 0.0065);
/// The left ankle from the left knee.
const ANKLE_OFFSET: Vec3 = Vec3::new(-0.0125, -0.4245, -0.0429);

/// The vertical length of the chain from the pelvis to the head at rest, metres.
const REST_SPINE_M: f32 =
    SPINE1_OFFSET.y + SPINE2_OFFSET.y + SPINE3_OFFSET.y + NECK_OFFSET.y + HEAD_OFFSET.y;

/// The vertical length of a leg from the hip to the ankle at rest, metres.
const REST_LEG_M: f32 = -(KNEE_OFFSET.y + ANKLE_OFFSET.y);

/// The share of the leg's length from the hip down to the knee at rest.
const KNEE_SHARE: f32 = KNEE_OFFSET.y / (KNEE_OFFSET.y + ANKLE_OFFSET.y);

/// The shortest leg the FK works with, metres, so a garbled calibration cannot fold it away.
const MIN_LEG_M: f32 = 0.3;

fn zero_body() -> FkBody {
    FkBody {
        pelvis: Vec3::ZERO,
        spine1: Vec3::ZERO,
        hips: [Vec3::ZERO; 2],
        knees: [Vec3::ZERO; 2],
        ankles: [Vec3::ZERO; 2],
    }
}

/// One spring step of every point of `body`, changing at `rate`, toward `target`.
fn follow(
    body: &FkBody,
    rate: &FkBody,
    target: &FkBody,
    smooth_s: f32,
    dt: f32,
) -> (FkBody, FkBody) {
    let step = |p: Vec3, v: Vec3, t: Vec3| {
        let (x, vx) = critically_damped(p.x, v.x, t.x, smooth_s, dt);
        let (y, vy) = critically_damped(p.y, v.y, t.y, smooth_s, dt);
        let (z, vz) = critically_damped(p.z, v.z, t.z, smooth_s, dt);
        (Vec3::new(x, y, z), Vec3::new(vx, vy, vz))
    };
    let (pelvis, pelvis_rate) = step(body.pelvis, rate.pelvis, target.pelvis);
    let (spine1, spine1_rate) = step(body.spine1, rate.spine1, target.spine1);
    let pair = |a: [Vec3; 2], v: [Vec3; 2], t: [Vec3; 2]| {
        let l = step(a[0], v[0], t[0]);
        let r = step(a[1], v[1], t[1]);
        ([l.0, r.0], [l.1, r.1])
    };
    let (hips, hips_rate) = pair(body.hips, rate.hips, target.hips);
    let (knees, knees_rate) = pair(body.knees, rate.knees, target.knees);
    let (ankles, ankles_rate) = pair(body.ankles, rate.ankles, target.ankles);
    let next = FkBody {
        pelvis,
        spine1,
        hips,
        knees,
        ankles,
    };
    let next_rate = FkBody {
        pelvis: pelvis_rate,
        spine1: spine1_rate,
        hips: hips_rate,
        knees: knees_rate,
        ankles: ankles_rate,
    };
    if next.is_finite() && next_rate.is_finite() {
        (next, next_rate)
    } else {
        (*target, zero_body())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const DT: f32 = 1.0 / 90.0;

    fn body() -> PlayerBody {
        PlayerBody::from_heights(1.8, 1.68)
    }

    fn skeleton() -> FkSkeleton {
        let body = body();
        FkSkeleton::for_body(&body, body.standing_neck())
    }

    /// Facing -Z in the room: SMPL's +Z forward turned half way round.
    fn facing_forward() -> Quat {
        Quat::from_rotation_y(std::f32::consts::PI)
    }

    fn standing() -> BodyRotations {
        BodyRotations::REST.turned(facing_forward())
    }

    #[test]
    fn standing_puts_the_pelvis_at_the_waist_pivot_under_the_neck() {
        let body = body();
        let skeleton = skeleton();
        let fk = solve(&standing(), &skeleton).unwrap();
        // The chain's height is the rig's spine.
        assert!((-fk.pelvis.y - body.spine_length(body.standing_neck())).abs() < 1e-4);
        // Nearly under the neck: the SMPL chain's forward offsets nearly cancel.
        assert!(
            fk.pelvis.x.abs() < 1e-5 && fk.pelvis.z.abs() < 0.02,
            "{}",
            fk.pelvis
        );
        // The ankles stand about their height over the floor.
        let floor = -body.standing_neck();
        for ankle in fk.ankles {
            let over = ankle.y - floor;
            assert!((over - ANKLE_HEIGHT_M).abs() < 0.03, "{over}");
        }
        // Facing -Z, the left hip is at -X.
        assert!(fk.hips[0].x < 0.0 && fk.hips[1].x > 0.0);
        assert!(fk.yaw().unwrap().abs() < 1e-4);
        assert!(fk.up().unwrap().y > 0.95);
    }

    #[test]
    fn a_bend_at_the_hips_puts_the_pelvis_behind_and_below_the_head() {
        let skeleton = skeleton();
        let upright = solve(&standing(), &skeleton).unwrap();
        // The upper body pitched 60 degrees forward about the pelvis: every spine joint turns.
        let bend = Quat::from_rotation_x(-60f32.to_radians());
        let mut rotations = standing();
        for q in [
            &mut rotations.pelvis,
            &mut rotations.spine1,
            &mut rotations.spine2,
            &mut rotations.spine3,
            &mut rotations.neck,
        ] {
            *q = bend * *q;
        }
        let bent = solve(&rotations, &skeleton).unwrap();
        // The head is ahead of the pelvis, so the pelvis is behind the neck: +Z.
        assert!(bent.pelvis.z > upright.pelvis.z + 0.3, "{}", bent.pelvis);
        // And the neck nearer the pelvis's height.
        assert!(bent.pelvis.y > upright.pelvis.y + 0.2, "{}", bent.pelvis);
    }

    #[test]
    fn turning_the_body_turns_the_pelvis_heading() {
        let skeleton = skeleton();
        let turn = 0.7;
        let fk = solve(&standing().turned(Quat::from_rotation_y(turn)), &skeleton).unwrap();
        assert!((wrap(fk.yaw().unwrap() - turn)).abs() < 1e-4);
    }

    #[test]
    fn bent_knees_bring_the_ankles_up_toward_the_hips() {
        let skeleton = skeleton();
        let mut rotations = standing();
        // A crouch: the thighs forward and the shins back.
        let thigh = Quat::from_rotation_x(70f32.to_radians());
        let shin = Quat::from_rotation_x(-70f32.to_radians());
        rotations.hips = rotations.hips.map(|q| thigh * q);
        rotations.knees = rotations.knees.map(|q| shin * q);
        let upright = solve(&standing(), &skeleton).unwrap();
        let crouched = solve(&rotations, &skeleton).unwrap();
        for i in 0..2 {
            let reach = |b: &FkBody| b.hips[i].distance(b.ankles[i]);
            assert!(reach(&crouched) < reach(&upright) * 0.85);
            // The knee comes forward: -Z.
            assert!(crouched.knees[i].z < upright.knees[i].z - 0.2);
        }
    }

    #[test]
    fn garbled_rotations_give_no_body() {
        let mut rotations = standing();
        rotations.spine2 = Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0);
        assert_eq!(solve(&rotations, &skeleton()), None);
        rotations.spine2 = Quat::from_xyzw(0.0, 0.0, 0.0, 0.0);
        assert_eq!(solve(&rotations, &skeleton()), None);
    }

    #[test]
    fn a_garbled_calibration_still_gives_a_usable_skeleton() {
        let bad = PlayerBody {
            eye_height: f32::NAN,
            hip_height: f32::NAN,
        };
        let skeleton = FkSkeleton::for_body(&bad, f32::NAN);
        assert!(skeleton.spine_scale.is_finite() && skeleton.spine_scale > 0.0);
        assert!(skeleton.leg_scale.is_finite() && skeleton.leg_scale > 0.0);
    }

    fn frame(rotations: Option<BodyRotations>) -> FkFrame {
        FkFrame {
            rotations,
            fallback_pelvis: None,
            fallback_yaw: 0.0,
            skeleton: skeleton(),
            dt: DT,
        }
    }

    #[test]
    fn the_first_estimate_is_eased_in_from_the_fallback_without_a_snap() {
        let mut follower = FkFollower::new();
        let first = follower.update(&frame(None), &FkTuning::new());
        assert_eq!(first.source, PelvisSource::Fallback);
        let mut last = first.body.pelvis;
        let mut sources = Vec::new();
        for _ in 0..90 {
            let out = follower.update(&frame(Some(standing())), &FkTuning::new());
            assert!(out.body.pelvis.distance(last) < 0.01, "a snap");
            last = out.body.pelvis;
            sources.push(out.source);
        }
        assert!(sources.contains(&PelvisSource::Blending));
        assert_eq!(*sources.last().unwrap(), PelvisSource::Estimate);
    }

    #[test]
    fn a_stale_estimate_hands_over_to_the_rigs_waist() {
        let mut follower = FkFollower::new();
        for _ in 0..90 {
            follower.update(&frame(Some(standing())), &FkTuning::new());
        }
        let waist = Vec3::new(0.1, -0.6, 0.05);
        let mut out = follower.update(&frame(Some(standing())), &FkTuning::new());
        for _ in 0..90 {
            out = follower.update(
                &FkFrame {
                    fallback_pelvis: Some(waist),
                    ..frame(None)
                },
                &FkTuning::new(),
            );
        }
        assert_eq!(out.source, PelvisSource::Fallback);
        assert!(out.body.pelvis.distance(waist) < 1e-5);
        assert_eq!(out.estimate_weight, 0.0);
    }

    #[test]
    fn the_spring_takes_out_the_estimates_jitter() {
        let mut follower = FkFollower::new();
        for _ in 0..120 {
            follower.update(&frame(Some(standing())), &FkTuning::new());
        }
        let rest = solve(&standing(), &skeleton()).unwrap().pelvis;
        // Every other frame the estimate's pelvis tips 4 degrees either way.
        let mut furthest = 0.0f32;
        for i in 0..180 {
            let tip = Quat::from_rotation_x(if i % 2 == 0 { 0.07 } else { -0.07 });
            let jittered = standing().turned(tip);
            let out = follower.update(&frame(Some(jittered)), &FkTuning::new());
            furthest = furthest.max(out.body.pelvis.distance(rest));
        }
        let raw = solve(&standing().turned(Quat::from_rotation_x(0.07)), &skeleton())
            .unwrap()
            .pelvis
            .distance(rest);
        assert!(furthest < raw * 0.3, "{furthest} against {raw}");
    }

    proptest! {
        #[test]
        fn the_chain_keeps_its_length_however_the_body_turns(
            yaw in -3.1f32..3.1, pitch in -1.2f32..1.2, roll in -0.6f32..0.6,
        ) {
            let skeleton = skeleton();
            let turn = Quat::from_euler(glam::EulerRot::YXZ, yaw, pitch, roll);
            let fk = solve(&standing().turned(turn), &skeleton).unwrap();
            let rest = solve(&standing(), &skeleton).unwrap();
            prop_assert!((fk.pelvis.length() - rest.pelvis.length()).abs() < 1e-4);
            for i in 0..2 {
                let leg = |b: &FkBody| b.hips[i].distance(b.ankles[i]);
                prop_assert!((leg(&fk) - leg(&rest)).abs() < 1e-4);
                prop_assert!(leg(&fk) <= skeleton.leg_length() + 1e-4);
            }
        }

        #[test]
        fn the_follower_stays_finite(
            frames in proptest::collection::vec((any::<bool>(), -1.0f32..1.0, 0.0f32..0.1), 1..200),
        ) {
            let mut follower = FkFollower::new();
            for (present, tip, dt) in frames {
                let rotations = present.then(|| standing().turned(Quat::from_rotation_x(tip)));
                let out = follower.update(&FkFrame { dt, ..frame(rotations) }, &FkTuning::new());
                prop_assert!(out.body.is_finite());
                prop_assert!((0.0..=1.0).contains(&out.estimate_weight));
            }
        }
    }
}
