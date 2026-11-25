//! Rico's pose as recording events: the animated pose with the HumanIK targets live at the entry
//! of the pose-finalization pass's solve, and the pose after the solves and every joint the mod
//! writes after them. Together they let a rig be replayed offline against the game's own animation
//! and compared with what rendered.
//!
//! Both samples read the [`BODY_BONES`] in model space through the checked live pose, on the thread
//! that owns the character's pass; the bone indices are resolved by name once per skeleton.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    animation::ik::{HumanIK, Pass},
    character::character::Character,
    graphics_engine::render_block::Matrix3x4,
};
use parking_lot::Mutex;
use tracking_format::{
    event::{BODY_BONES, BoneSet, Diagnostic, IkGoal, IkTarget, Label, PoseSample, PoseStage},
    model::Pose,
};

use crate::{
    controllers::{
        body::procedural::skeleton::SkeletonKey,
        diagnostics::world_state::pose_of,
        hands::palette::{PaletteLayout, entry_to_matrix},
    },
    frame_timing::{self, Slot},
    hooks::{self, character::pose::LivePose},
    recorder::tracking::events::{self, EventKind},
    stereo,
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

pub(crate) fn drawn_arm_joints(
    entries: &[Matrix3x4],
    layout: PaletteLayout,
    bones: [[usize; 3]; 2],
    bind_points: [[Vec3; 3]; 2],
) -> Option<[[Vec3; 3]; 2]> {
    let mut joints = [[Vec3::ZERO; 3]; 2];
    for side in 0..2 {
        for joint in 0..3 {
            let entry = entries.get(bones[side][joint])?;
            let matrix = entry_to_matrix(&entry.m, layout);
            let bind = bind_points[side][joint];
            let point = matrix.transform_point3(bind);
            if !(matrix.is_finite() && bind.is_finite() && point.is_finite()) {
                return None;
            }
            joints[side][joint] = point;
        }
    }
    Some(joints)
}

pub(crate) fn sample_drawn_arms(sequence: u64, dtf: f32, joints: [[Vec3; 3]; 2]) {
    if stereo::draw_index() != 0
        || !hooks::in_gameplay()
        || !dtf.is_finite()
        || !joints.iter().flatten().all(|point| point.is_finite())
    {
        return;
    }
    let collecting = events::collecting();
    let trace_enabled = tracing::enabled!(target: "controllers", tracing::Level::DEBUG);
    if !collecting && !trace_enabled {
        return;
    }
    let now = Instant::now();
    let trace = {
        let mut sampled = DRAWN.lock();
        if sampled.sequence.is_some_and(|last| sequence <= last) {
            return;
        }
        sampled.sequence = Some(sequence);
        let trace = trace_enabled
            && sampled
                .traced_at
                .is_none_or(|last| now.saturating_duration_since(last) >= DRAWN_TRACE_INTERVAL);
        if trace {
            sampled.traced_at = Some(now);
        }
        trace
    };
    let [
        [left_shoulder, left_elbow, left_wrist],
        [right_shoulder, right_elbow, right_wrist],
    ] = joints.map(|arm| arm.map(|point| point.to_array()));
    if trace {
        tracing::debug!(
            target: "controllers",
            stage = "post_presentation_palette",
            space = "model",
            sequence,
            dtf,
            left_shoulder_m = ?left_shoulder,
            left_elbow_m = ?left_elbow,
            left_wrist_m = ?left_wrist,
            right_shoulder_m = ?right_shoulder,
            right_elbow_m = ?right_elbow,
            right_wrist_m = ?right_wrist,
            "drawn arm joints"
        );
    }
    if collecting {
        events::note_with(|| {
            EventKind::Diagnostic(Box::new(Diagnostic {
                source: Label::Borrowed("presentation"),
                hand: None,
                what: Label::Borrowed("drawn arm joints"),
                detail: Some(Label::Owned(format!(
                    "stage=post_presentation_palette space=model sequence={sequence} dtf={dtf:.6} \
                     left_shoulder_m={left_shoulder:?} left_elbow_m={left_elbow:?} left_wrist_m={left_wrist:?} \
                     right_shoulder_m={right_shoulder:?} right_elbow_m={right_elbow:?} right_wrist_m={right_wrist:?}"
                ))),
            }))
        });
    }
}

const DRAWN_TRACE_INTERVAL: Duration = Duration::from_millis(500);

struct DrawnSample {
    sequence: Option<u64>,
    traced_at: Option<Instant>,
}

static DRAWN: Mutex<DrawnSample> = Mutex::new(DrawnSample {
    sequence: None,
    traced_at: None,
});

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
    events::note(EventKind::Pose(Box::new(PoseSample {
        pass,
        stage,
        character: pose_of(world),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::hands::palette::matrix_to_entry;

    #[test]
    fn drawn_joints_use_bind_points_in_both_layouts() {
        let bind = [[Vec3::X, Vec3::Y, Vec3::Z], [-Vec3::X, -Vec3::Y, -Vec3::Z]];
        let bones = [[0, 1, 2], [3, 4, 5]];
        let edits: [Mat4; 6] = std::array::from_fn(|i| {
            Mat4::from_scale_rotation_translation(
                Vec3::splat(1.0 + i as f32 * 0.1),
                Quat::from_rotation_z(i as f32 * 0.2),
                Vec3::new(i as f32, 2.0, -3.0),
            )
        });
        for layout in [PaletteLayout::Rows, PaletteLayout::Columns] {
            let entries = edits.map(|edit| Matrix3x4 {
                m: matrix_to_entry(&edit, layout),
            });
            let drawn = drawn_arm_joints(&entries, layout, bones, bind).unwrap();
            for side in 0..2 {
                for joint in 0..3 {
                    let expected = edits[bones[side][joint]].transform_point3(bind[side][joint]);
                    assert!(drawn[side][joint].abs_diff_eq(expected, 1e-5));
                }
            }
        }
    }

    #[test]
    fn drawn_joints_reject_incomplete_or_non_finite_palettes() {
        let layout = PaletteLayout::Columns;
        let identity = Matrix3x4 {
            m: matrix_to_entry(&Mat4::IDENTITY, layout),
        };
        let bones = [[0, 1, 2], [3, 4, 5]];
        let bind = [[Vec3::ZERO; 3]; 2];
        assert!(drawn_arm_joints(&[identity; 5], layout, bones, bind).is_none());
        let mut entries = [identity; 6];
        entries[4].m[0] = f32::NAN;
        assert!(drawn_arm_joints(&entries, layout, bones, bind).is_none());
        let mut bad_bind = bind;
        bad_bind[0][1].x = f32::INFINITY;
        assert!(drawn_arm_joints(&[identity; 6], layout, bones, bad_bind).is_none());
    }
}
