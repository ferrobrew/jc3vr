use jc3gi::{character::inventory::WeaponSlot, input::input_action_map::Action};
use jc3vr_body::controllers::{
    grab::GrabVolume,
    intent::{
        controls::ButtonControl,
        hold::{Desired, MapKind, Target, Want},
    },
};
use jc3vr_input::frame::Hand;

use crate::controllers::{
    loadout::{
        Socket,
        contents::{HandContents, HeldCharge, HeldGrenade},
        grab::SocketRefusal,
    },
    weapons::{
        begin::entered_body_volumes,
        capabilities::{GunHand, NativePair},
        engine::{HeldWeapon, Wielded},
        finish::{
            STAGES,
            fire::{
                FireRefusal, HandFire, RefusedFire, explosive_holds_fire, hand_fire,
                hold_fire_back, primary_action_blocked, primary_trigger_idle, refused_fire,
            },
            grabs::cues_grab,
            release::{passes_by_action, retry_socket, waiting_on_action},
        },
        foregrip::Transition,
        hold,
        reload::fire_hold::FireHold,
        types::{GrabOutcome, SocketOutcome},
    },
};

#[test]
fn the_stages_run_in_their_documented_order() {
    // The fire decision reads the hands after every move of the tick: a gun handed over, drawn,
    // docked, or let go of this tick fires from where it is now.
    assert_eq!(
        STAGES.map(|stage| stage.name),
        [
            "fingers", "foregrip", "grabs", "grenades", "C4", "reload", "release", "fire",
            "publish",
        ]
    );
}

#[test]
fn a_pending_release_never_retargets_another_socket_or_redocks_after_timeout() {
    let left = Socket::Hip(Hand::Left);
    let right = Socket::Hip(Hand::Right);
    let intent = hold::dock::ReleaseIntent::new(7, Some(left));
    assert_eq!(retry_socket(None, Some(right)), Some(right));
    assert_eq!(retry_socket(Some(intent), Some(left)), Some(left));
    assert_eq!(retry_socket(Some(intent), Some(right)), None);
    assert_eq!(retry_socket(Some(intent), None), None);
    let dropping = hold::dock::ReleaseIntent::new(7, None);
    assert_eq!(retry_socket(Some(dropping), Some(left)), None);
}

#[test]
fn a_holder_opening_while_the_other_hand_holds_the_action_passes_the_gun_at_once() {
    let grip = |down| ButtonControl {
        down,
        ..ButtonControl::default()
    };
    // The right hand holds the gun, and the left holds its slide with its grip closed.
    let action_held = [false, true];
    let passes = passes_by_action(action_held, [grip(true), grip(false)]);
    assert_eq!(passes, [false, true]);
    // The pass fires on the opening rather than waiting for the action to let go.
    assert_eq!(waiting_on_action([true, true], passes), [true, false]);
    // A hand that opened its grip on the slide passes nothing: the release waits as before.
    let open_on_slide = passes_by_action(action_held, [grip(false), grip(false)]);
    assert_eq!(open_on_slide, [false, false]);
    assert_eq!(waiting_on_action([true, true], open_on_slide), [true, true]);
}

#[test]
fn each_hand_fires_from_its_own_trigger_unless_its_gun_is_held_back() {
    let pulled = |down| ButtonControl {
        down,
        ..ButtonControl::default()
    };
    let triggers = [pulled(true), pulled(true)];
    let free = HandFire {
        allowed: [true; 2],
        triggers: &triggers,
        holder: Some(Hand::Right),
        docking: [false; 2],
        action_running: [false; 2],
    };
    assert_eq!(hand_fire(&free), [true, true]);
    assert_eq!(
        hand_fire(&HandFire {
            docking: [false, true],
            ..free
        }),
        [true, false],
        "a dock's slide holds only its own hand's gun"
    );
    assert_eq!(
        hand_fire(&HandFire {
            allowed: [false; 2],
            ..free
        }),
        [false, false]
    );
    let idle = [pulled(false), pulled(true)];
    assert_eq!(
        hand_fire(&HandFire {
            triggers: &idle,
            ..free
        }),
        [false, true]
    );
}

#[test]
fn a_trigger_press_that_fires_nothing_is_refused_once_with_its_reason() {
    let press = |pressed| ButtonControl {
        down: pressed,
        pressed,
        ..ButtonControl::default()
    };
    let triggers = [press(true), press(true)];
    let fire = HandFire {
        allowed: [false, true],
        triggers: &triggers,
        holder: Some(Hand::Right),
        docking: [false; 2],
        action_running: [false; 2],
    };
    // The right hand holds the grip and fires; the left braces.
    let refused = RefusedFire {
        fire: &fire,
        fired: hand_fire(&fire),
        reserved: [true; 2],
        roles: [GunHand::Support, GunHand::Grip],
        ready: [true; 2],
        may_fire: true,
        reload_held: [None; 2],
    };
    assert_eq!(refused_fire(&refused), [Some(FireRefusal::Bracing), None]);
    // A trigger held down from an earlier tick is not refused again.
    let held = [
        ButtonControl {
            down: true,
            ..ButtonControl::default()
        },
        press(false),
    ];
    let fire_held = HandFire {
        triggers: &held,
        ..fire
    };
    assert_eq!(
        refused_fire(&RefusedFire {
            fire: &fire_held,
            fired: hand_fire(&fire_held),
            ..refused
        }),
        [None, None]
    );
    // An empty hand's press is the game's.
    assert_eq!(
        refused_fire(&RefusedFire {
            reserved: [false, true],
            roles: [GunHand::Empty, GunHand::Grip],
            ..refused
        }),
        [None, None]
    );
    let docking = HandFire {
        docking: [false, true],
        ..fire
    };
    assert_eq!(
        refused_fire(&RefusedFire {
            fire: &docking,
            fired: hand_fire(&docking),
            ..refused
        })[Hand::Right.index()],
        Some(FireRefusal::Docking)
    );
    assert_eq!(
        refused_fire(&RefusedFire {
            ready: [true, false],
            fired: [false; 2],
            ..refused
        })[Hand::Right.index()],
        Some(FireRefusal::Untracked)
    );
    assert_eq!(
        refused_fire(&RefusedFire {
            reload_held: [None, Some(FireHold::Empty)],
            ..refused
        })[Hand::Right.index()],
        Some(FireRefusal::Reload(FireHold::Empty))
    );
    assert_eq!(
        FireRefusal::Reload(FireHold::ActionOpen).label(),
        FireHold::ActionOpen.label(),
        "the click's line says what the reload checked"
    );
    // The native pair's shared trigger is the game's, unless the reload holds the gun.
    let pair = GunHand::NativePair(NativePair {
        first_hand: Hand::Right,
    });
    let native = RefusedFire {
        roles: [pair; 2],
        ..refused
    };
    assert_eq!(refused_fire(&native), [None, None]);
    assert_eq!(
        refused_fire(&RefusedFire {
            reload_held: [Some(FireHold::MagazineOut); 2],
            ..native
        }),
        [Some(FireRefusal::Reload(FireHold::MagazineOut)); 2]
    );
}

#[test]
fn current_primary_holder_owns_the_action_block_after_same_tick_moves() {
    let triggers = [ButtonControl {
        down: true,
        ..Default::default()
    }; 2];
    for current in Hand::BOTH {
        let old = current.other();
        let running = Hand::BOTH.map(|hand| hand == current);
        let fire = HandFire {
            allowed: [true; 2],
            triggers: &triggers,
            holder: Some(current),
            docking: [false; 2],
            action_running: running,
        };
        assert!(
            !hand_fire(&fire)[current.index()],
            "the new primary remains held by its action"
        );
        assert!(
            hand_fire(&fire)[old.index()],
            "the previous holder's other gun is independent"
        );
        assert!(primary_action_blocked(Some(current), running));
        assert!(!primary_action_blocked(Some(old), running));
        assert!(
            !primary_action_blocked(None, running),
            "dropping the primary removes its action block"
        );
    }
}

#[test]
fn a_grenade_in_the_hand_holds_fire_back_unless_the_other_hand_has_a_gun() {
    let empty = HandContents::default();
    let grenade = HandContents {
        grenade: Some(HeldGrenade::Safe),
        ..empty
    };
    assert!(!explosive_holds_fire(&[empty; 2]));
    assert!(explosive_holds_fire(&[grenade, empty]));
    // The gun in the hand that now holds the grenade does not count.
    assert!(explosive_holds_fire(&[
        HandContents {
            wielded: true,
            ..grenade
        },
        empty
    ]));
    assert!(!explosive_holds_fire(&[
        grenade,
        HandContents {
            wielded: true,
            ..empty
        }
    ]));
    assert!(!explosive_holds_fire(&[
        grenade,
        HandContents {
            armed: Some(1),
            ..empty
        }
    ]));
    // A charge or the detonator holds the fire back as a grenade does.
    for explosive in [
        HandContents {
            charge: Some(HeldCharge::Asked),
            ..empty
        },
        HandContents {
            detonator: true,
            ..empty
        },
    ] {
        assert!(explosive_holds_fire(&[empty, explosive]));
    }
}

#[test]
fn a_hand_entering_a_socket_or_a_pouch_is_cued_once() {
    let hip = Some(GrabVolume::Socket(Socket::Hip(Hand::Right)));
    let pouch = Some(GrabVolume::MagazinePouch);
    let foregrip = Some(GrabVolume::Foregrip);
    assert_eq!(
        entered_body_volumes([None, None], [pouch, hip]),
        [true, true]
    );
    // Staying in a volume is not entering it again.
    assert_eq!(
        entered_body_volumes([pouch, hip], [pouch, hip]),
        [false, false]
    );
    // Moving from one body volume straight into another is.
    assert_eq!(
        entered_body_volumes([hip, None], [pouch, None]),
        [true, false]
    );
    // The foregrip is not on the body; its grab is cued instead.
    assert_eq!(
        entered_body_volumes([None; 2], [foregrip, None]),
        [false, false]
    );
    assert!(cues_grab(GrabOutcome::Foregrip(Transition::Grabbed)));
    assert!(cues_grab(GrabOutcome::Socket(SocketOutcome::Docked)));
    assert!(!cues_grab(GrabOutcome::Socket(SocketOutcome::Refused(
        SocketRefusal::Occupied(Socket::Hip(Hand::Left))
    ))));
    assert!(!cues_grab(GrabOutcome::Magazine));
}

fn rifle_in(hand: Hand) -> HeldWeapon {
    HeldWeapon {
        address: 0x1000,
        wielded: Wielded {
            slot: WeaponSlot::E_SLOT_TWO_HANDED,
            hands: Hand::BOTH.map(|h| h == hand),
        },
        holder: Some(hand),
        two_handed: true,
        class: jc3vr_body::controllers::held::guns::HeftClass::of(0, true),
    }
}

fn triggers(left: bool, right: bool) -> [ButtonControl; 2] {
    let pulled = |down| ButtonControl {
        down,
        ..ButtonControl::default()
    };
    [pulled(left), pulled(right)]
}

#[test]
fn only_a_hand_holding_the_gun_fires_it() {
    let left_rifle = Some(rifle_in(Hand::Left));
    // The other trigger alone fires nothing while the gun is in this hand.
    assert!(primary_trigger_idle(left_rifle, &triggers(false, true)));
    // The holding hand's trigger fires it, with or without the other trigger.
    assert!(!primary_trigger_idle(left_rifle, &triggers(true, false)));
    assert!(!primary_trigger_idle(left_rifle, &triggers(true, true)));
    // No trigger pulled holds nothing back.
    assert!(!primary_trigger_idle(left_rifle, &triggers(false, false)));
    // Mirrored: the trigger of a gun in the left hand does not fire the right-held gun.
    let right_rifle = Some(rifle_in(Hand::Right));
    assert!(primary_trigger_idle(right_rifle, &triggers(true, false)));
    assert!(!primary_trigger_idle(right_rifle, &triggers(false, true)));
    // Pistols in both hands fire from either trigger, and no weapon is the game's as before.
    let mut pair = rifle_in(Hand::Right);
    pair.wielded.hands = [true; 2];
    assert!(!primary_trigger_idle(Some(pair), &triggers(true, false)));
    assert!(!primary_trigger_idle(None, &triggers(true, true)));
}

#[test]
fn a_weapon_action_holds_only_the_fire_inputs_back() {
    let want = |action| Desired {
        target: Target {
            map: MapKind::Player,
            action,
        },
        want: Want::Hold(1.0),
    };
    let mut desired = vec![
        want(Action::FIRE_RIGHT),
        want(Action::R2),
        want(Action::JUMP),
    ];
    hold_fire_back(&mut desired);
    assert_eq!(desired, vec![want(Action::JUMP)]);
}
