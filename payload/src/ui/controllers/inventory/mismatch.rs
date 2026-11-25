//! Where the game's inventory and the loadout table disagree. The reconcile keeps the game's
//! tracked slots holding what the table projects into them and the wielded weapon the primary
//! hand's, so a disagreement that outlasts a tick is a bug in the table or in the reconcile.

use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::{
    loadout::{LoadoutSnapshot, ObjectAddress, Place, SlotContent},
    weapons::inventory::game_view::{GameInventory, GameSlot},
};

/// The disagreements between one capture of the game's inventory and one table snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Mismatches {
    /// The slots whose weapon is not the one the table projects there.
    pub slots: Vec<WeaponSlot>,
    /// Whether the game wields another weapon than the primary hand's, where its weapon updates
    /// do not skip the wielded weapon.
    pub wielded: bool,
}

impl Mismatches {
    /// Find the disagreements between `game` and `table`.
    pub(super) fn find(game: &GameInventory, table: &LoadoutSnapshot) -> Mismatches {
        let slots = game
            .slots
            .iter()
            .filter(|slot| slot_disagrees(expected(table, slot.slot), actual(slot)))
            .map(|slot| slot.slot)
            .collect();
        let primary = table.primary.and_then(|hand| {
            table
                .held(hand)
                .filter(|item| item.kind.weapon().is_some())
                .and_then(|item| item.binding.address())
        });
        let on_body = |object: ObjectAddress| {
            table.items.iter().any(|item| {
                item.binding.address() == Some(object) && !matches!(item.place, Place::World(_))
            })
        };
        Mismatches {
            slots,
            wielded: !game.wield_skipped && wielded_disagrees(primary, game.wielded, on_body),
        }
    }

    pub(super) fn slot(&self, slot: WeaponSlot) -> bool {
        self.slots.contains(&slot)
    }

    pub(super) fn any(&self) -> bool {
        !self.slots.is_empty() || self.wielded
    }
}

/// What the table's projection puts in a slot.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Expected {
    /// A weapon of the table's, by its object, or `None` when the table has no live object for it.
    Weapon(Option<ObjectAddress>),
    Empty,
    /// The game's own slot, which the table never holds.
    Game,
}

/// What one of the game's slots holds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Actual {
    Weapon(ObjectAddress),
    Empty,
    /// The slot is changing hands for the game's own reasons: a spawn is on its way, or the player
    /// mans a mounted gun, which the game keeps in the heavy slot meanwhile.
    Settling,
}

/// What the table projects into `slot`.
pub(super) fn expected(table: &LoadoutSnapshot, slot: WeaponSlot) -> Expected {
    let Some(fill) = table.projection.slots.iter().find(|f| f.slot == slot) else {
        return Expected::Game;
    };
    match fill.content {
        SlotContent::Weapon { item, .. } => Expected::Weapon(
            table
                .items
                .iter()
                .find(|i| i.id == item)
                .and_then(|i| i.binding.address()),
        ),
        SlotContent::Empty => Expected::Empty,
        SlotContent::Game => Expected::Game,
    }
}

fn actual(slot: &GameSlot) -> Actual {
    match &slot.weapon {
        _ if slot.pending_spawn => Actual::Settling,
        Some(weapon) if weapon.mounted() => Actual::Settling,
        Some(weapon) => Actual::Weapon(weapon.object),
        None => Actual::Empty,
    }
}

/// Whether a slot holding `actual` disagrees with the table's `expected`.
fn slot_disagrees(expected: Expected, actual: Actual) -> bool {
    match (expected, actual) {
        (Expected::Game, _) | (_, Actual::Settling) => false,
        (Expected::Empty, Actual::Empty) => false,
        (Expected::Weapon(Some(want)), Actual::Weapon(have)) => want != have,
        (Expected::Weapon(_), _) | (Expected::Empty, Actual::Weapon(_)) => true,
    }
}

/// Whether the game's `wielded` weapon disagrees with the primary hand's weapon `primary`. A
/// weapon the game wields that the table does not have on the body (`on_body`), such as a grenade
/// or the planted explosive, is the game's to wield.
fn wielded_disagrees(
    primary: Option<ObjectAddress>,
    wielded: Option<ObjectAddress>,
    on_body: impl Fn(ObjectAddress) -> bool,
) -> bool {
    if wielded.is_some_and(|w| !on_body(w)) {
        return false;
    }
    primary != wielded
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;

    fn object(address: usize) -> ObjectAddress {
        ObjectAddress::from_address(address).unwrap()
    }

    #[test_case(Expected::Game, Actual::Weapon(object(1)), false; "the game's slot")]
    #[test_case(Expected::Game, Actual::Empty, false; "the game's empty slot")]
    #[test_case(Expected::Empty, Actual::Empty, false; "both empty")]
    #[test_case(Expected::Empty, Actual::Weapon(object(1)), true; "a weapon the table does not project")]
    #[test_case(Expected::Weapon(Some(object(1))), Actual::Weapon(object(1)), false; "the projected weapon")]
    #[test_case(Expected::Weapon(Some(object(1))), Actual::Weapon(object(2)), true; "another weapon")]
    #[test_case(Expected::Weapon(Some(object(1))), Actual::Empty, true; "a projected weapon missing")]
    #[test_case(Expected::Weapon(None), Actual::Empty, true; "a projected weapon with no object")]
    #[test_case(Expected::Weapon(Some(object(1))), Actual::Settling, false; "a spawn on its way")]
    #[test_case(Expected::Empty, Actual::Settling, false; "a mounted gun")]
    fn slots_disagree_only_where_the_reconcile_would_act(
        expected: Expected,
        actual: Actual,
        disagrees: bool,
    ) {
        assert_eq!(slot_disagrees(expected, actual), disagrees);
    }

    #[test]
    fn the_wielded_weapon_follows_the_primary_hand() {
        let body = |o: ObjectAddress| o.get() <= 2;
        assert!(!wielded_disagrees(None, None, body));
        assert!(!wielded_disagrees(Some(object(1)), Some(object(1)), body));
        assert!(wielded_disagrees(Some(object(1)), Some(object(2)), body));
        assert!(wielded_disagrees(Some(object(1)), None, body));
        assert!(wielded_disagrees(None, Some(object(1)), body));
    }

    #[test]
    fn a_weapon_off_the_body_is_the_game_s_to_wield() {
        let body = |o: ObjectAddress| o.get() <= 2;
        assert!(!wielded_disagrees(None, Some(object(9)), body));
        assert!(!wielded_disagrees(Some(object(1)), Some(object(9)), body));
    }
}
