//! The grapple slot follows the off-hand: the plan of record's "The grapple"
//! (`docs/mod/input/controllers-and-roomscale.md`), with the engine side in
//! `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! Every grapple consumer reads the grapple slot ([`TargetType::TARGET_GRAPPLE`]) of the player's
//! aim control, and every part of the aim control that fills that slot takes its ray from one of two
//! getters: the adjusted camera matrix ([`PlayerAimControl::GetAdjustedCameraMatrix`]) and the
//! camera matrix (`GameCameraManager::GetCameraMatrix`). So rather than writing the slot by hand,
//! the engine fills it from the grapple ray: while a thread-local view override is set, both
//! getters return a camera matrix built from the grapple ray ([`ray::view_matrix`]).
//!
//! - The direct-aim raycast ([`update_direct_aim`]) runs twice. The first pass, under the override,
//!   fills every slot from the grapple ray with the engine's own filter, range, holey-geometry, and
//!   target resolution; the grapple slot is taken out of it. The second pass, without the override,
//!   refills every other slot from the camera exactly as the game does, and the grapple slot is put
//!   back.
//! - The grapple target scoring ([`set_target_grapple_fitness`]) runs under the override, so that
//!   entity attachment and tethering select their target the way the game does, but around the
//!   hand's ray. Its visibility check ([`is_target_visible_cast`]) is a synchronous cast from the
//!   hand's ray origin instead: the engine keeps one scheduled visibility request per target and
//!   answers every target type from its last result, so queuing the grapple's from the hand would
//!   answer the camera-based types from the hand too, and the grapple's from the camera.
//! - The last-valid grapple cache is re-validated under the override
//!   ([`is_last_valid_grapple_aim_pos_still_valid`]); the cache itself is derived by the game from
//!   the slot in the same update.
//!
//! The per-slot game objects are `boost::weak_ptr`s. Moving one out and back is a raw copy with
//! the source zeroed, which leaves its weak count unchanged; clearing one goes through the engine's
//! own assignment, which releases the weak reference.

use std::{cell::Cell, ffi::c_void, ptr};

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    aim::{
        aim::PlayerAimControl,
        target::{Target, TargetType},
    },
    types::{
        math::{Matrix4, Vector3},
        shared_ptr::WeakPtrGameObject,
    },
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::aim::{
    ray::DIRECT_AIM_START_OFFSET,
    raycast::{self, CastOptions},
    targets,
};

/// The grapple hooks, aggregated by [`crate::controllers::aim::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_DIRECT_AIM_BINDER)
        .with_static_binder(&GET_ADJUSTED_CAMERA_MATRIX_BINDER)
        .with_static_binder(&SET_TARGET_GRAPPLE_FITNESS_BINDER)
        .with_static_binder(&IS_TARGET_VISIBLE_CAST_BINDER)
        .with_static_binder(&IS_LAST_VALID_GRAPPLE_AIM_POS_STILL_VALID_BINDER)
}

/// The camera matrix the camera getters return while the grapple ray stands in for the camera,
/// for the `GameCameraManager::GetCameraMatrix` detour in [`crate::hooks::camera`]. `None` outside
/// the grapple's scoped calls.
pub(crate) fn camera_matrix_override() -> Option<Matrix4> {
    OVERRIDE.with(Cell::get)
}

/// Set the grapple view for the player's aim update on this thread: the camera matrix of the
/// grapple ray, or `None` to leave the grapple on the camera. Called around the aim control's
/// post-simulation update.
pub(super) fn set_view(view: Option<Matrix4>) {
    VIEW.with(|v| v.set(view));
}

/// The slot index of the grapple.
const GRAPPLE: usize = TargetType::TARGET_GRAPPLE as usize;
/// The slot index of melee, which the direct-aim ray never fills.
const MELEE: usize = TargetType::TARGET_MELEE as usize;
const SLOTS: usize = TargetType::NOF_TARGET_TYPES as usize;

thread_local! {
    /// The grapple view for the aim update in progress on this thread.
    static VIEW: Cell<Option<Matrix4>> = const { Cell::new(None) };
    /// The matrix the camera getters return, set only for the duration of a scoped call.
    static OVERRIDE: Cell<Option<Matrix4>> = const { Cell::new(None) };
}

/// Run `f` with the camera getters returning `view`, the camera matrix of a ray: the scoring the
/// aim control does around its camera then happens around the ray. Per-hand aim assist scores the
/// candidates this way ([`crate::controllers::aim::assist`]).
pub(super) fn with_view<R>(view: Matrix4, f: impl FnOnce() -> R) -> R {
    let previous = OVERRIDE.with(|o| o.replace(Some(view)));
    let result = f();
    OVERRIDE.with(|o| o.set(previous));
    result
}

/// Run `f` with the camera getters returning the grapple view, when one is set.
fn with_grapple_view<R>(f: impl FnOnce() -> R) -> R {
    match VIEW.with(Cell::get) {
        Some(view) => with_view(view, f),
        None => f(),
    }
}

/// The direct-aim raycast, run from the grapple ray and then from the camera, with the grapple
/// slot kept from the first pass (see the module docs).
#[detour(address = jc3gi::aim::aim::PlayerAimControl::UpdateDirectAim_ADDRESS)]
fn update_direct_aim(aim: *mut PlayerAimControl) {
    let original = move || UPDATE_DIRECT_AIM.get().unwrap().call(aim);
    if VIEW.with(Cell::get).is_none() || aim.is_null() {
        return original();
    }
    with_grapple_view(original);
    let grapple = unsafe { GrappleSlot::take(&mut *aim) };
    unsafe { clear_other_slots(&mut *aim) };
    original();
    unsafe { grapple.put_back(&mut *aim) };
}

/// The adjusted camera matrix, replaced by the grapple view inside the grapple's scoped calls. The
/// view already is the aim ray, so the alternate-aim transform and the weapon's ballistic pitch do
/// not apply to it.
#[detour(address = jc3gi::aim::aim::PlayerAimControl::GetAdjustedCameraMatrix_ADDRESS)]
fn get_adjusted_camera_matrix(result: *mut Matrix4, weapon: *mut c_void) -> *mut Matrix4 {
    if let (Some(view), Some(out)) = (camera_matrix_override(), unsafe { result.as_mut() }) {
        *out = view;
        return result;
    }
    GET_ADJUSTED_CAMERA_MATRIX
        .get()
        .unwrap()
        .call(result, weapon)
}

/// The grapple candidate scoring, around the grapple ray.
#[detour(address = jc3gi::aim::aim::PlayerAimControl::SetTargetGrappleFitness_ADDRESS)]
fn set_target_grapple_fitness(
    aim: *mut PlayerAimControl,
    game_object: *mut c_void,
    target: *mut Target,
) -> bool {
    with_grapple_view(|| {
        SET_TARGET_GRAPPLE_FITNESS
            .get()
            .unwrap()
            .call(aim, game_object, target)
    })
}

/// The target visibility cast. For the grapple under a grapple view, a synchronous cast from the
/// grapple ray's origin, leaving the target's scheduled request to the camera-based types (see the
/// module docs).
#[detour(address = jc3gi::aim::aim::PlayerAimControl::IsTargetVisibleCast_ADDRESS)]
fn is_target_visible_cast(
    aim: *mut PlayerAimControl,
    target: *mut Target,
    target_type: i32,
) -> bool {
    if target_type == GRAPPLE as i32
        && let Some(view) = VIEW.with(Cell::get)
    {
        // SAFETY: the player aim control's update, in the simulation phase, with the engine's own
        // aim control and target.
        return unsafe { visible_from_view(aim, target, view) };
    }
    IS_TARGET_VISIBLE_CAST
        .get()
        .unwrap()
        .call(aim, target, target_type)
}

/// The last-valid grapple cache's re-validation, against the grapple ray.
#[detour(
    address = jc3gi::aim::aim::PlayerAimControl::IsLastValidGrappleAimPosStillValid_ADDRESS
)]
fn is_last_valid_grapple_aim_pos_still_valid(aim: *mut PlayerAimControl) -> bool {
    with_grapple_view(|| {
        IS_LAST_VALID_GRAPPLE_AIM_POS_STILL_VALID
            .get()
            .unwrap()
            .call(aim)
    })
}

/// How far short of a grapple candidate's body position its line of sight may be blocked and still
/// count as clear, metres: the line ends inside the candidate's own body, or its vehicle's.
const GRAPPLE_OCCLUSION_TOLERANCE_M: f32 = 0.75;

/// Whether `target`'s body position is in sight of the ray `view` stands for, cast from where the
/// engine's visibility cast starts from a camera matrix, ignoring the aim control's owner and its
/// vehicle.
///
/// # Safety
///
/// `aim` and `target` must be null or live, called from the aim control's update.
unsafe fn visible_from_view(
    aim: *mut PlayerAimControl,
    target: *mut Target,
    view: Matrix4,
) -> bool {
    let Some(owner) = (unsafe { aim.as_ref() }).and_then(|a| unsafe { a.m_Owner.as_ref() }) else {
        return false;
    };
    if target.is_null() {
        return false;
    }
    let Some(end) = (unsafe { targets::body_position(target) }) else {
        return false;
    };
    let view = Mat4::from(view);
    let origin = view.w_axis.truncate() - view.z_axis.truncate() * DIRECT_AIM_START_OFFSET;
    let vehicle = unsafe { owner.GetVehiclePtr() } as usize;
    let options = CastOptions {
        ignore: [owner as *const _ as usize, vehicle],
        see_through: true,
        material: false,
    };
    unsafe { raycast::line_of_sight(origin, end, GRAPPLE_OCCLUSION_TOLERANCE_M, &options) }
}

/// Everything the direct-aim distribution writes for the grapple slot.
struct GrappleSlot {
    hit: bool,
    normal: Vector3,
    target: *mut Target,
    position: Vector3,
    grapplable: bool,
    aim_position: Vector3,
    in_range: bool,
    direct_in_range: bool,
    /// Moved out of the slot: the slot no longer owns this weak reference, this does.
    object: WeakPtrGameObject,
}

impl GrappleSlot {
    /// Take the grapple slot's direct-aim results out of `aim`, leaving its game object empty.
    unsafe fn take(aim: &mut PlayerAimControl) -> Self {
        let object = unsafe { ptr::read(&aim.m_DirectAimTargetGOs[GRAPPLE]) };
        unsafe { ptr::write(&mut aim.m_DirectAimTargetGOs[GRAPPLE], null_weak()) };
        Self {
            hit: aim.m_DirectHits[GRAPPLE],
            normal: aim.m_LastDirectAimHitNormals[GRAPPLE],
            target: aim.m_DirectTargets[GRAPPLE],
            position: aim.m_DirectPositions[GRAPPLE],
            grapplable: aim.m_DirectGrapplePosIsGrapplable,
            aim_position: aim.m_AimPos[GRAPPLE],
            in_range: aim.m_IsInRange[GRAPPLE],
            direct_in_range: aim.m_DirectPosIsInRange[GRAPPLE],
            object,
        }
    }

    /// Put the taken results back into `aim`, releasing whatever the camera pass left in the slot's
    /// game object.
    unsafe fn put_back(self, aim: &mut PlayerAimControl) {
        unsafe { release(&mut aim.m_DirectAimTargetGOs[GRAPPLE]) };
        unsafe { ptr::write(&mut aim.m_DirectAimTargetGOs[GRAPPLE], self.object) };
        aim.m_DirectHits[GRAPPLE] = self.hit;
        aim.m_LastDirectAimHitNormals[GRAPPLE] = self.normal;
        aim.m_DirectTargets[GRAPPLE] = self.target;
        aim.m_DirectPositions[GRAPPLE] = self.position;
        aim.m_DirectGrapplePosIsGrapplable = self.grapplable;
        aim.m_AimPos[GRAPPLE] = self.aim_position;
        aim.m_IsInRange[GRAPPLE] = self.in_range;
        aim.m_DirectPosIsInRange[GRAPPLE] = self.direct_in_range;
    }
}

/// Return every slot the direct-aim distribution fills, except the grapple's, to the state the
/// post-simulation update leaves it in before the raycast: no hit, no target, and no game object.
/// The distribution writes the other fields of every slot unconditionally.
unsafe fn clear_other_slots(aim: &mut PlayerAimControl) {
    for slot in (0..SLOTS).filter(|&s| s != GRAPPLE && s != MELEE) {
        aim.m_DirectHits[slot] = false;
        aim.m_DirectTargets[slot] = ptr::null_mut();
        unsafe { release(&mut aim.m_DirectAimTargetGOs[slot]) };
    }
}

/// Release a weak pointer through the engine's assignment from an empty one.
unsafe fn release(weak: &mut WeakPtrGameObject) {
    if weak.px.is_null() && weak.pn.is_null() {
        return;
    }
    let empty = null_weak();
    unsafe { weak.Assign(&empty) };
}

fn null_weak() -> WeakPtrGameObject {
    WeakPtrGameObject {
        px: ptr::null_mut(),
        pn: ptr::null_mut(),
    }
}
