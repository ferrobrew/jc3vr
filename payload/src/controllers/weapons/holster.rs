//! What a grab at a holster does: draw that holster's weapon, or holster the held one. Pure.
//!
//! Each holster carries one inventory slot: the hips the dual-wield slot, the dominant shoulder the
//! two-handed slot, the other shoulder the special slot, and the chest the planted explosive.
//!
//! - A grab at the holster of the slot that is wielded, by a hand holding that weapon, holsters it.
//!   The holster goes through `Character::HolsterWeapon`, not the reload button's hold, because that
//!   hold also drops a heavy weapon and blocks melee until the melee input is released. The heavy
//!   slot has no holster, so a heavy weapon is never holstered here; the reload button's hold still
//!   drops it, as in the game.
//! - A grab at any other holster draws its slot through the slot-select action, which the game acts
//!   on when the input is released: it switches weapons when one is wielded, and draws the slot
//!   when none is (selecting the wielded slot's own holster while it is holstered draws it too).
//! - A grab at the wielded slot's holster by the hand that does not hold the weapon does nothing.

use jc3gi::{character::inventory::WeaponSlot, input::input_action_map::Action};

use crate::controllers::{bindings::grab::HolsterSite, frame::Hand};

/// The weapon in the hands, as the holsters see it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Wielded {
    pub slot: WeaponSlot,
    /// Which hands hold one of its guns, indexed by [`Hand::index`].
    pub hands: [bool; 2],
}

/// What a holster grab does.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HolsterAction {
    /// Click the slot-select action.
    Draw { slot: WeaponSlot, action: Action },
    /// Holster the wielded weapon.
    Holster { slot: WeaponSlot },
    /// The slot is already wielded, in the other hand.
    AlreadyDrawn { slot: WeaponSlot },
}

impl HolsterAction {
    pub fn label(self) -> String {
        match self {
            HolsterAction::Draw { slot, .. } => format!("draw {}", slot_label(slot)),
            HolsterAction::Holster { slot } => format!("holster {}", slot_label(slot)),
            HolsterAction::AlreadyDrawn { slot } => {
                format!("{} already drawn", slot_label(slot))
            }
        }
    }
}

/// The inventory slot a holster carries.
pub const fn site_slot(site: HolsterSite, dominant: Hand) -> WeaponSlot {
    match site {
        HolsterSite::Hip(_) => WeaponSlot::E_SLOT_DUAL_WIELD,
        HolsterSite::Shoulder(side) if side.index() == dominant.index() => {
            WeaponSlot::E_SLOT_TWO_HANDED
        }
        HolsterSite::Shoulder(_) => WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL,
        HolsterSite::Chest => WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE,
    }
}

/// The action that selects `slot`, for the slots a holster carries.
pub const fn select_action(slot: WeaponSlot) -> Option<Action> {
    match slot {
        WeaponSlot::E_SLOT_DUAL_WIELD => Some(Action::SELECT_DUEL_WIELD),
        WeaponSlot::E_SLOT_TWO_HANDED => Some(Action::SELECT_TWO_HANDED),
        WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL => Some(Action::SELECT_TWO_HANDED_SPECIAL),
        WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE => Some(Action::SELECT_EXPLOSIVES),
        _ => None,
    }
}

/// What `hand`'s grab at `site` does, with `wielded` in the hands.
pub fn decide(
    site: HolsterSite,
    hand: Hand,
    dominant: Hand,
    wielded: Option<Wielded>,
) -> Option<HolsterAction> {
    let slot = site_slot(site, dominant);
    match wielded {
        Some(w) if w.slot == slot && w.hands[hand.index()] => Some(HolsterAction::Holster { slot }),
        Some(w) if w.slot == slot => Some(HolsterAction::AlreadyDrawn { slot }),
        _ => select_action(slot).map(|action| HolsterAction::Draw { slot, action }),
    }
}

pub const fn slot_label(slot: WeaponSlot) -> &'static str {
    match slot {
        WeaponSlot::E_SLOT_DUAL_WIELD => "pistols",
        WeaponSlot::E_SLOT_TWO_HANDED => "two-handed",
        WeaponSlot::E_SLOT_HEAVY => "heavy",
        WeaponSlot::E_SLOT_HAND_GRENADE => "grenade",
        WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE => "explosives",
        WeaponSlot::E_SLOT_FOW => "FOW",
        WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL => "special",
        WeaponSlot::E_SLOT_BEACON => "beacon",
        _ => "none",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wielded(slot: WeaponSlot, hands: [bool; 2]) -> Option<Wielded> {
        Some(Wielded { slot, hands })
    }

    const RIGHT_ONLY: [bool; 2] = [false, true];

    #[test]
    fn the_shoulders_follow_the_dominant_hand() {
        let right = HolsterSite::Shoulder(Hand::Right);
        assert_eq!(site_slot(right, Hand::Right), WeaponSlot::E_SLOT_TWO_HANDED);
        assert_eq!(
            site_slot(right, Hand::Left),
            WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
        );
        assert_eq!(
            site_slot(HolsterSite::Hip(Hand::Left), Hand::Right),
            WeaponSlot::E_SLOT_DUAL_WIELD
        );
    }

    #[test]
    fn an_empty_hand_draws() {
        assert_eq!(
            decide(HolsterSite::Chest, Hand::Right, Hand::Right, None),
            Some(HolsterAction::Draw {
                slot: WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE,
                action: Action::SELECT_EXPLOSIVES
            })
        );
    }

    #[test]
    fn the_holding_hand_holsters_at_its_own_holster() {
        let rifle = wielded(WeaponSlot::E_SLOT_TWO_HANDED, RIGHT_ONLY);
        let site = HolsterSite::Shoulder(Hand::Right);
        assert_eq!(
            decide(site, Hand::Right, Hand::Right, rifle),
            Some(HolsterAction::Holster {
                slot: WeaponSlot::E_SLOT_TWO_HANDED
            })
        );
        assert_eq!(
            decide(site, Hand::Left, Hand::Right, rifle),
            Some(HolsterAction::AlreadyDrawn {
                slot: WeaponSlot::E_SLOT_TWO_HANDED
            })
        );
    }

    #[test]
    fn another_holster_switches_weapons() {
        let rifle = wielded(WeaponSlot::E_SLOT_TWO_HANDED, RIGHT_ONLY);
        assert!(matches!(
            decide(HolsterSite::Hip(Hand::Left), Hand::Left, Hand::Right, rifle),
            Some(HolsterAction::Draw {
                action: Action::SELECT_DUEL_WIELD,
                ..
            })
        ));
    }

    #[test]
    fn either_hand_holsters_dual_pistols() {
        let pistols = wielded(WeaponSlot::E_SLOT_DUAL_WIELD, [true, true]);
        for hand in Hand::BOTH {
            assert_eq!(
                decide(HolsterSite::Hip(hand), hand, Hand::Right, pistols),
                Some(HolsterAction::Holster {
                    slot: WeaponSlot::E_SLOT_DUAL_WIELD
                })
            );
        }
    }

    #[test]
    fn a_heavy_weapon_is_never_holstered() {
        let heavy = wielded(WeaponSlot::E_SLOT_HEAVY, RIGHT_ONLY);
        for site in [
            HolsterSite::Hip(Hand::Right),
            HolsterSite::Shoulder(Hand::Right),
            HolsterSite::Shoulder(Hand::Left),
            HolsterSite::Chest,
        ] {
            assert!(matches!(
                decide(site, Hand::Right, Hand::Right, heavy),
                Some(HolsterAction::Draw { .. })
            ));
        }
    }
}
