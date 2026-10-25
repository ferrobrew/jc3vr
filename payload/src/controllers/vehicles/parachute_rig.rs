//! The parachute's handle props, drawn where the mod's handles are.
//!
//! The parachute's rig has a handle on each side, on its own cord, and the game draws each in the
//! player's hand wherever the hand is ([`NParachuteObjectControl_SyncBones`]): the rig's handle bone
//! takes the character's second hand-attach bone's position, its rotation bone the hand's turn, and
//! the link its cord hangs from is drawn toward the hand. The mod's handles are taken with the
//! grip ([`crate::controllers::vehicles::parachute`]), so left to the game a hand that has not
//! taken its handle is drawn gripping the game's while the mod's mark says the handle is free.
//!
//! A post-call detour on the rig's sync puts each handle the local player's hand does not hold
//! back on its link, as the sync itself does for a hand it does not place (a hand wielding a
//! weapon, say): the handle's, the rotation bone's, and the link's joints are put back as they
//! were before the sync, and the handle's translation is set to the link's. A held handle is left
//! to the game, which draws it in the hand holding it. While the mod's handles do not act (the
//! handles switched off, or no controller tick lately) the rig is left to the game.
//!
//! [`NParachuteObjectControl_SyncBones`]: jc3gi::input::parachute::NParachuteObjectControl_SyncBones

use std::{
    ffi::c_void,
    mem::MaybeUninit,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use detours_macro::detour;
use jc3gi::character::character::{AnimatedModel, AnimationController, Character, Joint};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{frame::Hand, hands::ik};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&SYNC_BONES_BINDER)
}

/// Publish whether the mod's handles act and which hands hold theirs, indexed by [`Hand::index`].
/// Called on each controller tick under the parachute; `None` when the handles do not act.
pub(super) fn publish(held: Option<[bool; 2]>) {
    *HOLDS.lock() = held.map(|held| (held, Instant::now()));
}

/// Handles put back on their links since the payload loaded.
pub fn handles_unplaced() -> u64 {
    UNPLACED.load(Ordering::Relaxed)
}

/// Which handles to put back on their links: each hand's that does not hold its handle while the
/// handles acted lately, and none otherwise.
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
    let unplace = if ours {
        unplaced(*HOLDS.lock(), Instant::now())
    } else {
        [false; 2]
    };
    // SAFETY: the sync's animated model is live for the call.
    let controller = unplace
        .contains(&true)
        .then(|| unsafe { am.as_mut() }.and_then(|am| unsafe { am.m_AnimationController.as_mut() }))
        .flatten();
    let Some(controller) = controller else {
        SYNC_BONES.get().unwrap().call(parachute, am, owner, props);
        return;
    };
    // SAFETY: the rig's controller is live for the sync, on the thread running it.
    let saved = Hand::BOTH.map(|hand| {
        unplace[hand.index()]
            .then(|| unsafe { HandleBones::resolve(controller, hand) })
            .flatten()
            .map(|bones| (bones, unsafe { bones.read(controller) }))
    });
    SYNC_BONES.get().unwrap().call(parachute, am, owner, props);
    for (bones, joints) in saved.into_iter().flatten() {
        // SAFETY: as above.
        unsafe { bones.unplace(controller, joints) };
        UNPLACED.fetch_add(1, Ordering::Relaxed);
    }
}

/// One side's handle bones in the rig.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct HandleBones {
    handle: u32,
    rotate: u32,
    link: u32,
}

/// The three joints of one side, as they were before the sync.
#[derive(Copy, Clone)]
struct HandleJoints {
    handle: Joint,
    rotate: Joint,
    link: Joint,
}

impl HandleBones {
    /// `hand`'s handle bones in the rig, or `None` when one does not resolve.
    ///
    /// # Safety
    ///
    /// `controller` must be the rig's live animation controller, on the thread that owns it.
    unsafe fn resolve(controller: &AnimationController, hand: Hand) -> Option<Self> {
        let [handle, rotate, link] = BONE_NAMES[hand.index()];
        let index = |name| unsafe { ik::bone_index(controller, name) };
        Some(HandleBones {
            handle: index(handle)?,
            rotate: index(rotate)?,
            link: index(link)?,
        })
    }

    /// The side's joints.
    ///
    /// # Safety
    ///
    /// As [`resolve`](Self::resolve).
    unsafe fn read(self, controller: &AnimationController) -> HandleJoints {
        let joint = |index| {
            let mut joint = MaybeUninit::<Joint>::zeroed();
            unsafe {
                controller.GetJoint(index, joint.as_mut_ptr());
                joint.assume_init()
            }
        };
        HandleJoints {
            handle: joint(self.handle),
            rotate: joint(self.rotate),
            link: joint(self.link),
        }
    }

    /// Put the side's joints back as `before` had them, with the handle on its link, as the sync
    /// leaves a handle it does not place.
    ///
    /// # Safety
    ///
    /// As [`resolve`](Self::resolve).
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

/// Each side's handle, rotation, and link bones, indexed by [`Hand::index`].
const BONE_NAMES: [[&str; 3]; 2] = [
    ["LeftHandle", "LeftHandleRotate", "LeftLink"],
    ["RightHandle", "RightHandleRotate", "RightLink"],
];

/// How long a publication stays current: a few controller ticks.
const STALE_AFTER: Duration = Duration::from_millis(250);

static HOLDS: Mutex<Option<([bool; 2], Instant)>> = Mutex::new(None);
static UNPLACED: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_handles_the_hands_do_not_hold_are_put_back_while_the_handles_act() {
        let now = Instant::now();
        assert_eq!(unplaced(None, now), [false; 2]);
        assert_eq!(unplaced(Some(([false, true], now)), now), [true, false]);
        assert_eq!(unplaced(Some(([true, true], now)), now), [false, false]);
        // A publication gone stale leaves the rig to the game.
        let stale = now + STALE_AFTER * 2;
        assert_eq!(unplaced(Some(([false, false], now)), stale), [false; 2]);
    }
}

#[cfg(test)]
#[path = "parachute_native_model.rs"]
mod native_rest_model;
