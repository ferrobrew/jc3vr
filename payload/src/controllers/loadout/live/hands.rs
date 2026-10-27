//! The hands' moves, as events for the live table. Pure.
//!
//! A weapon the table has in a hand that has left it (the game holstered or dropped it, or the
//! hands put it back or let it fall) is a [`departure`], and one that came into a hand an
//! [`arrival`]. Each is made against the table as it is at that moment, departures first, so a
//! weapon handed from one hand to the other finds the other hand empty. A prop, a grenade, or a
//! charge that left a hand or came into one is an [`other_departure`] or an [`other_arrival`].
//!
//! A move the table already shows sends nothing, and a hand the table disagrees about is left as
//! the table has it.

use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::{Binding, ObjectAddress},
        event::Event,
        item::{ItemKind, WeaponType},
        live::HeldOther,
        socket::Socket,
        table::{Loadout, Place},
        transition::{Release, Removal, RemovalWhy, Take, TakeSource},
    },
};

/// A weapon one of the old hand authorities holds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Held {
    pub object: ObjectAddress,
    pub weapon: WeaponType,
}

/// Where a weapon that left a hand went.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Whereabouts {
    /// Onto the body where the game docks its slot, or the nearest socket that fits it.
    Slot,
    /// Out of the player's keeping: dropped, taken, or gone.
    Away,
}

/// The event for `object` having left `hand` for `whereabouts`, when the table holds it there: a
/// dock onto the socket the game docks its slot on, or the nearest free one that fits it, or a
/// let-go. `None` when the table does not hold it in that hand, or no socket of the table's that
/// fits it is free.
pub(crate) fn departure(
    table: &Loadout,
    hand: Hand,
    object: ObjectAddress,
    whereabouts: Whereabouts,
) -> Option<Event> {
    let held = table.held(hand)?;
    if held.binding != Binding::Live(object) {
        return None;
    }
    match whereabouts {
        Whereabouts::Away => Some(Event::LetGo {
            hand,
            release: Release::Dropped,
        }),
        Whereabouts::Slot => {
            let socket = free_socket(table, held.kind.weapon()?, None)?;
            Some(Event::Dock { hand, socket })
        }
    }
}

/// The event for `held` having come into `hand`: a hand-over from the other hand, a draw off its
/// socket, or a take, from the ground for a weapon the table has lying in the world and otherwise
/// from `source`. `None` when the table already has it in that hand.
pub(crate) fn arrival(
    table: &Loadout,
    hand: Hand,
    held: Held,
    source: TakeSource,
) -> Option<Event> {
    let known = table
        .items()
        .find(|item| item.binding == Binding::Live(held.object));
    match known.map(|item| (item.place, item.kind)) {
        Some((Place::Hand(at), _)) if at == hand => None,
        Some((Place::Hand(from), _)) => Some(Event::HandOver { from }),
        Some((Place::Socket(socket), _)) => Some(Event::Draw { socket, hand }),
        Some((Place::World(_), kind)) => Some(Event::Take(Take {
            hand,
            object: held.object,
            kind,
            source: TakeSource::World,
        })),
        None => Some(Event::Take(Take {
            hand,
            object: held.object,
            kind: ItemKind::Weapon(held.weapon),
            source,
        })),
    }
}

/// The event for the prop, grenade, or charge the table has in `hand` having left it, when the
/// hand no longer holds it (`wanted` is what the hand holds now): a let-go for a prop, or for a
/// grenade or a charge the hand threw (`thrown`), and otherwise a removal, the object gone (a
/// grenade that went off in the hand, a charge the game took back). `None` when the hand still
/// holds it, or the table has no such item there.
pub(crate) fn other_departure(
    table: &Loadout,
    hand: Hand,
    wanted: Option<HeldOther>,
    thrown: bool,
) -> Option<Event> {
    let held = table
        .held(hand)
        .filter(|item| item.kind.weapon().is_none())?;
    if table.safe_item(held.id).is_some() {
        return None;
    }
    if wanted.is_some_and(|w| held.binding == Binding::Live(w.object)) {
        return None;
    }
    if thrown || held.kind == ItemKind::Prop {
        return Some(Event::LetGo {
            hand,
            release: Release::Dropped,
        });
    }
    Some(Event::Removed {
        target: Removal::Object(held.binding.address()?),
        why: RemovalWhy::Gone,
    })
}

/// The event for `wanted`, a prop, a grenade, or a charge, having come into `hand`: a take from
/// where the hand took it. `None` when the hand holds something, or the table already has the
/// object (a prop the other hand holds too, which is the first hand's).
pub(crate) fn other_arrival(
    table: &Loadout,
    hand: Hand,
    wanted: Option<HeldOther>,
) -> Option<Event> {
    let wanted = wanted?;
    let known = table
        .items()
        .any(|item| item.binding == Binding::Live(wanted.object));
    (table.held(hand).is_none() && !known).then_some(Event::Take(Take {
        hand,
        object: wanted.object,
        kind: wanted.kind,
        source: wanted.source,
    }))
}

/// The socket a weapon of `weapon` goes onto in `table` when no hand chose one: `preferred` when the
/// body has it free and it fits, otherwise the first free socket of the body's preference for it,
/// the game's own dock for its slot first.
pub(crate) fn free_socket(
    table: &Loadout,
    weapon: WeaponType,
    preferred: Option<Socket>,
) -> Option<Socket> {
    let body = table.body();
    let free = |socket: Socket| {
        body.has(socket)
            && socket.fits(weapon.size())
            && !table
                .items()
                .any(|item| item.place == Place::Socket(socket))
    };
    preferred
        .filter(|&socket| free(socket))
        .or_else(|| body.preference(weapon).find(|&socket| free(socket)))
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU64, NonZeroUsize};

    use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::controllers::loadout::{safe_items::ItemLeaseId, socket::BodyLayout};

    fn object(n: usize) -> ObjectAddress {
        ObjectAddress::new(NonZeroUsize::MIN.saturating_add(n * 0x10))
    }

    fn weapon(slot: WeaponSlot, class: WeaponClass) -> WeaponType {
        WeaponType::new(0x100 + slot as u32, slot, class as i32).unwrap()
    }

    fn rifle() -> Held {
        Held {
            object: object(1),
            weapon: weapon(
                WeaponSlot::E_SLOT_TWO_HANDED,
                WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE,
            ),
        }
    }

    fn apply(table: &mut Loadout, event: Option<Event>) {
        table.fold(event.expect("an event")).unwrap();
    }

    fn place(table: &Loadout, object: ObjectAddress) -> Option<Place> {
        table
            .items()
            .find(|item| item.binding == Binding::Live(object))
            .map(|item| item.place)
    }

    #[test]
    fn an_unknown_weapon_coming_into_a_hand_is_taken_from_its_source() {
        let mut t = Loadout::new(BodyLayout::full(Hand::Right));
        let event = arrival(&t, Hand::Left, rifle(), TakeSource::Character);
        assert!(matches!(
            event,
            Some(Event::Take(Take {
                source: TakeSource::Character,
                ..
            }))
        ));
        apply(&mut t, event);
        assert_eq!(place(&t, rifle().object), Some(Place::Hand(Hand::Left)));
        assert_eq!(
            arrival(&t, Hand::Left, rifle(), TakeSource::Character),
            None,
            "already there"
        );
    }

    #[test]
    fn a_holstered_weapon_docks_on_its_slot_s_socket_and_is_drawn_from_it() {
        let mut t = Loadout::new(BodyLayout::full(Hand::Right));
        let event = arrival(&t, Hand::Right, rifle(), TakeSource::Give);
        apply(&mut t, event);
        let event = departure(&t, Hand::Right, rifle().object, Whereabouts::Slot);
        apply(&mut t, event);
        let Some(Place::Socket(socket)) = place(&t, rifle().object) else {
            panic!("docked");
        };
        assert_eq!(
            Some(socket),
            BodyLayout::full(Hand::Right)
                .preference(rifle().weapon)
                .next()
        );
        let draw = arrival(&t, Hand::Left, rifle(), TakeSource::Give);
        assert_eq!(
            draw,
            Some(Event::Draw {
                socket,
                hand: Hand::Left
            })
        );
    }

    #[test]
    fn a_weapon_away_is_let_go_of_and_taken_again_off_the_ground() {
        let mut t = Loadout::new(BodyLayout::full(Hand::Right));
        let event = arrival(&t, Hand::Right, rifle(), TakeSource::World);
        apply(&mut t, event);
        let event = departure(&t, Hand::Right, rifle().object, Whereabouts::Away);
        apply(&mut t, event);
        assert!(matches!(place(&t, rifle().object), Some(Place::World(_))));
        let again = arrival(&t, Hand::Left, rifle(), TakeSource::Rack);
        assert!(
            matches!(
                again,
                Some(Event::Take(Take {
                    source: TakeSource::World,
                    ..
                }))
            ),
            "a weapon the table has lying in the world is taken off the ground"
        );
    }

    #[test]
    fn a_weapon_moving_to_the_other_hand_is_handed_over() {
        let mut t = Loadout::new(BodyLayout::full(Hand::Right));
        let event = arrival(&t, Hand::Right, rifle(), TakeSource::World);
        apply(&mut t, event);
        assert_eq!(
            arrival(&t, Hand::Left, rifle(), TakeSource::World),
            Some(Event::HandOver { from: Hand::Right })
        );
    }

    #[test]
    fn a_hand_the_table_disagrees_about_sends_nothing() {
        let t = Loadout::new(BodyLayout::full(Hand::Right));
        assert_eq!(
            departure(&t, Hand::Right, rifle().object, Whereabouts::Away),
            None
        );
    }

    fn other(n: usize, kind: ItemKind, source: TakeSource) -> HeldOther {
        HeldOther {
            object: object(n),
            kind,
            source,
        }
    }

    #[test]
    fn a_prop_comes_into_the_hand_and_leaves_with_its_let_go() {
        let mut t = Loadout::new(BodyLayout::full(Hand::Right));
        let prop = other(7, ItemKind::Prop, TakeSource::World);
        let event = other_arrival(&t, Hand::Left, Some(prop));
        apply(&mut t, event);
        assert_eq!(place(&t, prop.object), Some(Place::Hand(Hand::Left)));
        assert_eq!(other_arrival(&t, Hand::Left, Some(prop)), None);
        assert_eq!(other_departure(&t, Hand::Left, Some(prop), false), None);
        assert_eq!(
            other_arrival(&t, Hand::Right, Some(prop)),
            None,
            "a prop both hands hold is the first hand's"
        );
        let event = other_departure(&t, Hand::Left, None, false);
        assert_eq!(
            event,
            Some(Event::LetGo {
                hand: Hand::Left,
                release: Release::Dropped
            })
        );
        apply(&mut t, event);
        assert_eq!(place(&t, prop.object), None, "a prop let go of leaves");
    }

    #[test]
    fn a_grenade_thrown_is_let_go_of_and_one_gone_is_removed() {
        let grenade = other(8, ItemKind::Grenade, TakeSource::Pouch);
        for (thrown, let_go) in [(true, true), (false, false)] {
            let mut t = Loadout::new(BodyLayout::full(Hand::Right));
            let event = other_arrival(&t, Hand::Right, Some(grenade));
            apply(&mut t, event);
            let event = other_departure(&t, Hand::Right, None, thrown);
            assert_eq!(matches!(event, Some(Event::LetGo { .. })), let_go);
            apply(&mut t, event);
            assert_eq!(place(&t, grenade.object), None);
        }
    }

    #[test]
    fn a_hand_holding_a_weapon_takes_no_grenade() {
        let mut t = Loadout::new(BodyLayout::full(Hand::Right));
        let event = arrival(&t, Hand::Right, rifle(), TakeSource::World);
        apply(&mut t, event);
        let charge = other(9, ItemKind::Charge, TakeSource::Pouch);
        assert_eq!(other_arrival(&t, Hand::Right, Some(charge)), None);
        assert_eq!(
            other_departure(&t, Hand::Right, None, false),
            None,
            "a weapon is not the others' to move"
        );
    }

    #[test]
    fn native_other_reconciliation_cannot_remove_an_actor_free_item() {
        let mut table = Loadout::new(BodyLayout::full(Hand::Right));
        let safe = table
            .admit_safe_item(ItemLeaseId::new(NonZeroU64::MIN), Hand::Left)
            .unwrap();
        let before = table.snapshot();
        for thrown in [false, true] {
            assert_eq!(other_departure(&table, Hand::Left, None, thrown), None);
            assert_eq!(
                other_departure(
                    &table,
                    Hand::Left,
                    Some(other(8, ItemKind::Grenade, TakeSource::Pouch)),
                    thrown
                ),
                None
            );
        }
        assert_eq!(table.snapshot(), before);
        assert_eq!(table.safe_item(safe.item), Some(safe));
    }

    #[test]
    fn a_full_body_leaves_nowhere_to_dock() {
        let mut t = Loadout::new(BodyLayout::new(Hand::Right, 0, 0, false));
        let event = arrival(&t, Hand::Right, rifle(), TakeSource::World);
        apply(&mut t, event);
        assert_eq!(
            departure(&t, Hand::Right, rifle().object, Whereabouts::Slot),
            None
        );
    }
}
