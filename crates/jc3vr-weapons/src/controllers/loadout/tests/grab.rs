//! The socket grab's decision against the table: draws, docks, refusals, and releases.

use jc3vr_body::controllers::grab::socket::Socket;
use jc3vr_input::frame::Hand;
use pretty_assertions::assert_eq;

use super::fixtures::{a_launcher, a_pistol, a_rifle, docked, table, take, weapon};
use crate::controllers::loadout::{
    grab::{Release, SocketMove, SocketRefusal, grab, refusing, release, swaps},
    item::ItemKind,
};

const RIGHT_SHOULDER: Socket = Socket::Shoulder(Hand::Right);
const LEFT_HIP: Socket = Socket::Hip(Hand::Left);

#[test]
fn an_empty_hand_draws_what_a_socket_holds() {
    let mut t = table();
    docked(&mut t, RIGHT_SHOULDER, 1, a_rifle());
    let s = t.snapshot();
    for hand in Hand::BOTH {
        assert_eq!(
            grab(&s, hand, RIGHT_SHOULDER),
            Ok(SocketMove::Draw {
                socket: RIGHT_SHOULDER,
                hand
            })
        );
    }
    assert_eq!(
        grab(&s, Hand::Left, LEFT_HIP),
        Err(SocketRefusal::Empty(LEFT_HIP))
    );
}

#[test]
fn a_hand_docks_its_weapon_on_a_free_socket_that_fits_it() {
    let mut t = table();
    take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    docked(&mut t, Socket::Shoulder(Hand::Left), 2, a_rifle());
    let s = t.snapshot();
    assert_eq!(
        grab(&s, Hand::Right, RIGHT_SHOULDER),
        Ok(SocketMove::Dock {
            hand: Hand::Right,
            socket: RIGHT_SHOULDER
        })
    );
    assert_eq!(
        grab(&s, Hand::Right, LEFT_HIP),
        Err(SocketRefusal::DoesNotFit(LEFT_HIP)),
        "a rifle is too big for a hip"
    );
    assert_eq!(
        grab(&s, Hand::Right, Socket::Shoulder(Hand::Left)),
        Err(SocketRefusal::Occupied(Socket::Shoulder(Hand::Left)))
    );
}

#[test]
fn a_hand_draws_while_the_other_holds_a_weapon() {
    let mut t = table();
    take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    docked(&mut t, LEFT_HIP, 2, a_pistol());
    assert!(grab(&t.snapshot(), Hand::Left, LEFT_HIP).is_ok());
}

#[test]
fn a_prop_fits_no_socket() {
    let mut t = table();
    take(&mut t, Hand::Right, 1, ItemKind::Prop);
    let s = t.snapshot();
    assert_eq!(
        grab(&s, Hand::Right, RIGHT_SHOULDER),
        Err(SocketRefusal::DoesNotFit(RIGHT_SHOULDER))
    );
    assert_eq!(
        refusing(&s, Hand::Right).count(),
        0,
        "only a weapon is cued"
    );
    assert_eq!(release(&s, Hand::Right, None, false), Release::LetGo);
}

#[test]
fn the_sockets_that_refuse_a_held_weapon_are_the_taken_and_the_small() {
    let mut t = table();
    take(&mut t, Hand::Right, 1, weapon(a_launcher()));
    docked(&mut t, Socket::Shoulder(Hand::Left), 2, a_rifle());
    let refused: Vec<Socket> = refusing(&t.snapshot(), Hand::Right).collect();
    assert_eq!(
        refused,
        [
            Socket::Hip(Hand::Left),
            Socket::Hip(Hand::Right),
            Socket::Shoulder(Hand::Left)
        ]
    );
    assert_eq!(refusing(&t.snapshot(), Hand::Left).count(), 0);
}

#[test]
fn a_release_docks_over_a_socket_that_fits_and_lets_go_elsewhere() {
    let mut t = table();
    take(&mut t, Hand::Right, 1, weapon(a_pistol()));
    let s = t.snapshot();
    assert_eq!(
        release(&s, Hand::Right, Some(LEFT_HIP), true),
        Release::Dock(LEFT_HIP)
    );
    assert_eq!(release(&s, Hand::Right, None, true), Release::LetGo);
    // A pair that cannot fall goes onto the socket the body prefers for it.
    assert_eq!(
        release(&s, Hand::Right, None, false),
        Release::Dock(Socket::Hip(Hand::Right))
    );
    let mut full = table();
    take(&mut full, Hand::Right, 1, weapon(a_pistol()));
    for (n, socket) in Socket::ALL.into_iter().enumerate() {
        docked(&mut full, socket, 10 + n, a_pistol());
    }
    assert_eq!(
        release(&full.snapshot(), Hand::Right, None, false),
        Release::Keep
    );
}

#[test]
fn a_release_over_a_taken_socket_that_fits_swaps() {
    let mut t = table();
    take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    docked(&mut t, RIGHT_SHOULDER, 2, a_rifle());
    docked(&mut t, LEFT_HIP, 3, a_pistol());
    let s = t.snapshot();
    assert!(swaps(&s, Hand::Right, RIGHT_SHOULDER));
    // A free back socket docks rather than swaps; a taken hip, too small for the rifle, neither.
    assert!(!swaps(&s, Hand::Right, Socket::Shoulder(Hand::Left)));
    assert!(!swaps(&s, Hand::Right, LEFT_HIP));
    // An empty hand swaps nothing.
    assert!(!swaps(&s, Hand::Left, RIGHT_SHOULDER));
    // A pistol let go of over a taken hip swaps for the pistol docked there.
    let mut t = table();
    take(&mut t, Hand::Right, 1, weapon(a_pistol()));
    docked(&mut t, LEFT_HIP, 2, a_pistol());
    assert!(swaps(&t.snapshot(), Hand::Right, LEFT_HIP));
    // Either shoulder accepts a launcher in place of a docked rifle.
    let mut t = table();
    take(&mut t, Hand::Right, 1, weapon(a_launcher()));
    docked(&mut t, Socket::Shoulder(Hand::Left), 2, a_rifle());
    assert!(swaps(
        &t.snapshot(),
        Hand::Right,
        Socket::Shoulder(Hand::Left)
    ));
}
