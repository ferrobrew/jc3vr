//! What the solve gate keeps between ticks ([`State`]): the rig, the torso's blend, the clavicles'
//! followers, the wingsuit's arms, and the status, behind one lock, with the steps of the gate that
//! read and update them.

use body_rig::math::{Side, constants::GRAVITY_MPS2};
use glam::{Quat, Vec3};
use jc3gi::character::character::{AnimationController, Character};
use parking_lot::Mutex;

use crate::controllers::{
    body::procedural::{
        ArmContext, INITIAL_STATUS, ProceduralStatus,
        authority::{Case, TorsoBlend, WeightFollower},
        hips::HipFollower,
        joints::{LiveRig, RigCache},
        rig::{Rig, RigBone},
        skeleton::Transform,
        solver::{
            clavicles::{self, ClavicleFollower, ClavicleTurn},
            pose::RigPose,
            wings::WingTarget,
        },
        stance::StanceBlend,
        state::StateTracker,
        targets::TargetFade,
        tuning::{PROCEDURAL, ProceduralTuning},
        wings::WingQueue,
    },
    hands::ik::deferred_wrist_targets,
};

pub(super) struct State {
    /// The rig of the skeleton the animation controller holds.
    pub(super) rig: RigCache,
    /// The torso's weight against the animation.
    pub(super) blend: TorsoBlend,
    /// What the hand-over table reads from the character, with the history it keeps.
    pub(super) tracker: StateTracker,
    /// The case and the torso's target last logged.
    pub(super) logged: Option<(Case, f32)>,
    /// The clavicles' turns toward the hands.
    pub(super) clavicles: ClavicleFollower,
    /// The fade of the owned bones' IK targets.
    pub(super) fade: TargetFade,
    /// The case the torso was last logged for, while it lasts.
    pub(super) on_hips_logged: Option<Case>,
    /// The wingsuit's arms' weight against the animation.
    pub(super) wings: WeightFollower,
    /// The wingsuit's last wrist targets, model space, which fade where they were as the arms ease
    /// out.
    pub(super) last_wings: Option<[Option<WingTarget>; 2]>,
    /// This tick's wingsuit wrist targets, queued once the gate lets go of its state.
    pub(super) wing_queue: Option<WingQueue>,
    /// Whether the wingsuit's arms have been logged since they last eased in.
    pub(super) wing_logged: bool,
    /// This tick's arm context, for the hand IK the gate runs next.
    pub(super) arms: Option<ArmContext>,
    /// The clip's hips' heading against Rico's facing, followed on foot.
    pub(super) hips: HipFollower,
    /// The square stance's blend against the clip's feet.
    pub(super) stance: StanceBlend,
    /// The share of the gait's forward lean taken out of the torso, eased across the cases.
    pub(super) delean: WeightFollower,
    pub(super) legs: WeightFollower,
    pub(super) leg_weight: f32,
    pub(super) leg_fade: TargetFade,
    pub(super) leg_swing: LegSwing,
    pub(super) status: ProceduralStatus,
}

impl State {
    /// The rig of `controller`'s skeleton, checked against it.
    pub(super) unsafe fn rig_for(&mut self, controller: &AnimationController) -> Option<LiveRig> {
        let lookup = unsafe { self.rig.get(controller) };
        if lookup.rebuilt {
            self.legs = WeightFollower::new();
            self.leg_weight = 0.0;
            self.leg_fade.forget();
            self.leg_swing = LegSwing::new();
            self.status.rig_error = lookup.error;
        }
        lookup.rig
    }

    /// The clavicles' turns this tick: toward the wrist targets the hand IK queues next, eased.
    ///
    /// # Safety
    ///
    /// As [`crate::controllers::body::procedural::at_solve_gate`].
    pub(super) unsafe fn turn_clavicles(
        &mut self,
        character: &Character,
        rig: &Rig,
        pose: &RigPose,
        dt: f32,
        config: &ProceduralTuning,
    ) -> [ClavicleTurn; 2] {
        let wanted = {
            let targets = unsafe { deferred_wrist_targets(character) };
            Side::BOTH.map(|side| {
                targets[side.index()].map_or(ClavicleTurn::ZERO, |(target, weight)| {
                    clavicles::wanted(pose, rig, side, target, weight, &config.clavicles)
                })
            })
        };
        let turns = self.clavicles.step(wanted, dt, config.clavicles.follow_s);
        self.status.clavicles = turns;
        turns
    }

    /// Log a change of the hand-over table's case, or of the torso's target within one (a flight's
    /// hold on the torso running out), with the torso's weight it leaves.
    pub(super) fn log_case(&mut self, case: Case, target: f32, weight: f32) {
        if self.logged == Some((case, target)) {
            return;
        }
        self.logged = Some((case, target));
        tracing::info!(
            target: "body",
            from = self.status.case.label(),
            to = case.label(),
            torso = target,
            torso_now = weight,
            "procedural body: the hand-over case changes",
        );
    }
}

pub(super) fn relaxed_legs(
    animated: &RigPose,
    reference: &[Transform; RigBone::COUNT],
    weight: f32,
    down: Vec3,
) -> RigPose {
    let mut pose = *animated;
    if !weight.is_finite() || weight <= 0.0 || !down.is_finite() {
        return pose;
    }
    let weight = weight.min(1.0);
    for bone in RigBone::ALL {
        if bone == RigBone::Pelvis || !bone.descends_from(RigBone::Pelvis) {
            continue;
        }
        let parent = bone.parent().unwrap();
        let current = animated.get(bone);
        let old_parent = animated.get(parent);
        let rest = reference[bone.index()];
        let rest_parent = reference[parent.index()];
        let local = old_parent.rotation.inverse() * (current.translation - old_parent.translation)
            / old_parent.scale;
        let rest_local = rest_parent.rotation.inverse()
            * (rest.translation - rest_parent.translation)
            / rest_parent.scale;
        let direction = local
            .lerp(rest_local, weight)
            .try_normalize()
            .or_else(|| rest_local.try_normalize());
        let offset = direction.map_or(local, |direction| direction * local.length());
        let rotation = (old_parent.rotation.inverse() * current.rotation)
            .slerp(rest_parent.rotation.inverse() * rest.rotation, weight)
            .normalize();
        let parent = *pose.get(parent);
        pose.bones[bone.index()] = parent.compose(&Transform {
            translation: offset,
            rotation,
            scale: current.scale / old_parent.scale,
        });
    }
    let Some(down) = down.try_normalize() else {
        return *animated;
    };
    for (thigh, knee) in [
        (RigBone::LeftUpLeg, RigBone::LeftLeg),
        (RigBone::RightUpLeg, RigBone::RightLeg),
    ] {
        let pivot = pose.position(thigh);
        if let Some(direction) = (pose.position(knee) - pivot).try_normalize() {
            let turn = Quat::IDENTITY.slerp(Quat::from_rotation_arc(direction, down), weight);
            pose.rotate_about(thigh, turn, pivot);
        }
    }
    if pose.is_finite() { pose } else { *animated }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct LegSwing {
    root: Option<Vec3>,
    velocity: Option<Vec3>,
    offset: Vec3,
    speed: Vec3,
}

impl LegSwing {
    const fn new() -> Self {
        Self {
            root: None,
            velocity: None,
            offset: Vec3::ZERO,
            speed: Vec3::ZERO,
        }
    }

    pub(super) fn step(&mut self, root: Vec3, active: bool, dt: f32) -> Vec3 {
        let procedural = PROCEDURAL.get();
        let tuning = &procedural.leg_swing;
        if !active
            || !root.is_finite()
            || !dt.is_finite()
            || !(0.0..=procedural.blend.max_step_s).contains(&dt)
        {
            *self = Self::new();
            return Vec3::NEG_Y;
        }
        if dt == 0.0 {
            return self.direction();
        }
        let previous = self.root.replace(root);
        let Some(previous) = previous else {
            return self.direction();
        };
        let delta = root - previous;
        if delta.length() > tuning.jump_m || dt < 1e-4 {
            *self = Self::new();
            self.root = Some(root);
            return Vec3::NEG_Y;
        }
        let velocity = delta / dt;
        let acceleration = self.velocity.replace(velocity).map_or(Vec3::ZERO, |old| {
            ((velocity - old) / dt).clamp_length_max(tuning.max_acceleration)
        });
        let acceleration = Vec3::new(acceleration.x, 0.0, acceleration.z);
        let steps = (dt / tuning.substep_s).ceil() as u32;
        let step = dt / steps as f32;
        for _ in 0..steps {
            self.speed += (-self.offset * (GRAVITY_MPS2 / tuning.length_m)
                - self.speed * tuning.damping
                - acceleration / tuning.length_m)
                * step;
            self.speed = self.speed.clamp_length_max(tuning.max_rate);
            self.offset += self.speed * step;
            if self.offset.length() > tuning.max_offset {
                self.offset = self.offset.normalize() * tuning.max_offset;
                let outward = self.speed.dot(self.offset.normalize()).max(0.0);
                self.speed -= self.offset.normalize() * outward;
            }
        }
        self.direction()
    }

    fn direction(&self) -> Vec3 {
        (Vec3::NEG_Y + self.offset).normalize()
    }
}

pub(super) static STATE: Mutex<State> = Mutex::new(State {
    rig: RigCache::new(),
    blend: TorsoBlend::new(),
    tracker: StateTracker::new(),
    logged: None,
    clavicles: ClavicleFollower::new(),
    fade: TargetFade::new(),
    on_hips_logged: None,
    wings: WeightFollower::new(),
    last_wings: None,
    wing_queue: None,
    wing_logged: false,
    arms: None,
    hips: HipFollower::new(),
    stance: StanceBlend::new(),
    delean: WeightFollower::new(),
    legs: WeightFollower::new(),
    leg_weight: 0.0,
    leg_fade: TargetFade::new(),
    leg_swing: LegSwing::new(),
    status: INITIAL_STATUS,
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_lags_acceleration_then_overshoots_and_settles() {
        let mut swing = LegSwing::new();
        let dt = 1.0 / 60.0;
        swing.step(Vec3::ZERO, true, dt);
        swing.step(Vec3::ZERO, true, dt);
        let root = Vec3::X * dt;
        assert!(swing.step(root, true, dt).x < 0.0);
        let mut crossed = false;
        let mut direction = Vec3::NEG_Y;
        for tick in 1..=600 {
            direction = swing.step(root + Vec3::X * dt * tick as f32, true, dt);
            crossed |= direction.x > 0.001;
            assert!(direction.is_finite());
            assert!(direction.y < -0.5);
        }
        assert!(crossed);
        assert!(direction.distance(Vec3::NEG_Y) < 1e-4);
    }

    #[test]
    fn inactive_invalid_and_teleported_motion_reset_the_swing() {
        let mut moving = LegSwing::new();
        moving.step(Vec3::ZERO, true, 0.02);
        moving.step(Vec3::ZERO, true, 0.02);
        moving.step(Vec3::X * 0.1, true, 0.02);
        assert!(moving.direction().x < 0.0);
        for (root, active, dt) in [
            (Vec3::ZERO, false, 0.02),
            (Vec3::NAN, true, 0.02),
            (Vec3::ZERO, true, f32::NAN),
            (Vec3::ZERO, true, -0.02),
            (Vec3::ZERO, true, 0.2),
        ] {
            let mut swing = moving;
            assert_eq!(swing.step(root, active, dt), Vec3::NEG_Y);
            assert_eq!(swing, LegSwing::new());
            assert_eq!(swing.step(Vec3::splat(500.0), true, 0.02), Vec3::NEG_Y);
        }
        let mut swing = moving;
        assert_eq!(swing.step(Vec3::splat(500.0), true, 0.02), Vec3::NEG_Y);
        assert_eq!(swing.step(Vec3::splat(500.0), true, 0.02), Vec3::NEG_Y);
        let unchanged = moving;
        moving.step(Vec3::splat(1.0), true, 0.0);
        assert_eq!(moving, unchanged);
    }

    #[test]
    fn swing_is_yaw_equivariant_and_consistent_across_tick_rates() {
        let yaw = Quat::from_rotation_y(0.7);
        let simulate = |hz: u32, rotation: Quat| {
            let mut swing = LegSwing::new();
            let dt = 1.0 / hz as f32;
            let mut direction = Vec3::NEG_Y;
            for tick in 0..=hz {
                let time = tick as f32 * dt;
                let root = Vec3::new(1.5 * time * time, 0.0, -time * time);
                direction = swing.step(rotation * root, true, dt);
            }
            direction
        };
        let direction = simulate(60, Quat::IDENTITY);
        assert!(direction.distance(Vec3::NEG_Y) > 0.1);
        assert!(simulate(60, yaw).distance(yaw * direction) < 1e-5);
        assert!(simulate(120, Quat::IDENTITY).distance(direction) < 0.02);
    }

    #[test]
    fn relaxed_chains_keep_the_animated_pelvis_and_segment_lengths() {
        let identity = Transform {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        };
        let mut rest = RigPose {
            bones: [identity; RigBone::COUNT],
        };
        for bone in RigBone::ALL {
            if let Some(parent) = bone.parent() {
                let offset = match bone {
                    RigBone::LeftUpLeg => Vec3::new(-0.12, 0.0, 0.0),
                    RigBone::RightUpLeg => Vec3::new(0.12, 0.0, 0.0),
                    RigBone::LeftToeBase | RigBone::RightToeBase => Vec3::NEG_Z * 0.15,
                    _ => Vec3::NEG_Y * 0.4,
                };
                rest.bones[bone.index()].translation = rest.position(parent) + offset;
            }
        }
        let mut animated = rest;
        animated.rotate_about(RigBone::Hips, Quat::from_rotation_y(0.6), Vec3::ZERO);
        for transform in &mut animated.bones {
            transform.translation += Vec3::new(2.0, 4.0, -3.0);
        }
        animated.rotate_about(
            RigBone::LeftUpLeg,
            Quat::from_rotation_x(1.0),
            animated.position(RigBone::LeftUpLeg),
        );
        animated.rotate_about(
            RigBone::RightLeg,
            Quat::from_rotation_x(-0.7),
            animated.position(RigBone::RightLeg),
        );
        assert_eq!(
            relaxed_legs(&animated, &rest.bones, 0.0, Vec3::NEG_Y),
            animated
        );
        for invalid in [Vec3::ZERO, Vec3::NAN] {
            assert_eq!(relaxed_legs(&animated, &rest.bones, 1.0, invalid), animated);
        }
        for weight in [f32::NAN, -1.0] {
            assert_eq!(
                relaxed_legs(&animated, &rest.bones, weight, Vec3::NEG_Y),
                animated
            );
        }
        let down = Vec3::new(-0.5, -1.0, 0.3).normalize();
        for weight in [0.2, 0.6, 1.0] {
            let relaxed = relaxed_legs(&animated, &rest.bones, weight, down);
            for bone in RigBone::ALL {
                if bone != RigBone::Pelvis && bone.descends_from(RigBone::Pelvis) {
                    let parent = bone.parent().unwrap();
                    let length = animated.position(bone).distance(animated.position(parent));
                    assert!(
                        (relaxed.position(bone).distance(relaxed.position(parent)) - length).abs()
                            < 1e-5
                    );
                } else {
                    assert_eq!(relaxed.get(bone), animated.get(bone));
                }
            }
        }
        let relaxed = relaxed_legs(&animated, &rest.bones, 1.0, down);
        for (thigh, knee) in [
            (RigBone::LeftUpLeg, RigBone::LeftLeg),
            (RigBone::RightUpLeg, RigBone::RightLeg),
        ] {
            let direction = (relaxed.position(knee) - relaxed.position(thigh)).normalize();
            assert!(direction.distance(down) < 1e-5);
        }
        let yaw = Quat::from_rotation_y(-0.8);
        let mut turned = animated;
        turned.rotate_about(RigBone::Hips, yaw, Vec3::ZERO);
        let turned_relaxed = relaxed_legs(&turned, &rest.bones, 1.0, yaw * down);
        for bone in RigBone::ALL {
            assert!(
                turned_relaxed
                    .position(bone)
                    .distance(yaw * relaxed.position(bone))
                    < 1e-5
            );
        }
        assert!(
            relaxed
                .position(RigBone::LeftFoot)
                .distance(animated.position(RigBone::LeftFoot))
                > 0.1
        );
    }
}
