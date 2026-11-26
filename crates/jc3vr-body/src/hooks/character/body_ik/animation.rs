//! Synthetic head-frame capture at the pre-IK publication seam.

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    animation::pose_producer::QsTransform, character::character::AnimationController,
    types::math::Matrix4,
};
use jc3vr_core::published::Stamp;
use jc3vr_view::headpose::{
    Source,
    anchors::{Context, Identity, Sample},
    animation::{RotationFilter, eye_arm, head_frame, synthetic_rotation},
};
use parking_lot::Mutex;

use crate::{hooks::character::tuning::CHARACTER, tuning::FRESHNESS};

/// All gameplay modes share this history. A mode switch does not reseed the view from a transient
/// entry pose; pause, tracking loss, recenter, teleport, and skeleton changes break continuity.
/// Apply the filtered orientation while retaining the current animated eye translation.
/// The caller serializes the native controller and has already checked its bone selection.
pub(super) fn capture(
    bind: Quat,
    identity: Identity,
    context: Context,
    sample: &mut Sample,
    tick: u64,
    dt: f32,
) {
    let mut state = STATE.lock();
    let now = Stamp::now();
    let continuous = state.identity == Some(identity)
        && state.tick.and_then(|tick| tick.checked_add(1)) == Some(tick)
        && state.root.is_some_and(|root| {
            root.distance(sample.root.w_axis.truncate()) < CHARACTER.get().body_ik.max_root_step_m
        })
        && state
            .context
            .is_some_and(|previous| previous.continuous(context))
        && FRESHNESS
            .get()
            .hooks
            .body_ik_animation_gap_ms
            .window()
            .holds(state.at, now);
    if !continuous {
        state.filter.clear();
    }
    let (_, root, _) = sample.root.to_scale_rotation_translation();
    let target = head_frame(root, sample.head_orientation, bind);
    let filtered = synthetic_rotation(&mut state.filter, root, target, dt, !continuous);
    if context.source == Source::Vr {
        sample.geometry.animation_rotation = filtered;
        let neck_to_eyes = sample.root.transform_point3(sample.eyes_model)
            - sample.root.transform_point3(sample.neck_model);
        sample.geometry.eye_arm = eye_arm(root, filtered, neck_to_eyes);
    }
    state.identity = Some(identity);
    state.context = Some(context);
    state.at = Some(now);
    state.tick = Some(tick);
    state.root = Some(sample.root.w_axis.truncate());
}

pub(super) fn clear() {
    *STATE.lock() = State::default();
}

pub(super) unsafe fn bind_rotation(controller: &AnimationController, index: u32) -> Option<Quat> {
    let skeleton = unsafe { controller.m_Skeleton.as_ref() }?;
    let havok = unsafe { skeleton.m_HavokSkeleton.as_ref() }?;
    let count = havok.m_Bones.size;
    if count <= 0
        || index >= count as u32
        || havok.m_ReferencePose.size != count
        || havok.m_ReferencePose.data.is_null()
        || havok.m_ParentIndices.size != count
        || havok.m_ParentIndices.data.is_null()
        || !(havok.m_ReferencePose.data as usize)
            .is_multiple_of(std::mem::align_of::<QsTransform>())
        || !(havok.m_ParentIndices.data as usize).is_multiple_of(std::mem::align_of::<i16>())
    {
        return None;
    }
    let parents = unsafe { std::slice::from_raw_parts(havok.m_ParentIndices.data, count as usize) };
    if parents
        .iter()
        .enumerate()
        .any(|(bone, &parent)| parent != -1 && (parent < 0 || parent as usize >= bone))
    {
        return None;
    }
    let mut matrix = Matrix4::default();
    unsafe { controller.GetBindPoseBoneMatrix(index, &mut matrix) };
    let matrix = Mat4::from(matrix);
    if !matrix.is_finite() || matrix.determinant().abs() <= f32::EPSILON {
        return None;
    }
    let (_, rotation, _) = matrix.to_scale_rotation_translation();
    (rotation.is_finite() && rotation.length_squared() > f32::EPSILON).then(|| rotation.normalize())
}

#[derive(Default)]
struct State {
    identity: Option<Identity>,
    context: Option<Context>,
    at: Option<Stamp>,
    tick: Option<u64>,
    root: Option<Vec3>,
    filter: RotationFilter,
}

static STATE: Mutex<State> = Mutex::new(State {
    identity: None,
    context: None,
    at: None,
    tick: None,
    root: None,
    filter: RotationFilter::new(),
});
