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
//! - A grab by the hand without a gun while the other hand holds one never switches weapons: that
//!   hand reaches for the pouches beside the holsters, and a press that lands in a holster must not
//!   take the gun from the other hand. It may still draw its own pistol with independent draws.
//! - With independent pistol draws ([`crate::controllers::weapons::dual_wield::draw`]), a hip
//!   grab acts on the grabbing hand's own pistol: a hand without one draws it, and a hand with one
//!   holsters it, the last pistol through the game's holster. A hip grab while the pistols are not
//!   wielded draws the slot as any other holster does, and the draw brings out only that hand's
//!   pistol.
//! - With second weapons ([`crate::controllers::weapons::hold::second`]), the hand without a gun
//!   takes a long gun of another slot off its shoulder as the second weapon instead of being
//!   refused, and a hand holding the second weapon puts it back at its own holster and draws
//!   nothing else.

use jc3gi::{character::inventory::WeaponSlot, input::input_action_map::Action};

use crate::controllers::{frame::Hand, grab::HolsterSite};

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
    /// A draw of `slot` by the hand without a gun while the other hand holds one, which would
    /// switch away from that gun.
    Refused { slot: WeaponSlot },
    /// Draw the grabbing hand's own pistol of the wielded pair.
    DrawPistol { hand: Hand },
    /// Holster the grabbing hand's own pistol, the other hand keeping its own.
    HolsterPistol { hand: Hand },
    /// Take `slot`'s long gun into the grabbing hand as the second weapon.
    TakeSecond { slot: WeaponSlot },
    /// Put the second weapon, of `slot`, back on its holster.
    PutBackSecond { slot: WeaponSlot },
    /// A grab of `slot`'s holster by a hand that holds the second weapon of another slot.
    HandFull { slot: WeaponSlot },
}

/// The second weapon, as the holsters see it.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Second {
    /// Whether the free hand may take a second weapon.
    pub enabled: bool,
    /// The hand holding the second weapon and its slot.
    pub held: Option<(Hand, WeaponSlot)>,
}

impl HolsterAction {
    pub fn label(self) -> String {
        match self {
            HolsterAction::Draw { slot, .. } => format!("draw {}", slot_label(slot)),
            HolsterAction::Holster { slot } => format!("holster {}", slot_label(slot)),
            HolsterAction::AlreadyDrawn { slot } => {
                format!("{} already drawn", slot_label(slot))
            }
            HolsterAction::Refused { slot } => {
                format!("{} not drawn: the other hand holds a gun", slot_label(slot))
            }
            HolsterAction::DrawPistol { hand } => format!("draw {} pistol", hand.label()),
            HolsterAction::HolsterPistol { hand } => format!("holster {} pistol", hand.label()),
            HolsterAction::TakeSecond { slot } => {
                format!("take {} as the second weapon", slot_label(slot))
            }
            HolsterAction::PutBackSecond { slot } => {
                format!("put the second weapon ({}) back", slot_label(slot))
            }
            HolsterAction::HandFull { slot } => {
                format!(
                    "{} not drawn: the hand holds the second weapon",
                    slot_label(slot)
                )
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

/// What `hand`'s grab at `site` does, with `wielded` in the hands and `second` the second weapon.
/// With `independent_pistols`, each hand draws and holsters its own pistol of the dual-wield slot.
pub fn decide(
    site: HolsterSite,
    hand: Hand,
    dominant: Hand,
    wielded: Option<Wielded>,
    independent_pistols: bool,
    second: Second,
) -> Option<HolsterAction> {
    let slot = site_slot(site, dominant);
    if let Some((holding, second_slot)) = second.held
        && holding == hand
    {
        return Some(if second_slot == slot {
            HolsterAction::PutBackSecond { slot }
        } else {
            HolsterAction::HandFull { slot }
        });
    }
    let per_hand = independent_pistols && slot == WeaponSlot::E_SLOT_DUAL_WIELD;
    // The hand without the gun is the one that reaches for the pouches, and a press of it that
    // lands in a holster must not switch away from the gun the other hand holds.
    let other_hand_armed =
        wielded.is_some_and(|w| !w.hands[hand.index()] && w.hands.contains(&true));
    let action = decide_for(slot, hand, wielded, per_hand)?;
    let second_free = second.enabled && second.held.is_none() && is_long_gun(slot);
    Some(match action {
        HolsterAction::Draw { slot, .. } if other_hand_armed && second_free => {
            HolsterAction::TakeSecond { slot }
        }
        HolsterAction::Draw { slot, .. } if other_hand_armed => HolsterAction::Refused { slot },
        action => action,
    })
}

/// What `hand`'s grab at the holster of `slot` does, before the free hand's refusal.
fn decide_for(
    slot: WeaponSlot,
    hand: Hand,
    wielded: Option<Wielded>,
    per_hand: bool,
) -> Option<HolsterAction> {
    match wielded {
        Some(w) if w.slot == slot && w.hands[hand.index()] => {
            if per_hand && w.hands[hand.other().index()] {
                Some(HolsterAction::HolsterPistol { hand })
            } else {
                Some(HolsterAction::Holster { slot })
            }
        }
        Some(w) if w.slot == slot => Some(if per_hand {
            HolsterAction::DrawPistol { hand }
        } else {
            HolsterAction::AlreadyDrawn { slot }
        }),
        _ => select_action(slot).map(|action| HolsterAction::Draw { slot, action }),
    }
}

/// Whether `slot` is a long gun's on a shoulder holster: the two-handed and special slots.
pub const fn is_long_gun(slot: WeaponSlot) -> bool {
    matches!(
        slot,
        WeaponSlot::E_SLOT_TWO_HANDED | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
    )
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
            decide(
                HolsterSite::Chest,
                Hand::Right,
                Hand::Right,
                None,
                false,
                Second::default()
            ),
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
            decide(
                site,
                Hand::Right,
                Hand::Right,
                rifle,
                false,
                Second::default()
            ),
            Some(HolsterAction::Holster {
                slot: WeaponSlot::E_SLOT_TWO_HANDED
            })
        );
        assert_eq!(
            decide(
                site,
                Hand::Left,
                Hand::Right,
                rifle,
                false,
                Second::default()
            ),
            Some(HolsterAction::AlreadyDrawn {
                slot: WeaponSlot::E_SLOT_TWO_HANDED
            })
        );
    }

    #[test]
    fn another_holster_switches_weapons_for_the_gun_hand_only() {
        let rifle = wielded(WeaponSlot::E_SLOT_TWO_HANDED, RIGHT_ONLY);
        for independent in [false, true] {
            assert!(matches!(
                decide(
                    HolsterSite::Hip(Hand::Left),
                    Hand::Right,
                    Hand::Right,
                    rifle,
                    independent,
                    Second::default()
                ),
                Some(HolsterAction::Draw {
                    action: Action::SELECT_DUEL_WIELD,
                    ..
                })
            ));
            // The free hand, feeling for a pouch beside a holster, never takes the rifle away.
            for site in [
                HolsterSite::Hip(Hand::Left),
                HolsterSite::Shoulder(Hand::Left),
                HolsterSite::Chest,
            ] {
                assert!(
                    matches!(
                        decide(
                            site,
                            Hand::Left,
                            Hand::Right,
                            rifle,
                            independent,
                            Second::default()
                        ),
                        Some(HolsterAction::Refused { .. })
                    ),
                    "{site:?} {independent}"
                );
            }
        }
        // With nothing wielded, either hand draws.
        assert!(matches!(
            decide(
                HolsterSite::Chest,
                Hand::Left,
                Hand::Right,
                None,
                false,
                Second::default()
            ),
            Some(HolsterAction::Draw { .. })
        ));
    }

    #[test]
    fn either_hand_holsters_dual_pistols() {
        let pistols = wielded(WeaponSlot::E_SLOT_DUAL_WIELD, [true, true]);
        for hand in Hand::BOTH {
            assert_eq!(
                decide(
                    HolsterSite::Hip(hand),
                    hand,
                    Hand::Right,
                    pistols,
                    false,
                    Second::default()
                ),
                Some(HolsterAction::Holster {
                    slot: WeaponSlot::E_SLOT_DUAL_WIELD
                })
            );
        }
    }

    #[test]
    fn each_hand_draws_and_holsters_its_own_pistol() {
        let left_only = wielded(WeaponSlot::E_SLOT_DUAL_WIELD, [true, false]);
        let both = wielded(WeaponSlot::E_SLOT_DUAL_WIELD, [true; 2]);
        // Either hip carries the pair; the grabbing hand decides which pistol.
        for site in Hand::BOTH.map(HolsterSite::Hip) {
            assert_eq!(
                decide(
                    site,
                    Hand::Right,
                    Hand::Right,
                    left_only,
                    true,
                    Second::default()
                ),
                Some(HolsterAction::DrawPistol { hand: Hand::Right })
            );
            assert_eq!(
                decide(site, Hand::Left, Hand::Right, both, true, Second::default()),
                Some(HolsterAction::HolsterPistol { hand: Hand::Left })
            );
            assert_eq!(
                decide(
                    site,
                    Hand::Left,
                    Hand::Right,
                    left_only,
                    true,
                    Second::default()
                ),
                Some(HolsterAction::Holster {
                    slot: WeaponSlot::E_SLOT_DUAL_WIELD
                }),
                "the last pistol goes through the game's holster"
            );
        }
    }

    #[test]
    fn independent_draws_leave_the_other_holsters_alone() {
        let rifle = wielded(WeaponSlot::E_SLOT_TWO_HANDED, RIGHT_ONLY);
        let site = HolsterSite::Shoulder(Hand::Right);
        assert_eq!(
            decide(
                site,
                Hand::Left,
                Hand::Right,
                rifle,
                true,
                Second::default()
            ),
            Some(HolsterAction::AlreadyDrawn {
                slot: WeaponSlot::E_SLOT_TWO_HANDED
            })
        );
        assert!(matches!(
            decide(
                HolsterSite::Hip(Hand::Left),
                Hand::Right,
                Hand::Right,
                rifle,
                true,
                Second::default()
            ),
            Some(HolsterAction::Draw {
                slot: WeaponSlot::E_SLOT_DUAL_WIELD,
                ..
            })
        ));
    }

    #[test]
    fn the_free_hand_takes_a_second_long_gun() {
        let rifle = wielded(WeaponSlot::E_SLOT_TWO_HANDED, RIGHT_ONLY);
        let on = Second {
            enabled: true,
            held: None,
        };
        let special = HolsterSite::Shoulder(Hand::Left);
        assert_eq!(
            decide(special, Hand::Left, Hand::Right, rifle, false, on),
            Some(HolsterAction::TakeSecond {
                slot: WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
            })
        );
        // A pistol or the explosive is never a second weapon, and the rifle's own holster draws
        // nothing new.
        assert!(matches!(
            decide(
                HolsterSite::Chest,
                Hand::Left,
                Hand::Right,
                rifle,
                false,
                on
            ),
            Some(HolsterAction::Refused { .. })
        ));
        assert!(matches!(
            decide(
                HolsterSite::Hip(Hand::Left),
                Hand::Left,
                Hand::Right,
                rifle,
                false,
                on
            ),
            Some(HolsterAction::Refused { .. })
        ));
        assert!(matches!(
            decide(
                HolsterSite::Shoulder(Hand::Right),
                Hand::Left,
                Hand::Right,
                rifle,
                false,
                on
            ),
            Some(HolsterAction::AlreadyDrawn { .. })
        ));
        // Off, the free hand is refused as before.
        assert!(matches!(
            decide(
                special,
                Hand::Left,
                Hand::Right,
                rifle,
                false,
                Second::default()
            ),
            Some(HolsterAction::Refused { .. })
        ));
    }

    #[test]
    fn the_second_weapon_s_hand_puts_it_back_and_draws_nothing_else() {
        let rifle = wielded(WeaponSlot::E_SLOT_TWO_HANDED, RIGHT_ONLY);
        let held = Second {
            enabled: true,
            held: Some((Hand::Left, WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL)),
        };
        assert_eq!(
            decide(
                HolsterSite::Shoulder(Hand::Left),
                Hand::Left,
                Hand::Right,
                rifle,
                false,
                held
            ),
            Some(HolsterAction::PutBackSecond {
                slot: WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
            })
        );
        assert_eq!(
            decide(
                HolsterSite::Chest,
                Hand::Left,
                Hand::Right,
                None,
                false,
                held
            ),
            Some(HolsterAction::HandFull {
                slot: WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE
            })
        );
        // The other hand acts as before.
        assert_eq!(
            decide(
                HolsterSite::Shoulder(Hand::Right),
                Hand::Right,
                Hand::Right,
                rifle,
                false,
                held
            ),
            Some(HolsterAction::Holster {
                slot: WeaponSlot::E_SLOT_TWO_HANDED
            })
        );
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
                decide(
                    site,
                    Hand::Right,
                    Hand::Right,
                    heavy,
                    false,
                    Second::default()
                ),
                Some(HolsterAction::Draw { .. })
            ));
        }
    }
}
