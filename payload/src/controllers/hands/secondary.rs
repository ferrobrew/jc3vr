//! The per-hand switch for the game's `SECONDARY` hand IK pass.
//!
//! [`Character::UpdateSecondaryHandIKPass`](jc3gi::character::character::Character::UpdateSecondaryHandIKPass)
//! runs after the `MAIN` solve and can move the wrists again. It is driven entirely by one-frame
//! blackboard keys that the animation tasks write during graph evaluation, before the
//! pose-finalization pass reads them, so zeroing a key at the finalize entry switches that part off
//! for the frame:
//!
//! - each wrist's weight key, which the grapple fire-arm IK and the parachute handle IK write;
//! - the weapon off-hand grip's weight key, which targets only the hand its hand key names.
//!
//! Only keys that are present and positive are overwritten, so the switch never creates a key the
//! game did not write this frame. The foregrip (a later task) reuses [`SecondaryParts`] to switch
//! the wrist keys off while leaving the weapon grip on.

use jc3gi::blackboard::{ObjectBlackboard, PersistenceType};

use crate::controllers::frame::Hand;

/// Which parts of the `SECONDARY` pass to switch off for a hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SecondaryParts {
    /// The wrist's own target (the grapple fire arm and the parachute handles).
    pub wrist_target: bool,
    /// The weapon off-hand grip, when it targets this hand.
    pub weapon_grip: bool,
}

impl SecondaryParts {
    /// Every part: the hand belongs to its controller alone.
    pub const ALL: SecondaryParts = SecondaryParts {
        wrist_target: true,
        weapon_grip: true,
    };
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
/// `blackboard` must be the character's live blackboard, called from the pose-finalization pass's
/// entry, after the graph evaluation wrote this frame's keys.
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
