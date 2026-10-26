//! Native parachute handle positions and the unheld handle props.
//!
//! The sync reads each link before placing its handle in the character's hand. The scoped
//! `GetJoint` detour copies those existing reads, without evaluating more joints. The copied
//! link positions compose through the parachute object's transform for the input targets.
//! Unheld props return to those joints before the callback ends; held props remain native.

use std::{
    cell::RefCell,
    ffi::c_void,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::{AnimatedModel, AnimationController, Character, Joint},
    game::GameObject,
    types::math::Matrix4,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{frame::Hand, hands::ik, vehicles::wheel::CockpitTransform};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SYNC_BONES_BINDER)
        .with_static_binder(&GET_JOINT_BINDER)
        .with_static_binder(&GET_BONE_MATRIX_BINDER)
}

/// Publish whether the handles act and which hands hold theirs, indexed by [`Hand::index`].
pub(super) fn publish(held: Option<[bool; 2]>) {
    *HOLDS.lock() = held.map(|held| (held, Instant::now()));
}

/// Request native positions and copy the latest positions into the current cockpit frame.
/// A missing or stale native position supplies no free-handle target.
pub(super) fn centres(active: bool, cockpit: &CockpitTransform) -> [Option<Vec3>; 2] {
    let owner = active.then(ik::local_character).filter(|owner| *owner != 0);
    let now = Instant::now();
    let mut native = NATIVE.lock();
    if native.owner != owner {
        native.generation = native.generation.wrapping_add(1);
        native.owner = owner;
        native.sample = None;
    }
    native.requested_at = owner.map(|_| now);
    native
        .sample
        .filter(|sample| {
            Some(sample.owner) == owner
                && sample.rig.0 != 0
                && sample.rig.1 != 0
                && sample.rig.2 != 0
                && sample.generation == native.generation
                && now
                    .checked_duration_since(sample.sampled_at)
                    .is_some_and(|age| age <= STALE_AFTER)
        })
        .map_or([None; 2], |sample| {
            sample.world.map(|point| {
                let point = cockpit.point_to_cockpit(point?);
                point.is_finite().then_some(point)
            })
        })
}

/// Handles put back on their links since the payload loaded.
pub fn handles_unplaced() -> u64 {
    UNPLACED.load(Ordering::Relaxed)
}

/// Which handles to put back while the handle controls acted recently.
pub fn unplaced(holds: Option<([bool; 2], Instant)>, now: Instant) -> [bool; 2] {
    match holds {
        Some((held, at)) if now.saturating_duration_since(at) <= STALE_AFTER => held.map(|h| !h),
        _ => [false; 2],
    }
}

#[detour(address = jc3gi::input::parachute::NParachuteObjectControl_SyncBones_ADDRESS)]
fn sync_bones(
    parachute: *mut c_void,
    am: *mut AnimatedModel,
    owner: *mut Character,
    props: *mut c_void,
) {
    let local = ik::local_character();
    let ours = local != 0 && owner as usize == local;
    let now = Instant::now();
    let unplace = if ours {
        unplaced(*HOLDS.lock(), now)
    } else {
        [false; 2]
    };
    let request = ours
        .then(|| {
            let native = NATIVE.lock();
            (native.owner == Some(local)
                && native
                    .requested_at
                    .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER))
            .then_some(native.generation)
        })
        .flatten();
    if request.is_none() && !unplace.contains(&true) {
        SYNC_BONES.get().unwrap().call(parachute, am, owner, props);
        return;
    }
    // SAFETY: the sync owns these objects for this invocation; only copied values cross the call.
    let setup = unsafe {
        am.as_ref().and_then(|model| {
            let controller = model.m_AnimationController.as_ref()?;
            let owner_controller = owner
                .as_ref()?
                .m_AnimatedModel
                .m_AnimationController
                .as_ref()?;
            let bones = Hand::BOTH.map(|hand| {
                let bones = HandleBones::resolve(controller, hand)?;
                ik::bone_index(owner_controller, HAND_ATTACH_NAMES[hand.index()])?;
                Some(bones)
            });
            let mut world = Matrix4::IDENTITY;
            let world = (parachute
                .cast::<GameObject>()
                .as_ref()?
                .GetTransform(&mut world))
            .then_some(Mat4::from(world))
            .filter(|world| world.is_finite() && world.determinant().abs() > 1e-8);
            Some((model.m_AnimationController, bones, world))
        })
    };
    let Some((controller, bones, world)) = setup else {
        SYNC_BONES.get().unwrap().call(parachute, am, owner, props);
        return;
    };
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let capture = Capture {
        controller: controller as usize,
        bones,
        joints: [CapturedJoints::default(); 2],
        last_joint: None,
        invalid: false,
    };
    let scope = CaptureScope::enter(capture);
    SYNC_BONES.get().unwrap().call(parachute, am, owner, props);
    let captured = scope.finish();
    if captured.invalid {
        return;
    }
    let rests = captured.joints.map(CapturedJoints::complete);
    if let (Some(generation), Some(world)) = (request, world) {
        let points = rests.map(|rest| {
            let point = Vec3::from_array(rest?.link.m_Translation.data);
            let point = world.transform_point3(point);
            point.is_finite().then_some(point)
        });
        let mut native = NATIVE.lock();
        if native.owner == Some(local)
            && native.generation == generation
            && native
                .sample
                .is_none_or(|sample| sample.sequence < sequence)
        {
            native.sample = Some(NativeSample {
                owner: local,
                generation,
                sequence,
                sampled_at: now,
                rig: (parachute as usize, am as usize, controller as usize),
                world: points,
            });
        }
    }
    for hand in Hand::BOTH {
        let i = hand.index();
        if unplace[i]
            && let (Some(bones), Some(joints)) = (bones[i], rests[i])
        {
            // SAFETY: the controller belongs to this sync and no borrow crosses the native call.
            unsafe { bones.unplace(&mut *controller, joints) };
            UNPLACED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[detour(address = jc3gi::character::character::AnimationController::GetJoint_ADDRESS)]
fn get_joint(controller: *const AnimationController, index: u32, joint: *mut Joint) {
    GET_JOINT.get().unwrap().call(controller, index, joint);
    CAPTURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(capture) = slot.as_mut() else {
            return;
        };
        if capture.invalid || capture.controller != controller as usize {
            return;
        }
        capture.last_joint = Some(index);
        for hand in Hand::BOTH {
            let i = hand.index();
            if let Some(bones) = capture.bones[i] {
                // SAFETY: the original reader has initialized its caller's complete output joint.
                if let Some(joint) = unsafe { joint.as_ref() } {
                    capture.joints[i].record(bones, index, *joint);
                }
            }
        }
    });
}

#[detour(address = jc3gi::character::character::AnimationController::GetBoneMatrix_ADDRESS)]
fn get_bone_matrix(controller: *const AnimationController, index: u32, matrix: *mut Matrix4) {
    CAPTURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(capture) = slot.as_mut()
            && capture.controller == controller as usize
            && capture.last_joint.take() == Some(index)
        {
            for hand in Hand::BOTH {
                let i = hand.index();
                if let Some(bones) = capture.bones[i] {
                    capture.joints[i].baseline(bones, index);
                }
            }
        }
    });
    GET_BONE_MATRIX
        .get()
        .unwrap()
        .call(controller, index, matrix);
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct HandleBones {
    handle: u32,
    rotate: u32,
    link: u32,
}

#[derive(Copy, Clone)]
struct HandleJoints {
    handle: Joint,
    rotate: Joint,
    link: Joint,
}

impl HandleBones {
    /// # Safety
    /// The controller is live on the thread running the sync.
    unsafe fn resolve(controller: &AnimationController, hand: Hand) -> Option<Self> {
        let [handle, rotate, link] = BONE_NAMES[hand.index()];
        let index = |name| unsafe { ik::bone_index(controller, name) };
        Some(Self {
            handle: index(handle)?,
            rotate: index(rotate)?,
            link: index(link)?,
        })
    }

    /// # Safety
    /// The controller is live, and these copied joints belong to this invocation.
    unsafe fn unplace(self, controller: &mut AnimationController, before: HandleJoints) {
        let HandleJoints {
            mut handle,
            mut rotate,
            mut link,
        } = before;
        handle.m_Translation = link.m_Translation;
        unsafe {
            controller.SetJoint(self.link, &mut link);
            controller.SetJoint(self.rotate, &mut rotate);
            controller.SetJoint(self.handle, &mut handle);
        }
    }
}

#[derive(Copy, Clone, Default)]
struct CapturedJoints {
    handle: Option<Joint>,
    rotate: Option<Joint>,
    link: Option<Joint>,
    handle_reads: u8,
    link_reads: u8,
    handle_baseline: u8,
    link_baseline: u8,
}

impl CapturedJoints {
    fn record(&mut self, bones: HandleBones, index: u32, joint: Joint) {
        if index == bones.link {
            self.link = Some(joint);
            self.link_reads = self.link_reads.saturating_add(1);
        } else if index == bones.handle {
            self.handle = Some(joint);
            self.handle_reads = self.handle_reads.saturating_add(1);
        } else if index == bones.rotate {
            self.rotate = Some(joint);
        }
    }

    fn baseline(&mut self, bones: HandleBones, index: u32) {
        if index == bones.handle {
            self.handle_baseline = self.handle_reads;
        } else if index == bones.link {
            self.link_baseline = self.link_reads;
        }
    }

    fn complete(self) -> Option<HandleJoints> {
        // A matching matrix read marks the re-derivation pass before handle placement.
        if self.handle_baseline == 0
            || self.link_baseline == 0
            || self.handle_reads <= self.handle_baseline
            || self.link_reads <= self.link_baseline
        {
            return None;
        }
        Some(HandleJoints {
            handle: self.handle?,
            rotate: self.rotate?,
            link: self.link?,
        })
    }
}

struct Capture {
    controller: usize,
    bones: [Option<HandleBones>; 2],
    joints: [CapturedJoints; 2],
    last_joint: Option<u32>,
    invalid: bool,
}

struct CaptureScope {
    previous: Option<Capture>,
    active: bool,
}

impl CaptureScope {
    fn enter(capture: Capture) -> Self {
        let previous = CAPTURE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if let Some(previous) = slot.as_mut()
                && previous.controller == capture.controller
            {
                previous.invalid = true;
            }
            slot.replace(capture)
        });
        Self {
            previous,
            active: true,
        }
    }

    fn finish(mut self) -> Capture {
        self.active = false;
        CAPTURE.with(|slot| {
            slot.replace(self.previous.take())
                .expect("parachute rig: missing native capture scope")
        })
    }
}

impl Drop for CaptureScope {
    fn drop(&mut self) {
        if self.active {
            CAPTURE.with(|slot| {
                slot.replace(self.previous.take());
            });
        }
    }
}

#[derive(Copy, Clone)]
struct NativeSample {
    owner: usize,
    generation: u64,
    sequence: u64,
    sampled_at: Instant,
    rig: (usize, usize, usize),
    world: [Option<Vec3>; 2],
}

struct NativeState {
    owner: Option<usize>,
    generation: u64,
    requested_at: Option<Instant>,
    sample: Option<NativeSample>,
}

const BONE_NAMES: [[&str; 3]; 2] = [
    ["LeftHandle", "LeftHandleRotate", "LeftLink"],
    ["RightHandle", "RightHandleRotate", "RightLink"],
];
const HAND_ATTACH_NAMES: [&str; 2] = ["LeftHandAttach2", "RightHandAttach2"];
const STALE_AFTER: Duration = Duration::from_millis(250);

static HOLDS: Mutex<Option<([bool; 2], Instant)>> = Mutex::new(None);
static UNPLACED: AtomicU64 = AtomicU64::new(0);
static SEQUENCE: AtomicU64 = AtomicU64::new(1);
static NATIVE: Mutex<NativeState> = Mutex::new(NativeState {
    owner: None,
    generation: 0,
    requested_at: None,
    sample: None,
});

thread_local! {
    static CAPTURE: RefCell<Option<Capture>> = const { RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_native_placement_reads_complete_a_side() {
        let bones = HandleBones {
            handle: 3,
            rotate: 4,
            link: 5,
        };
        let joint = Joint::default();
        let mut captured = CapturedJoints::default();
        for index in [bones.handle, bones.rotate, bones.link] {
            captured.record(bones, index, joint);
            captured.baseline(bones, index);
        }
        assert!(captured.complete().is_none());
        captured.record(bones, bones.link, joint);
        captured.record(bones, bones.handle, joint);
        assert!(captured.complete().is_some());
    }

    #[test]
    fn placement_copies_the_link_after_the_scale_and_rederivation_reads() {
        let bones = HandleBones {
            handle: 3,
            rotate: 4,
            link: 5,
        };
        let joint = Joint::default();
        let mut captured = CapturedJoints::default();
        for index in [bones.handle, bones.rotate, bones.link] {
            captured.record(bones, index, joint);
        }
        for index in [bones.handle, bones.rotate, bones.link] {
            captured.record(bones, index, joint);
            captured.baseline(bones, index);
        }
        assert!(captured.complete().is_none());
        let mut link = joint;
        link.m_Translation.data = [1.0, 2.0, 3.0];
        captured.record(bones, bones.link, link);
        captured.record(bones, bones.handle, joint);
        assert_eq!(
            Vec3::from_array(captured.complete().unwrap().link.m_Translation.data),
            Vec3::new(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn only_the_handles_the_hands_do_not_hold_are_put_back_while_the_handles_act() {
        let now = Instant::now();
        assert_eq!(unplaced(None, now), [false; 2]);
        assert_eq!(unplaced(Some(([false, true], now)), now), [true, false]);
        assert_eq!(
            unplaced(Some(([false; 2], now)), now + STALE_AFTER * 2),
            [false; 2]
        );
    }
}

#[cfg(test)]
#[path = "parachute_native_model.rs"]
mod native_rest_model;
