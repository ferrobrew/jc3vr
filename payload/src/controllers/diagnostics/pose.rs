//! Rico's pose as recording events: the animated pose with the HumanIK targets live at the entry
//! of the pose-finalization pass's solve, and the pose after the solves and every joint the mod
//! writes after them. Together they let a rig be replayed offline against the game's own animation
//! and compared with what rendered.
//!
//! Both samples read the [`BODY_BONES`] in model space through the checked live pose, on the thread
//! that owns the character's pass; the bone indices are resolved by name once per skeleton.

use std::sync::atomic::{AtomicU64, Ordering};

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    animation::ik::{HumanIK, Pass},
    character::character::Character,
};
use parking_lot::Mutex;
use tracking_format::{
    event::{BODY_BONES, BoneSet, IkGoal, IkTarget, PoseSample, PoseStage},
    model::Pose,
};

use crate::{
    controllers::body::procedural::skeleton::SkeletonKey,
    frame_timing::{self, Slot},
    hooks::{self, character::pose::LivePose},
    recorder::tracking::events::{self, EventKind},
};

/// Note the local player's animated pose and the live HumanIK targets, just before the pass's
/// solves run.
///
/// # Safety
///
/// `character` must be null or a live character at the entry of its pose-finalization pass, after
/// every target the mod queues there, on the thread that owns the pass.
pub(crate) unsafe fn sample_pre_ik(character: *mut Character) {
    let pass = PASSES.fetch_add(1, Ordering::Relaxed) + 1;
    if let Some(character) = unsafe { recordable(character) } {
        let targets = unsafe { live_targets(&character.m_HIK) };
        unsafe { note(character, pass, PoseStage::PreIk, targets) };
    }
}

/// Note the local player's final pose, after the solves and the mod's joint writes.
///
/// # Safety
///
/// `character` must be null or a live character at the end of its pose-finalization pass, on the
/// thread that owns the pass.
pub(crate) unsafe fn sample_final(character: *mut Character) {
    if let Some(character) = unsafe { recordable(character) } {
        let pass = PASSES.load(Ordering::Relaxed);
        unsafe { note(character, pass, PoseStage::Final, Vec::new()) };
    }
}

/// The pose-finalization passes counted since the payload loaded.
static PASSES: AtomicU64 = AtomicU64::new(0);

/// The bone indices of [`BODY_BONES`] for the skeleton last read.
static BONES: Mutex<Option<(SkeletonKey, [Option<u32>; BODY_BONES.len()])>> = Mutex::new(None);

unsafe fn recordable<'a>(character: *mut Character) -> Option<&'a mut Character> {
    if !events::collecting() || !hooks::in_gameplay() {
        return None;
    }
    unsafe { character.as_mut() }.filter(|c| c.m_IsLocalCharacter)
}

unsafe fn note(character: &mut Character, pass: u64, stage: PoseStage, targets: Vec<IkTarget>) {
    let _timing = frame_timing::scope(Slot::Recorder);
    let world = Mat4::from(character.m_WorldMatrixT1);
    let Some(controller) = (unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() })
    else {
        return;
    };
    let Some(live) = (unsafe { LivePose::new(controller) }) else {
        return;
    };
    let indices = {
        let mut cached = BONES.lock();
        match *cached {
            Some((key, indices)) if key == live.key() => indices,
            _ => {
                let indices = BODY_BONES.map(|name| live.named(name.as_bytes()));
                *cached = Some((live.key(), indices));
                indices
            }
        }
    };
    let bones = indices
        .iter()
        .map(|index| {
            let joint = live.read((*index)?)?;
            let position = Vec3::from_array(joint.m_Translation.data);
            let orientation = Quat::from_array(joint.m_Orientation.data);
            (position.is_finite() && orientation.is_finite()).then(|| Pose {
                position,
                orientation: orientation.normalize(),
            })
        })
        .collect();
    let (_, rotation, translation) = world.to_scale_rotation_translation();
    events::note(EventKind::Pose(Box::new(PoseSample {
        pass,
        stage,
        character: Pose {
            position: translation,
            orientation: rotation,
        },
        set: BoneSet::Body,
        bones,
        targets,
    })));
}

/// The `MAIN`-pass targets whose valid flag is set: the game's live claims and the mod's.
unsafe fn live_targets(hik: &HumanIK) -> Vec<IkTarget> {
    let pass = &hik.m_PassInfo[Pass::MAIN as usize];
    let weight = |table: &[f32; 44], effector: i32| {
        usize::try_from(effector)
            .ok()
            .and_then(|i| table.get(i).copied())
            .unwrap_or(0.0)
    };
    let mut targets = Vec::new();
    let positions = &pass.m_EffectorTargetPositions;
    let mut cur = positions.begin;
    while !cur.is_null() && cur < positions.end {
        // SAFETY: the pass's position list, read on the thread that owns the pass.
        let target = unsafe { &*cur };
        if target.is_valid {
            targets.push(IkTarget {
                effector: target.effector,
                goal: IkGoal::Position(Vec3::from_array(target.effector_position.data)),
                reach: weight(&hik.m_TargetReachT, target.effector),
                pull: weight(&hik.m_TargetPull, target.effector),
            });
        }
        cur = unsafe { cur.add(1) };
    }
    let rotations = &pass.m_EffectorTargetRotations;
    let mut cur = rotations.begin;
    while !cur.is_null() && cur < rotations.end {
        // SAFETY: as above, for the rotation list.
        let target = unsafe { &*cur };
        if target.is_valid {
            targets.push(IkTarget {
                effector: target.effector,
                goal: IkGoal::Rotation {
                    axis: Vec3::from_array(target.effector_rotation_axis.data),
                    angle_rad: target.effector_rotation_angle,
                },
                reach: weight(&hik.m_TargetReachR, target.effector),
                pull: weight(&hik.m_TargetPull, target.effector),
            });
        }
        cur = unsafe { cur.add(1) };
    }
    targets.truncate(MAX_TARGETS);
    targets
}

/// More targets than the solver has effectors would be a corrupt list; the event's bound is larger.
const MAX_TARGETS: usize = 64;
