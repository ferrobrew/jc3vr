//! Sitting on the ground: the hips on the ground under the player's head, held there in the world,
//! and the legs out in front, crossed with the knees out to the sides when the head leans forward
//! over them, or with the feet flat and the knees up when it is upright or back.
//!
//! Which way the legs lie is chosen when the sit begins, from how far the head has come down ahead
//! of the stance the player stood in, and changes only when the head leans the other way for a
//! moment, blending across.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::body::procedural::solver::{
    arms::Side,
    feet::FootPose,
    lower::{LowerInput, LowerRig, LowerTargets, SitLegs, facing, flat},
};

/// How high the hip joints sit above the ground while sitting, metres.
pub(super) const SIT_HIP_JOINT_M: f32 = 0.1;

/// How far above the sit's threshold the head must ask the hips to come for a sit to end,
/// metres: the hysteresis that keeps a small lift of the head from standing the body up.
pub(super) const SIT_EXIT_M: f32 = 0.12;

/// How far above the sit's threshold the head must rise before the body may sit again after the
/// stick stood it up, metres.
pub(super) const SIT_REARM_M: f32 = 0.1;

/// The fastest the head may move and still count as still for a sit, metres per second.
pub(super) const SIT_STILL_MPS: f32 = 0.25;

/// A sit: where the hips are held, which way the body faces, and how the legs lie.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Sit {
    /// The hips' place on the ground, world space.
    hips: Vec3,
    /// The hips' facing when the sit began, world space, on the ground plane.
    forward: Vec3,
    /// Whether the legs are crossed rather than up.
    crossed: bool,
    /// How far the legs have gone from up to crossed, `0..=1`, linear.
    mix: f32,
    /// How long the head has leaned toward the other way of lying, seconds.
    switch_s: f32,
    pub(super) active: bool,
}

impl Sit {
    /// Begin a sit under the player's head: the hips go where a neutral head over them would put
    /// them, and the legs cross when the head has come down well ahead of the stance.
    pub(super) fn begin(rig: &LowerRig, input: &LowerInput) -> Self {
        let turn = Quat::from_rotation_y(input.hips_yaw);
        let forward = turn * Vec3::NEG_Z;
        let hips = ground_point(input.head) - turn * ahead(rig.head_ahead);
        let support = (input.stance[0].position + input.stance[1].position) * 0.5;
        let lean = (ground_point(input.head) - ground_point(support)).dot(forward);
        let crossed = lean > CROSS_LEAN_M;
        Self {
            hips: input.world.transform_point3(hips),
            forward: flat(input.world.transform_vector3(forward)),
            crossed,
            mix: if crossed { 1.0 } else { 0.0 },
            switch_s: 0.0,
            active: true,
        }
    }

    pub(super) fn legs(&self) -> SitLegs {
        if self.crossed {
            SitLegs::Crossed
        } else {
            SitLegs::KneesUp
        }
    }

    /// Change how the legs lie once the head has leaned the other way for [`LEG_SWITCH_S`]: ahead
    /// of a neutral head over the hips by [`CROSS_LEAN_M`] crosses them, and back within
    /// [`UPRIGHT_LEAN_M`] puts the knees up.
    pub(super) fn choose_legs(&mut self, rig: &LowerRig, input: &LowerInput, dt: f32) {
        let to_model = input.world.inverse();
        let hips = ground_point(to_model.transform_point3(self.hips));
        let forward = flat(to_model.transform_vector3(self.forward));
        let neutral = facing(forward) * ahead(rig.head_ahead);
        let lean = (ground_point(input.head) - hips - neutral).dot(forward);
        let wanted = if lean > CROSS_LEAN_M {
            true
        } else if lean < UPRIGHT_LEAN_M {
            false
        } else {
            self.crossed
        };
        if wanted == self.crossed || !lean.is_finite() {
            self.switch_s = 0.0;
            return;
        }
        self.switch_s += dt;
        if self.switch_s >= LEG_SWITCH_S {
            self.crossed = wanted;
            self.switch_s = 0.0;
        }
    }

    /// Move the legs toward how they lie over [`LEG_BLEND_S`].
    pub(super) fn ease_legs(&mut self, dt: f32) {
        let step = dt / LEG_BLEND_S;
        self.mix = if self.crossed {
            (self.mix + step).min(1.0)
        } else {
            (self.mix - step).max(0.0)
        };
    }

    /// The sit's targets this tick, model space.
    pub(super) fn targets(&self, rig: &LowerRig, input: &LowerInput, weight: f32) -> LowerTargets {
        let to_model = input.world.inverse();
        let ground = ground_point(to_model.transform_point3(self.hips));
        let forward = flat(to_model.transform_vector3(self.forward));
        let right = forward.cross(Vec3::Y);
        let turn = facing(forward);
        let hips = ground + Vec3::Y * rig.sit_hips();
        let leg = rig.thigh + rig.shin;
        let m = self.mix.clamp(0.0, 1.0);
        let m = m * m * (3.0 - 2.0 * m);
        let mut feet = input.stance;
        let mut poles = [Vec3::Y; 2];
        for side in Side::BOTH {
            let i = side.index();
            let s = side.sign();
            let ankle = input.stance[i].position.y;
            let stance = turn * input.stance[i].rotation;

            // Knees up: the feet flat in front, a little wider than the hips.
            let up = ground
                + forward * (KNEES_UP_REACH * leg)
                + right * (s * (rig.hip_half_width + KNEES_UP_SPREAD_M));
            let up = Vec3::new(up.x, ankle, up.z);
            let up_pole = (Vec3::Y + right * (s * KNEES_UP_SPLAY))
                .try_normalize()
                .unwrap_or(Vec3::Y);

            // Crossed: each ankle in front of the other leg, the right one over the left, turned
            // across the body and rolled onto its outer edge, the knee out to its own side.
            let over = side == Side::Right;
            let ahead_share = if over {
                CROSS_AHEAD_OVER
            } else {
                CROSS_AHEAD_UNDER
            };
            let across = ground + forward * (ahead_share * rig.thigh)
                - right * (s * CROSS_ACROSS * rig.thigh);
            let lift = if over { CROSS_OVER_M } else { 0.0 };
            let across = Vec3::new(across.x, ankle + lift, across.z);
            let yaw = Quat::from_rotation_y(s * CROSS_YAW);
            let toes = (turn * yaw) * Vec3::NEG_Z;
            let crossed = (Quat::from_axis_angle(toes, s * CROSS_ROLL)
                * turn
                * yaw
                * input.stance[i].rotation)
                .normalize();
            let cross_pole = (right * s + forward * CROSS_POLE_AHEAD - Vec3::Y * CROSS_POLE_DOWN)
                .try_normalize()
                .unwrap_or(right * s);

            feet[i] = FootPose {
                position: up.lerp(across, m),
                rotation: stance.normalize().slerp(crossed, m).normalize(),
            };
            poles[i] = up_pole
                .lerp(cross_pole, m)
                .try_normalize()
                .unwrap_or(up_pole);
        }
        LowerTargets {
            weight,
            hips,
            feet,
            poles,
        }
    }
}

/// `point` on the ground plane of the root.
fn ground_point(point: Vec3) -> Vec3 {
    Vec3::new(point.x, 0.0, point.z)
}

/// A horizontal model-space offset `(x, z)` as a vector.
fn ahead(offset: Vec2) -> Vec3 {
    Vec3::new(offset.x, 0.0, offset.y)
}

/// How far ahead of a neutral head over the sitting hips the head leans before the legs cross,
/// metres; at the sit's start, how far ahead of the stance it came down.
const CROSS_LEAN_M: f32 = 0.12;

/// How far ahead the head may stay and still put the knees up again, metres.
const UPRIGHT_LEAN_M: f32 = 0.04;

/// How long the head leans the other way before the legs change, seconds.
const LEG_SWITCH_S: f32 = 1.0;

/// How long the legs take to go from up to crossed or back, seconds.
const LEG_BLEND_S: f32 = 0.6;

/// How far ahead the feet stand with the knees up, as a share of the leg's length.
const KNEES_UP_REACH: f32 = 0.68;

/// How much wider than the hip joints the feet stand with the knees up, metres.
const KNEES_UP_SPREAD_M: f32 = 0.05;

/// How far out the knees lean with the knees up, against straight up.
const KNEES_UP_SPLAY: f32 = 0.3;

/// How far ahead of the hips each crossed ankle lies, as a share of the thigh: the left one under,
/// the right one over.
const CROSS_AHEAD_UNDER: f32 = 0.5;
const CROSS_AHEAD_OVER: f32 = 0.7;

/// How far across the middle each crossed ankle lies, as a share of the thigh.
const CROSS_ACROSS: f32 = 0.3;

/// How much higher the crossed ankle on top lies, metres.
const CROSS_OVER_M: f32 = 0.05;

/// How far a crossed foot turns across the body, radians: about seventy degrees.
const CROSS_YAW: f32 = 1.2;

/// How far a crossed foot rolls onto its outer edge, radians: about thirty-five degrees.
const CROSS_ROLL: f32 = 0.6;

/// How a crossed knee points: out to its side, a little ahead, and a little down.
const CROSS_POLE_AHEAD: f32 = 0.4;
const CROSS_POLE_DOWN: f32 = 0.2;
