//! The attach bone each gun of a held weapon rides, for the hand holding it. Pure.
//!
//! The game puts a held weapon's guns on the right hand's attach bones, and its state tasks move a
//! gun between that hand's two attach bones. A weapon held in the left hand rides the left hand's
//! counterpart of whatever bone the game chose, so the game's moves within a hand are kept and only
//! the side changes. A gun on any other bone (a holster or back bone, or one the game moves off the
//! hands) is left where the game put it. A holstered gun docks on one of the hip holster or back
//! attach bones ([`dock_bone`]).

use jc3gi::character::character::SafeBoneIndex;

use crate::controllers::frame::Hand;

/// The bone a gun on `bone` rides while `holder` holds the weapon, when it differs from `bone`:
/// the counterpart on the holder's hand of the other hand's attach bone. `None` for a bone that
/// already is the holder's, or is not a hand attach bone.
pub fn held_bone(bone: u32, holder: Hand) -> Option<u32> {
    let (hand, mirror) = HAND_BONES
        .into_iter()
        .find(|&(_, safe, _)| safe as u32 == bone)
        .map(|(hand, _, mirror)| (hand, mirror))?;
    (hand != holder).then_some(mirror as u32)
}

/// The dock bone a holstered gun on `bone` sits on: a hip holster or a back attach bone.
pub fn dock_bone(bone: u32) -> Option<SafeBoneIndex> {
    DOCK_BONES.into_iter().find(|&safe| safe as u32 == bone)
}

/// The bones a holstered gun docks on.
const DOCK_BONES: [SafeBoneIndex; 4] = [
    SafeBoneIndex::RIGHT_HOLSTER,
    SafeBoneIndex::LEFT_HOLSTER,
    SafeBoneIndex::BACK_ATTACH,
    SafeBoneIndex::BACK_ATTACH_2,
];

/// Each hand attach bone, its hand, and its counterpart on the other hand.
const HAND_BONES: [(Hand, SafeBoneIndex, SafeBoneIndex); 4] = [
    (
        Hand::Right,
        SafeBoneIndex::ATTACH_HAND_RIGHT,
        SafeBoneIndex::ATTACH_HAND_LEFT,
    ),
    (
        Hand::Right,
        SafeBoneIndex::ATTACH_HAND_RIGHT2,
        SafeBoneIndex::ATTACH_HAND_LEFT2,
    ),
    (
        Hand::Left,
        SafeBoneIndex::ATTACH_HAND_LEFT,
        SafeBoneIndex::ATTACH_HAND_RIGHT,
    ),
    (
        Hand::Left,
        SafeBoneIndex::ATTACH_HAND_LEFT2,
        SafeBoneIndex::ATTACH_HAND_RIGHT2,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    const RIGHT: u32 = SafeBoneIndex::ATTACH_HAND_RIGHT as u32;
    const RIGHT2: u32 = SafeBoneIndex::ATTACH_HAND_RIGHT2 as u32;
    const LEFT: u32 = SafeBoneIndex::ATTACH_HAND_LEFT as u32;
    const LEFT2: u32 = SafeBoneIndex::ATTACH_HAND_LEFT2 as u32;

    #[test]
    fn a_left_held_gun_rides_the_left_counterpart() {
        assert_eq!(held_bone(RIGHT, Hand::Left), Some(LEFT));
        assert_eq!(held_bone(RIGHT2, Hand::Left), Some(LEFT2));
        assert_eq!(held_bone(LEFT, Hand::Right), Some(RIGHT));
        assert_eq!(held_bone(LEFT2, Hand::Right), Some(RIGHT2));
    }

    #[test]
    fn a_gun_on_the_holder_s_hand_stays() {
        assert_eq!(held_bone(RIGHT, Hand::Right), None);
        assert_eq!(held_bone(RIGHT2, Hand::Right), None);
        assert_eq!(held_bone(LEFT2, Hand::Left), None);
    }

    #[test]
    fn a_gun_off_the_hands_is_left_alone() {
        for bone in [
            SafeBoneIndex::RIGHT_HOLSTER,
            SafeBoneIndex::LEFT_HOLSTER,
            SafeBoneIndex::BACK_ATTACH,
            SafeBoneIndex::BACK_ATTACH_2,
        ] {
            for hand in Hand::BOTH {
                assert_eq!(held_bone(bone as u32, hand), None, "{bone:?}");
            }
            assert_eq!(dock_bone(bone as u32), Some(bone));
        }
        assert_eq!(dock_bone(RIGHT), None, "a hand is not a dock");
    }
}
