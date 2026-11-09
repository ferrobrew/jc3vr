//! What the solve gate keeps between ticks ([`State`]): the rig, the torso's blend, the clavicles'
//! followers, the wingsuit's arms, and the status, behind one lock, with the steps of the gate that
//! read and update them.

use jc3gi::character::character::{AnimationController, Character};
use parking_lot::Mutex;

use crate::controllers::{
    body::procedural::{
        ArmContext, INITIAL_STATUS, ProceduralStatus,
        authority::{Case, TorsoBlend, WeightFollower},
        config::ProceduralBodyConfig,
        hips::HipFollower,
        joints::{LiveRig, RigCache},
        rig::{Rig, RigBone},
        skeleton::Transform,
        solver::{
            arms::Side,
            clavicles::{self, ClavicleFollower, ClavicleTuning, ClavicleTurn},
            pose::RigPose,
            wings::WingTarget,
        },
        stance::StanceBlend,
        state::StateTracker,
        targets::TargetFade,
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
    pub(super) legs: WeightFollower,
    pub(super) leg_weight: f32,
    pub(super) leg_fade: TargetFade,
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
        config: &ProceduralBodyConfig,
    ) -> [ClavicleTurn; 2] {
        let wanted = {
            let targets = unsafe { deferred_wrist_targets(character) };
            let tuning = ClavicleTuning {
                elevation: config.clavicle_elevation_deg.to_radians(),
                depression: config.clavicle_depression_deg.to_radians(),
                protraction: config.clavicle_protraction_deg.to_radians(),
                reach_start: config.clavicle_reach_start,
            };
            Side::BOTH.map(|side| {
                targets[side.index()].map_or(ClavicleTurn::ZERO, |(target, weight)| {
                    clavicles::wanted(pose, rig, side, target, weight, &tuning)
                })
            })
        };
        let turns = self.clavicles.step(wanted, dt, config.clavicle_follow_s);
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
) -> RigPose {
    let mut pose = *animated;
    if !weight.is_finite() || weight <= 0.0 {
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
    if pose.is_finite() { pose } else { *animated }
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
    legs: WeightFollower::new(),
    leg_weight: 0.0,
    leg_fade: TargetFade::new(),
    status: INITIAL_STATUS,
});

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec3};

    use super::*;

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
        assert_eq!(relaxed_legs(&animated, &rest.bones, 0.0), animated);
        for weight in [0.2, 0.6, 1.0] {
            let relaxed = relaxed_legs(&animated, &rest.bones, weight);
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
        let relaxed = relaxed_legs(&animated, &rest.bones, 1.0);
        assert!(
            relaxed
                .position(RigBone::LeftFoot)
                .distance(animated.position(RigBone::LeftFoot))
                > 0.1
        );
    }
}
