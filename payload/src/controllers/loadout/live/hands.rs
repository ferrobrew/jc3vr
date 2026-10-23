//! The hands' moves, as events for the live table. Pure.
//!
//! A weapon the table has in a hand that has left it (the game holstered or dropped it, or the
//! hands put it back or let it fall) is a [`departure`], and one that came into a hand an
//! [`arrival`]. Each is made against the table as it is at that moment,
//! departures first, so a weapon handed from one hand to the other finds the other hand empty.
//!
//! A move the table already shows sends nothing, and a hand the table disagrees about is left as
//! the table has it.

use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::{Binding, ObjectAddress},
        event::Event,
        item::{ItemKind, WeaponType},
        socket::Socket,
        table::{Loadout, Place},
        transition::{Release, Take, TakeSource},
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
    /// Onto this socket, or the nearest that fits it when it is taken.
    Carried(Socket),
    /// Out of the player's keeping: dropped, taken, or gone.
    Away,
}

/// The event for `object` having left `hand` for `whereabouts`, when the table holds it there: a
/// dock onto the socket it went to (the carry set's, or for a slot's weapon the socket the game
/// docks its slot on), or a let-go. `None` when the table does not hold it in that hand, or no
/// socket of the table's that fits it is free.
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
    let preferred = match whereabouts {
        Whereabouts::Away => {
            return Some(Event::LetGo {
                hand,
                release: Release::Dropped,
            });
        }
        Whereabouts::Carried(socket) => Some(socket),
        Whereabouts::Slot => None,
    };
    let socket = dock_socket(table, held.kind.weapon()?, preferred)?;
    Some(Event::Dock { hand, socket })
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

/// The socket a weapon of `weapon` docks on in `table`: `preferred` when the body has it free,
/// otherwise the first free socket of the body's preference for it, the game's own dock for its
/// slot first.
pub(crate) fn dock_socket(
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
    use std::num::NonZeroUsize;

    use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::controllers::loadout::socket::BodyLayout;

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
    fn a_carried_weapon_docks_on_the_carry_set_s_socket_when_it_is_free() {
        let mut t = Loadout::new(BodyLayout::full(Hand::Right));
        let event = arrival(&t, Hand::Right, rifle(), TakeSource::World);
        apply(&mut t, event);
        let event = departure(
            &t,
            Hand::Right,
            rifle().object,
            Whereabouts::Carried(Socket::BackOuter),
        );
        assert_eq!(
            event,
            Some(Event::Dock {
                hand: Hand::Right,
                socket: Socket::BackOuter
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
