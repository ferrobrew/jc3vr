//! The per-hand switch for the game's `SECONDARY` hand IK pass.
//!
//! [`Character::UpdateSecondaryHandIKPass`](jc3gi::character::character::Character::UpdateSecondaryHandIKPass)
//! runs after the `MAIN` solve and can move the wrists again. It is driven entirely by blackboard
//! keys that animation state tasks write, some of them (the weapon off-hand IK and the grapple
//! fire-arm IK among them) inside the pose-finalization pass itself, after its entry. Most are
//! written for one frame; the parachute handle IK's persist from tick to tick while its task
//! runs. So the keys are zeroed at the `SECONDARY` pass's own entry, which switches that part off
//! for the frame:
//!
//! - each wrist's weight key, which the grapple fire-arm IK and the parachute handle IK write;
//! - the weapon off-hand grip's weight key, which targets only the hand its hand key names.
//!
//! Only keys that are present and positive are overwritten, so the switch never creates a key the
//! game did not write this frame. The weapon-agnostic body switches the weapon grip off for a hand
//! the controllers do not drive, leaving its wrist key alone.
//!
//! Zeroing the keys stops new `SECONDARY` targets, but not one queued on a frame before the hand
//! was driven (a tier blending in from the wingsuit, or a controller that was untracked): its
//! entry stays in the pass's target list for as long as the wrist's current reach is above zero,
//! and the `MAIN` targets keep that reach up every frame. Such an entry holds a stale model-space
//! point, the parachute handle's, and pulls the wrist back to it whenever the `SECONDARY` pass
//! solves for another target. [`release_wrist`] zeroes the driven wrist's current reach at the same
//! entry, after the `MAIN` solve has used it, so the pass's own clearing drops any such entry that
//! frame and a `SECONDARY` solve leaves the wrist where the controller put it. The next `MAIN` pass
//! drives the reach back up from its target.

use jc3gi::{
    animation::ik::{Effector, HumanIK},
    blackboard::{ObjectBlackboard, PersistenceType},
};

use crate::controllers::frame::Hand;

/// Which parts of the `SECONDARY` pass to switch off for a hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SecondaryParts {
    /// The wrist's own target (the grapple fire arm and the parachute handles).
    pub wrist_target: bool,
    /// The weapon off-hand grip, when it targets this hand.
    pub weapon_grip: bool,
}

/// What the switch did for a hand this frame.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Switched {
    pub wrist_target: bool,
    pub weapon_grip: bool,
}

/// Switch `parts` of the `SECONDARY` pass off for `hand` this frame.
///
/// # Safety
///
/// `blackboard` must be the character's live blackboard, called from the `SECONDARY` pass's entry,
/// after this frame's keys were written.
pub unsafe fn switch_off(
    blackboard: &mut ObjectBlackboard,
    hand: Hand,
    parts: SecondaryParts,
) -> Switched {
    let mut switched = Switched::default();
    if parts.wrist_target {
        let key = match hand {
            Hand::Left => ObjectBlackboard::SECONDARY_HAND_WEIGHT_LEFT_ID,
            Hand::Right => ObjectBlackboard::SECONDARY_HAND_WEIGHT_RIGHT_ID,
        };
        switched.wrist_target = unsafe { zero_if_positive(blackboard, key) };
    }
    if parts.weapon_grip && unsafe { weapon_grip_hand(blackboard) } == Some(hand) {
        switched.weapon_grip =
            unsafe { zero_if_positive(blackboard, ObjectBlackboard::OFFHAND_IK_WEIGHT_ID) };
    }
    switched
}

/// Zero the current translation reach of `hand`'s wrist effector, so that the `SECONDARY` pass
/// drops any stale target it holds for the wrist and its solve leaves the wrist alone (see the
/// module docs).
pub fn release_wrist(hik: &mut HumanIK, hand: Hand) {
    let effector = match hand {
        Hand::Left => Effector::LEFT_WRIST,
        Hand::Right => Effector::RIGHT_WRIST,
    };
    if let Some(reach) = hik.m_ReachT.get_mut(effector as usize) {
        *reach = 0.0;
    }
}

/// The hand the weapon off-hand grip targets this frame, or `None` when its key is absent.
unsafe fn weapon_grip_hand(blackboard: &mut ObjectBlackboard) -> Option<Hand> {
    let mut right = 0i32;
    unsafe { blackboard.GetInt(ObjectBlackboard::OFFHAND_IK_HAND_ID, &mut right) }
        .then_some(if right != 0 { Hand::Right } else { Hand::Left })
}

/// Overwrite a float key with zero when it is present and positive; returns whether it did.
unsafe fn zero_if_positive(blackboard: &mut ObjectBlackboard, key: u32) -> bool {
    let mut value = 0.0f32;
    if !unsafe { blackboard.GetFloat(key, &mut value) } || value <= 0.0 {
        return false;
    }
    unsafe { blackboard.SetFloat(key, 0.0, PersistenceType::PERSISTENCE_LOCAL_FRAME) }
}
