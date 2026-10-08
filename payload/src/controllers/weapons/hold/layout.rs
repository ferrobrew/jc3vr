//! The attach bone each gun of a held weapon rides, for the hand holding it. Pure.
//!
//! The game puts a held weapon's guns on the right hand's attach bones, and its state tasks move a
//! gun between that hand's two attach bones. A weapon held in the left hand rides the left hand's
//! counterpart of whatever bone the game chose, so the game's moves within a hand are kept and only
//! the side changes. A gun on any other bone (a holster or back bone, or one the game moves off the
//! hands) is left where the game put it. A weapon of more than one gun (the dual-wield pair, when
//! its own layout does not own it) keeps a gun in each hand, so held in the left hand its layout is
//! mirrored rather than gathered into one hand ([`placed_bone`]). A holstered gun docks on one of
//! the hip holster or back attach bones ([`dock_bone`]).

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

/// The bone gun `index` of a weapon of `guns` guns, on `bone`, rides while `holder` holds the
/// weapon, when it differs from `bone`. A single gun goes to the holder's hand ([`held_bone`]). A
/// weapon of several guns, which the game puts one in each hand, keeps a gun in each hand: the
/// first in the holder's and the second in the other, so held in the left hand its layout is
/// mirrored. Like [`held_bone`], applying it to its own result changes nothing, so it can be
/// written at every point the table is.
pub fn placed_bone(bone: u32, holder: Hand, index: usize, guns: usize) -> Option<u32> {
    let hand = if guns > 1 && index % 2 == 1 {
        holder.other()
    } else {
        holder
    };
    held_bone(bone, hand)
}

/// The primary attach bone of `hand`.
pub fn hand_bone(hand: Hand) -> u32 {
    match hand {
        Hand::Left => SafeBoneIndex::ATTACH_HAND_LEFT as u32,
        Hand::Right => SafeBoneIndex::ATTACH_HAND_RIGHT as u32,
    }
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
    fn a_pair_held_left_is_mirrored_not_gathered() {
        // The game's pair: the first gun in the right hand, the second in the left.
        assert_eq!(placed_bone(RIGHT, Hand::Left, 0, 2), Some(LEFT));
        assert_eq!(placed_bone(LEFT, Hand::Left, 1, 2), Some(RIGHT));
        assert_eq!(placed_bone(RIGHT2, Hand::Left, 0, 2), Some(LEFT2));
        // Applied again to its own result, it leaves the layout as it is.
        assert_eq!(placed_bone(LEFT, Hand::Left, 0, 2), None);
        assert_eq!(placed_bone(RIGHT, Hand::Left, 1, 2), None);
        // Held right, the game's layout stands.
        assert_eq!(placed_bone(RIGHT, Hand::Right, 0, 2), None);
        assert_eq!(placed_bone(LEFT, Hand::Right, 1, 2), None);
        // A single gun goes to the holding hand whichever hand the game chose.
        assert_eq!(placed_bone(RIGHT, Hand::Left, 0, 1), Some(LEFT));
        assert_eq!(placed_bone(LEFT, Hand::Left, 0, 1), None);
        assert_eq!(placed_bone(LEFT, Hand::Right, 0, 1), Some(RIGHT));
        assert_eq!(
            placed_bone(SafeBoneIndex::LEFT_HOLSTER as u32, Hand::Left, 1, 2),
            None,
            "a pistol on the hip stays there"
        );
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
