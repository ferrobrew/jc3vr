//! The player's and the game's moves: what each does, what it never touches, and its refusals.

use test_case::test_case;

use super::fixtures::*;
use crate::controllers::{
    frame::Hand,
    loadout::{
        item::{ItemId, ItemKind, SizeClass, WeaponType},
        socket::{BodyLayout, Socket},
        table::{Departure, Loadout, Place, primary},
        transition::{
            Cause, Change, Give, GiveSource, LostWhy, Release, Removal, RemovalWhy, Restored, Take,
            TakeSource, TransitionError,
        },
    },
};

#[test]
fn a_take_lands_in_the_grabbing_hand_and_never_touches_the_other() {
    let mut t = table();
    let rifle = take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    let pistol = take(&mut t, Hand::Left, 2, weapon(a_pistol()));
    assert_eq!(place(&t, rifle), Some(Place::Hand(Hand::Right)));
    assert_eq!(place(&t, pistol), Some(Place::Hand(Hand::Left)));
    // A second rifle with the right hand full is refused, rather than swapping anything.
    let refused = t.take(Take {
        hand: Hand::Right,
        object: object(3),
        kind: weapon(a_rifle()),
        source: TakeSource::Rack,
    });
    assert_eq!(
        refused,
        Err(TransitionError::HandOccupied {
            hand: Hand::Right,
            item: rifle
        })
    );
    assert_eq!(place(&t, rifle), Some(Place::Hand(Hand::Right)));
    check_invariants(&t);
}

#[test_case(TakeSource::World, ItemKind::Prop => true; "a prop off the ground")]
#[test_case(TakeSource::Rack, ItemKind::Prop => false; "no prop off a rack")]
#[test_case(TakeSource::Character, weapon(a_rifle()) => true; "a gun from a character")]
#[test_case(TakeSource::Pouch, ItemKind::Grenade => true; "a grenade from the pouch")]
#[test_case(TakeSource::Pouch, ItemKind::Charge => true; "a charge from the pouch")]
#[test_case(TakeSource::Pouch, weapon(a_pistol()) => false; "no weapon from the pouch")]
#[test_case(TakeSource::Catch, ItemKind::Grenade => true; "a caught grenade")]
#[test_case(TakeSource::Catch, ItemKind::Charge => false; "no caught charge")]
#[test_case(TakeSource::Snatch, weapon(a_launcher()) => true; "a snatched launcher")]
fn a_take_brings_only_what_its_source_holds(source: TakeSource, kind: ItemKind) -> bool {
    let mut t = table();
    let taken = t.take(Take {
        hand: Hand::Left,
        object: object(1),
        kind,
        source,
    });
    check_invariants(&t);
    match taken {
        Ok(_) => true,
        Err(error) => {
            assert_eq!(error, TransitionError::SourceMismatch { source, kind });
            assert_eq!(t.items().count(), 0);
            false
        }
    }
}

#[test]
fn a_weapon_picked_up_again_keeps_its_identity() {
    let mut t = table();
    let rifle = take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    t.let_go(Hand::Right, Release::Dropped).unwrap();
    assert_eq!(place(&t, rifle), Some(Place::World(Departure::Dropped)));
    let again = take(&mut t, Hand::Left, 1, weapon(a_rifle()));
    assert_eq!(again, rifle);
    assert_eq!(place(&t, rifle), Some(Place::Hand(Hand::Left)));
    // An object on the body is not taken a second time.
    assert_eq!(
        t.take(Take {
            hand: Hand::Right,
            object: object(1),
            kind: weapon(a_rifle()),
            source: TakeSource::World,
        }),
        Err(TransitionError::AlreadyInTable {
            item: rifle,
            place: Place::Hand(Hand::Left)
        })
    );
    check_invariants(&t);
}

#[test]
fn an_object_taken_as_another_kind_is_refused() {
    let mut t = table();
    let rifle = take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    t.let_go(Hand::Right, Release::Dropped).unwrap();
    assert_eq!(
        t.take(Take {
            hand: Hand::Right,
            object: object(1),
            kind: weapon(a_sniper()),
            source: TakeSource::World,
        }),
        Err(TransitionError::KindMismatch {
            item: rifle,
            known: weapon(a_rifle()),
            taken: weapon(a_sniper()),
        })
    );
    assert_eq!(place(&t, rifle), Some(Place::World(Departure::Dropped)));
}

#[test_case(a_pistol(), Socket::Hip(Hand::Right) => Ok(()); "a pistol on a hip")]
#[test_case(an_smg(), Socket::Chest => Ok(()); "a submachine gun on the chest")]
#[test_case(an_smg(), Socket::Hip(Hand::Left) => Ok(()); "a submachine gun on a hip")]
#[test_case(a_pistol(), Socket::Shoulder(Hand::Left) => Ok(()); "a pistol fits the back, being smaller")]
#[test_case(a_rifle(), Socket::Hip(Hand::Right) => Err(SizeClass::LongGun); "no rifle on a hip")]
#[test_case(a_launcher(), Socket::BackOuter => Ok(()); "a heavy weapon on the back")]
#[test_case(a_launcher(), Socket::Chest => Err(SizeClass::Heavy); "no heavy weapon on the chest")]
fn a_socket_takes_its_size_class_or_smaller(
    kind: WeaponType,
    socket: Socket,
) -> Result<(), SizeClass> {
    let mut t = table();
    let id = take(&mut t, Hand::Right, 1, weapon(kind));
    let docked = t.dock(Hand::Right, socket);
    check_invariants(&t);
    match docked {
        Ok(transitions) => {
            assert_eq!(transitions.len(), 1);
            assert_eq!(transitions[0].cause, Cause::Dock);
            assert_eq!(place(&t, id), Some(Place::Socket(socket)));
            Ok(())
        }
        Err(error) => {
            assert_eq!(
                error,
                TransitionError::DoesNotFit {
                    socket,
                    kind: weapon(kind)
                }
            );
            assert_eq!(
                place(&t, id),
                Some(Place::Hand(Hand::Right)),
                "refused whole"
            );
            Err(kind.size())
        }
    }
}

#[test]
fn a_prop_has_no_socket_and_letting_go_drops_it_out_of_the_table() {
    let mut t = table();
    let prop = take(&mut t, Hand::Left, 1, ItemKind::Prop);
    assert_eq!(
        t.dock(Hand::Left, Socket::Chest),
        Err(TransitionError::DoesNotFit {
            socket: Socket::Chest,
            kind: ItemKind::Prop
        })
    );
    let let_go = t.let_go(Hand::Left, Release::Dropped).unwrap();
    assert_eq!(
        let_go[0].change,
        Change::Left {
            from: Place::Hand(Hand::Left)
        }
    );
    assert_eq!(view(&t, prop), None);
}

#[test]
fn a_thrown_grenade_leaves_the_table() {
    let mut t = table();
    let grenade = take(&mut t, Hand::Right, 1, ItemKind::Grenade);
    t.let_go(Hand::Right, Release::AtSpeed).unwrap();
    assert_eq!(view(&t, grenade), None);
}

#[test]
fn a_dock_needs_a_socket_the_body_has_and_that_is_free() {
    let mut t = Loadout::new(BodyLayout::new(Hand::Right, 1, 1, false));
    take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    assert_eq!(
        t.dock(Hand::Right, Socket::Shoulder(Hand::Left)),
        Err(TransitionError::SocketAbsent(Socket::Shoulder(Hand::Left)))
    );
    t.dock(Hand::Right, Socket::Shoulder(Hand::Right)).unwrap();
    let second = take(&mut t, Hand::Right, 2, weapon(a_rifle()));
    let first = t.items().find(|i| i.id != second).map(|i| i.id).unwrap();
    assert_eq!(
        t.dock(Hand::Right, Socket::Shoulder(Hand::Right)),
        Err(TransitionError::SocketOccupied {
            socket: Socket::Shoulder(Hand::Right),
            item: first
        })
    );
    assert_eq!(
        t.dock(Hand::Left, Socket::Hip(Hand::Right)),
        Err(TransitionError::HandEmpty(Hand::Left))
    );
    check_invariants(&t);
}

#[test]
fn an_occupied_socket_the_body_no_longer_has_keeps_its_weapon_until_drawn() {
    let mut t = table();
    let sniper = docked(&mut t, Socket::BackOuter, 1, a_sniper());
    t.set_body(BodyLayout::new(Hand::Right, 2, 2, true));
    assert_eq!(place(&t, sniper), Some(Place::Socket(Socket::BackOuter)));
    t.draw(Socket::BackOuter, Hand::Left).unwrap();
    take(&mut t, Hand::Right, 2, weapon(a_rifle()));
    assert_eq!(
        t.dock(Hand::Left, Socket::BackOuter),
        Err(TransitionError::SocketAbsent(Socket::BackOuter))
    );
}

#[test]
fn a_draw_takes_the_socket_s_weapon_into_an_empty_hand() {
    let mut t = table();
    let rifle = docked(&mut t, Socket::Shoulder(Hand::Right), 1, a_rifle());
    assert_eq!(
        t.draw(Socket::Chest, Hand::Left),
        Err(TransitionError::SocketEmpty(Socket::Chest))
    );
    let pistol = take(&mut t, Hand::Left, 2, weapon(a_pistol()));
    assert_eq!(
        t.draw(Socket::Shoulder(Hand::Right), Hand::Left),
        Err(TransitionError::HandOccupied {
            hand: Hand::Left,
            item: pistol
        })
    );
    let drawn = t.draw(Socket::Shoulder(Hand::Right), Hand::Right).unwrap();
    assert_eq!(drawn[0].cause, Cause::Draw);
    assert_eq!(place(&t, rifle), Some(Place::Hand(Hand::Right)));
    check_invariants(&t);
}

#[test]
fn a_hand_over_needs_the_other_hand_empty() {
    let mut t = table();
    let rifle = take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    t.hand_over(Hand::Right).unwrap();
    assert_eq!(place(&t, rifle), Some(Place::Hand(Hand::Left)));
    assert_eq!(
        t.hand_over(Hand::Right),
        Err(TransitionError::HandEmpty(Hand::Right))
    );
    let pistol = take(&mut t, Hand::Right, 2, weapon(a_pistol()));
    assert_eq!(
        t.hand_over(Hand::Left),
        Err(TransitionError::HandOccupied {
            hand: Hand::Right,
            item: pistol
        })
    );
    check_invariants(&t);
}

#[test_case(Hand::Right, None, None => None; "empty hands")]
#[test_case(Hand::Right, Some(weapon(a_rifle())), Some(weapon(a_pistol())) => Some(Hand::Right); "the dominant hand's weapon")]
#[test_case(Hand::Right, Some(weapon(a_rifle())), None => Some(Hand::Left); "the other hand's weapon")]
#[test_case(Hand::Left, Some(weapon(a_rifle())), Some(weapon(a_pistol())) => Some(Hand::Left); "left-handed")]
#[test_case(Hand::Right, Some(weapon(a_rifle())), Some(ItemKind::Prop) => Some(Hand::Left); "a prop is no weapon")]
#[test_case(Hand::Right, Some(ItemKind::Grenade), Some(ItemKind::Charge) => None; "explosives are no weapons")]
fn the_primary_follows_the_hands(
    dominant: Hand,
    left: Option<ItemKind>,
    right: Option<ItemKind>,
) -> Option<Hand> {
    let mut t = Loadout::new(BodyLayout::full(dominant));
    for (hand, kind, n) in [(Hand::Left, left, 1), (Hand::Right, right, 2)] {
        if let Some(kind) = kind {
            take(&mut t, hand, n, kind);
        }
    }
    assert_eq!(t.primary(), primary([left, right], dominant));
    t.primary()
}

#[test]
fn the_primary_moves_with_a_hand_over_without_being_set() {
    let mut t = table();
    take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    assert_eq!(t.primary(), Some(Hand::Right));
    t.hand_over(Hand::Right).unwrap();
    assert_eq!(t.primary(), Some(Hand::Left));
    take(&mut t, Hand::Right, 2, weapon(a_pistol()));
    assert_eq!(t.primary(), Some(Hand::Right));
}

#[test_case(Release::Dropped, LostWhy::LeftBehind => None; "dropped and walked away from stays")]
#[test_case(Release::AtSpeed, LostWhy::LeftBehind => Some(Place::Socket(Socket::Shoulder(Hand::Right))); "left behind at speed returns")]
#[test_case(Release::Dropped, LostWhy::OutOfWorld => Some(Place::Socket(Socket::Shoulder(Hand::Right))); "out of the world returns")]
#[test_case(Release::AtSpeed, LostWhy::OutOfWorld => Some(Place::Socket(Socket::Shoulder(Hand::Right))); "out of the world at speed returns")]
fn a_weapon_returns_only_when_lost_rather_than_dropped(
    release: Release,
    why: LostWhy,
) -> Option<Place> {
    let mut t = table();
    let rifle = take(&mut t, Hand::Left, 1, weapon(a_rifle()));
    t.let_go(Hand::Left, release).unwrap();
    let lost = t.lose(object(1), why);
    check_invariants(&t);
    match lost {
        Ok(transitions) => {
            assert_eq!(transitions[0].cause, Cause::Lost(why));
            place(&t, rifle)
        }
        Err(error) => {
            assert_eq!(
                error,
                TransitionError::NotLost {
                    item: rifle,
                    departure: Departure::Dropped
                }
            );
            assert_eq!(place(&t, rifle), Some(Place::World(Departure::Dropped)));
            None
        }
    }
}

#[test]
fn a_lost_weapon_with_no_free_socket_comes_to_the_player_s_feet() {
    let mut t = Loadout::new(BodyLayout::new(Hand::Right, 1, 2, true));
    docked(&mut t, Socket::Shoulder(Hand::Right), 1, a_sniper());
    let rifle = take(&mut t, Hand::Left, 2, weapon(a_rifle()));
    t.let_go(Hand::Left, Release::Dropped).unwrap();
    t.lose(object(2), LostWhy::OutOfWorld).unwrap();
    assert_eq!(place(&t, rifle), Some(Place::World(Departure::Returned)));
    // Brought back, it was not left at speed, so it is not left behind again.
    assert_eq!(
        t.lose(object(2), LostWhy::LeftBehind),
        Err(TransitionError::NotLost {
            item: rifle,
            departure: Departure::Returned
        })
    );
}

#[test]
fn only_a_weapon_in_the_world_is_lost() {
    let mut t = table();
    let rifle = take(&mut t, Hand::Left, 1, weapon(a_rifle()));
    assert_eq!(
        t.lose(object(1), LostWhy::OutOfWorld),
        Err(TransitionError::NotInWorld {
            item: rifle,
            place: Place::Hand(Hand::Left)
        })
    );
    assert_eq!(
        t.lose(object(9), LostWhy::OutOfWorld),
        Err(TransitionError::UnknownObject(object(9)))
    );
}

fn give(t: &mut Loadout, n: usize, kind: WeaponType) -> Result<ItemId, TransitionError> {
    t.give(Give {
        object: object(n),
        weapon: kind,
        source: GiveSource::Mission,
    })
    .map(|transitions| transitions[0].item)
}

#[test_case(&[] => Place::Hand(Hand::Right); "to the dominant hand when both are free")]
#[test_case(&[Hand::Right] => Place::Hand(Hand::Left); "to the other hand when the dominant is full")]
#[test_case(&[Hand::Right, Hand::Left] => Place::Socket(Socket::Shoulder(Hand::Left)); "to the socket the game docks the slot on")]
fn a_give_goes_to_a_free_hand_then_a_fitting_socket(full: &[Hand]) -> Place {
    let mut t = table();
    for (n, hand) in full.iter().enumerate() {
        take(&mut t, *hand, n + 1, weapon(a_pistol()));
    }
    let id = give(&mut t, 10, a_sniper()).unwrap();
    check_invariants(&t);
    place(&t, id).unwrap()
}

#[test]
fn a_give_with_the_hands_and_the_back_full_comes_to_the_player_s_feet() {
    let mut t = table();
    for (n, socket) in [
        Socket::Shoulder(Hand::Right),
        Socket::Shoulder(Hand::Left),
        Socket::BackOuter,
    ]
    .into_iter()
    .enumerate()
    {
        docked(&mut t, socket, n + 1, a_rifle());
    }
    take(&mut t, Hand::Right, 4, weapon(a_pistol()));
    take(&mut t, Hand::Left, 5, weapon(a_pistol()));
    let id = give(&mut t, 6, a_launcher()).unwrap();
    assert_eq!(place(&t, id), Some(Place::World(Departure::Returned)));
    // The same object is not given twice.
    assert!(matches!(
        give(&mut t, 6, a_launcher()),
        Err(TransitionError::AlreadyInTable { .. })
    ));
    check_invariants(&t);
}

#[test]
fn a_removal_takes_every_weapon_on_the_body_and_leaves_the_rest() {
    let mut t = table();
    let on_hip = docked(&mut t, Socket::Hip(Hand::Right), 3, a_pistol());
    let lying = take(&mut t, Hand::Right, 5, weapon(a_sniper()));
    t.let_go(Hand::Right, Release::Dropped).unwrap();
    let held = take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    let prop = take(&mut t, Hand::Left, 2, ItemKind::Prop);
    t.let_go(Hand::Left, Release::Dropped).unwrap();
    let grenade = take(&mut t, Hand::Left, 4, ItemKind::Grenade);
    let removed = t.remove(Removal::AllWeapons, RemovalWhy::Mission).unwrap();
    let gone: Vec<ItemId> = removed.iter().map(|tr| tr.item).collect();
    assert_eq!(gone, vec![held, on_hip]);
    assert!(
        removed
            .iter()
            .all(|tr| tr.cause == Cause::Removed(RemovalWhy::Mission))
    );
    assert_eq!(view(&t, prop), None, "the prop left at its release");
    assert_eq!(place(&t, grenade), Some(Place::Hand(Hand::Left)));
    assert_eq!(
        place(&t, lying),
        Some(Place::World(Departure::Dropped)),
        "a weapon lying near the player is not the game's to take"
    );
}

#[test]
fn a_removal_names_an_object_or_an_item() {
    let mut t = table();
    let rifle = take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    let pistol = take(&mut t, Hand::Left, 2, weapon(a_pistol()));
    t.remove(Removal::Object(object(1)), RemovalWhy::TakenByOther)
        .unwrap();
    assert_eq!(view(&t, rifle), None);
    t.remove(Removal::Item(pistol), RemovalWhy::Gone).unwrap();
    assert_eq!(t.items().count(), 0);
    assert_eq!(
        t.remove(Removal::Object(object(1)), RemovalWhy::Gone),
        Err(TransitionError::UnknownObject(object(1)))
    );
    assert_eq!(
        t.remove(Removal::Item(pistol), RemovalWhy::Gone),
        Err(TransitionError::UnknownItem(pistol))
    );
}

#[test]
fn a_spawn_binds_only_an_item_that_waits_for_one() {
    let mut t = table();
    let rifle = take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    assert_eq!(
        t.spawned(rifle, object(2)),
        Err(TransitionError::NotAwaiting(rifle))
    );
    assert!(matches!(
        t.spawned(rifle, object(1)),
        Err(TransitionError::AlreadyInTable { .. })
    ));
}

#[test]
fn a_transition_logs_the_item_its_change_and_its_cause() {
    let mut t = table();
    take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    let docked = t.dock(Hand::Right, Socket::Shoulder(Hand::Right)).unwrap();
    assert_eq!(
        docked[0].to_string(),
        "loadout: #1 (a long gun of the two-handed slot (0x00000300)) moved from the right hand \
         to the right shoulder: docked"
    );
    assert_eq!(
        TransitionError::HandEmpty(Hand::Left).to_string(),
        "loadout: the left hand holds nothing"
    );
}

#[test]
fn identities_count_up_and_are_never_reused() {
    let mut t = table();
    let first = take(&mut t, Hand::Right, 1, ItemKind::Prop);
    t.let_go(Hand::Right, Release::Dropped).unwrap();
    let second = take(&mut t, Hand::Right, 1, ItemKind::Prop);
    assert!(second > first);
}

#[test]
fn a_game_drop_lays_a_weapon_in_the_world_from_wherever_it_is() {
    let mut t = table();
    let rifle = docked(&mut t, Socket::Shoulder(Hand::Right), 1, a_rifle());
    let pistol = take(&mut t, Hand::Left, 2, weapon(a_pistol()));
    let prop = take(&mut t, Hand::Right, 3, ItemKind::Prop);
    t.dropped(object(1)).unwrap();
    t.dropped(object(2)).unwrap();
    assert_eq!(place(&t, rifle), Some(Place::World(Departure::Dropped)));
    assert_eq!(place(&t, pistol), Some(Place::World(Departure::Dropped)));
    // A prop cannot lie in the world, so it leaves the table.
    let left = t.dropped(object(3)).unwrap();
    assert_eq!(
        left[0].change,
        Change::Left {
            from: Place::Hand(Hand::Right)
        }
    );
    assert_eq!(view(&t, prop), None);
    // A weapon already lying in the world stays as it was.
    assert_eq!(t.dropped(object(1)), Ok(Vec::new()));
    assert_eq!(
        t.dropped(object(9)),
        Err(TransitionError::UnknownObject(object(9)))
    );
    check_invariants(&t);
}

#[test]
fn a_restored_weapon_docks_where_it_was_recorded_or_where_it_fits() {
    let mut t = table();
    let restore = |t: &mut Loadout, n: usize, weapon: WeaponType, socket: Socket| {
        let transitions = t
            .restored(Restored {
                object: object(n),
                weapon,
                socket,
            })
            .unwrap();
        assert_eq!(transitions[0].cause, Cause::Restored);
        transitions[0].item
    };
    let sniper = restore(&mut t, 1, a_sniper(), Socket::BackOuter);
    assert_eq!(place(&t, sniper), Some(Place::Socket(Socket::BackOuter)));
    // Its socket is taken now, so the next goes where a weapon of its kind goes.
    let rifle = restore(&mut t, 2, a_rifle(), Socket::BackOuter);
    assert!(matches!(
        place(&t, rifle),
        Some(Place::Socket(Socket::Shoulder(_)))
    ));
    // A recorded socket that does not fit is not used.
    let launcher = restore(&mut t, 3, a_launcher(), Socket::Chest);
    assert!(matches!(
        place(&t, launcher),
        Some(Place::Socket(Socket::Shoulder(_)))
    ));
    assert!(matches!(
        t.restored(Restored {
            object: object(1),
            weapon: a_sniper(),
            socket: Socket::Chest,
        }),
        Err(TransitionError::AlreadyInTable { .. })
    ));
    check_invariants(&t);
}
