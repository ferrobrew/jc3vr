use glam::Vec3;

use super::{
    latch::{GripLatch, LatchTick},
    markers::{self, colour, test_frame},
    resolve::{Lost, choose, dock_target, losers, resolve},
    *,
};
use crate::controllers::{
    bindings::controls::ButtonControl,
    loadout::Socket,
    overlay::primitive::{Anchor, Primitive},
};

fn sphere(volume: GrabVolume, x: f32, radius: f32, hand: Option<Hand>) -> GripTarget {
    GripTarget::sphere(
        volume,
        hand,
        Vec3::new(x, 0.0, 0.0),
        radius,
        Mark::ring(MarkAt::Cockpit(Vec3::new(x, 0.0, 0.0)), 0.05),
    )
}

fn at(x: f32) -> Option<Vec3> {
    Some(Vec3::new(x, 0.0, 0.0))
}

/// What `hand` at `x` would take among `targets`.
fn takes(targets: &[GripTarget], hand: Hand, x: f32) -> Option<GrabVolume> {
    let mut points = [None; 2];
    points[hand.index()] = at(x);
    resolve(targets, points)[hand.index()]
        .chosen
        .map(|c| c.volume)
}

const HIP: GrabVolume = GrabVolume::Socket(Socket::Hip(Hand::Right));

#[test]
fn a_point_outside_every_reach_takes_nothing() {
    let t = [sphere(GrabVolume::Foregrip, 0.0, 0.1, None)];
    assert_eq!(takes(&t, Hand::Left, 0.2), None);
    assert_eq!(takes(&t, Hand::Left, f32::NAN), None);
    assert_eq!(resolve(&t, [None; 2])[0].chosen, None, "an untracked hand");
}

#[test]
fn the_nearest_grab_point_wins_whatever_its_reach() {
    // A large socket reach and a small pouch beside it: the hand takes whichever grab point it is
    // nearer, not the one it is deepest in.
    let t = [
        sphere(HIP, 0.0, 0.3, None),
        sphere(GrabVolume::GrenadePouch, 0.15, 0.1, None),
    ];
    assert_eq!(takes(&t, Hand::Right, 0.09), Some(GrabVolume::GrenadePouch));
    assert_eq!(takes(&t, Hand::Right, 0.07), Some(HIP));
}

#[test]
fn a_target_for_one_hand_ignores_the_other() {
    let t = [sphere(GrabVolume::Foregrip, 0.0, 0.1, Some(Hand::Left))];
    assert_eq!(takes(&t, Hand::Right, 0.0), None);
    assert_eq!(takes(&t, Hand::Left, 0.0), Some(GrabVolume::Foregrip));
}

#[test]
fn a_degenerate_reach_holds_nothing() {
    let t = [
        sphere(GrabVolume::Foregrip, 0.0, 0.0, None),
        sphere(GrabVolume::Handle, 0.0, f32::NAN, None),
    ];
    assert_eq!(takes(&t, Hand::Left, 0.0), None);
}

#[test]
fn a_fallback_is_taken_only_when_no_target_is_in_reach() {
    let climb = GripTarget::measured(
        GrabVolume::Climb,
        Hand::Left,
        0.0,
        Mark::ring(MarkAt::World(Vec3::ZERO), 0.05),
    );
    let prop = GripTarget::measured(
        GrabVolume::Pickup(PickupKind::Prop),
        Hand::Left,
        0.12,
        Mark::ring(MarkAt::World(Vec3::X), 0.05),
    );
    assert_eq!(climb.tier, Tier::Fallback);
    assert_eq!(
        takes(&[climb, prop], Hand::Left, 0.0),
        Some(GrabVolume::Pickup(PickupKind::Prop))
    );
    assert_eq!(takes(&[climb], Hand::Left, 0.0), Some(GrabVolume::Climb));
    // A pouch's reach margin ranks as a fallback too.
    let margin = sphere(GrabVolume::GrenadePouch, 0.0, 0.2, None).ranked(Tier::Fallback);
    let hip = sphere(HIP, 0.15, 0.2, None);
    assert_eq!(takes(&[margin, hip], Hand::Right, 0.0), Some(HIP));
}

#[test]
fn a_remote_target_is_taken_only_when_nothing_at_all_is_in_reach() {
    let remote = GripTarget::measured(
        GrabVolume::Snatch(RemoteKind::Weapon),
        Hand::Left,
        10.0,
        Mark::ring(MarkAt::World(Vec3::NEG_Z * 10.0), 0.1),
    );
    assert_eq!(remote.tier, Tier::Remote);
    let climb = GripTarget::measured(
        GrabVolume::Climb,
        Hand::Left,
        0.0,
        Mark::ring(MarkAt::World(Vec3::ZERO), 0.05),
    );
    let crate_in_reach = GripTarget::measured(
        GrabVolume::Pickup(PickupKind::Prop),
        Hand::Left,
        0.1,
        Mark::ring(MarkAt::World(Vec3::X), 0.05),
    );
    // A reach margin, which has no marker of its own, still wins.
    let margin = sphere(GrabVolume::GrenadePouch, 0.0, 0.2, None).ranked(Tier::Fallback);
    assert_eq!(
        takes(&[remote], Hand::Left, 0.0),
        Some(GrabVolume::Snatch(RemoteKind::Weapon))
    );
    assert_eq!(
        takes(&[remote, crate_in_reach], Hand::Left, 0.0),
        Some(GrabVolume::Pickup(PickupKind::Prop))
    );
    assert_eq!(
        takes(&[remote, climb], Hand::Left, 0.0),
        Some(GrabVolume::Climb)
    );
    assert_eq!(
        takes(&[remote, margin], Hand::Left, 0.0),
        Some(GrabVolume::GrenadePouch)
    );
    let resolved = &resolve(&[remote, climb], [at(0.0), None])[0];
    let lost = losers(&resolved.reaches, resolved.chosen.unwrap());
    assert_eq!(lost[0].1, Lost::Remote);
}

#[test]
fn a_fist_never_arms_a_snatch() {
    let mut latch = GripLatch::default();
    assert_eq!(latch.step(tick(PRESS, None)), None);
    assert_eq!(
        latch.step(tick(HELD, Some(GrabVolume::Snatch(RemoteKind::Weapon)))),
        None
    );
    assert!(is_fist(latch), "a closed hand passing over a remote target");
    let mut pressed = GripLatch::default();
    assert_eq!(
        pressed.step(tick(PRESS, Some(GrabVolume::Snatch(RemoteKind::Weapon)))),
        Some(GrabVolume::Snatch(RemoteKind::Weapon)),
        "a press aimed at it arms it"
    );
}

#[test]
fn a_held_target_is_kept_over_anything_nearer() {
    let wheel = sphere(
        GrabVolume::VehicleControl(ControlKind::Wheel),
        0.5,
        0.1,
        None,
    )
    .held(Hand::Left);
    let pouch = sphere(GrabVolume::GrenadePouch, 0.0, 0.1, None);
    assert_eq!(
        takes(&[wheel, pouch], Hand::Left, 0.0),
        Some(GrabVolume::VehicleControl(ControlKind::Wheel)),
        "ties keep the first, and the holder places its held target first"
    );
    assert_eq!(
        takes(&[wheel, pouch], Hand::Right, 0.0),
        Some(GrabVolume::GrenadePouch),
        "the other hand does not reach what this one holds"
    );
}

#[test]
fn a_target_worked_by_the_palm_is_never_taken() {
    let horn = sphere(GrabVolume::CockpitButton(Button::Horn), 0.0, 0.1, None);
    let resolved = resolve(&[horn], [at(0.0), None]);
    assert_eq!(
        resolved[0].reaches.len(),
        1,
        "it is in reach, for its marker"
    );
    assert_eq!(resolved[0].chosen, None);
}

#[test]
fn the_losers_say_why_they_lost() {
    let t = [
        sphere(GrabVolume::GrenadePouch, 0.04, 0.2, None),
        sphere(HIP, 0.1, 0.2, None),
        sphere(GrabVolume::Climb, 0.0, 0.2, None),
        sphere(GrabVolume::CockpitButton(Button::Nitrous), 0.0, 0.2, None),
    ];
    let resolved = &resolve(&t, [at(0.0), None])[0];
    let chosen = resolved.chosen.unwrap();
    assert_eq!(chosen.volume, GrabVolume::GrenadePouch);
    let lost = losers(&resolved.reaches, chosen);
    assert_eq!(lost.len(), 3);
    match lost[0].1 {
        Lost::Farther(by) => assert!((by - 0.06).abs() < 1e-6),
        other => panic!("{other:?}"),
    }
    assert_eq!(lost[1].1, Lost::Fallback);
    assert_eq!(lost[2].1, Lost::Touch);
    assert_eq!(choose([]), None);
}

#[test]
fn a_release_docks_in_the_nearest_socket_that_fits() {
    let socket = GrabVolume::Socket(Socket::Shoulder(Hand::Right));
    let t = [
        sphere(GrabVolume::MagazinePouch, 0.0, 0.1, None),
        sphere(socket, 0.08, 0.2, None),
        sphere(HIP, 0.12, 0.2, None),
    ];
    let fits_rifle = |v: GrabVolume| v == socket || v == HIP;
    assert_eq!(
        dock_target(Hand::Right, &t, at(0.0), fits_rifle),
        Some(socket),
        "the pouch is nearer, but a rifle does not fit it"
    );
    assert_eq!(
        dock_target(Hand::Right, &t, at(0.13), fits_rifle),
        Some(HIP)
    );
    assert_eq!(
        dock_target(Hand::Right, &t, at(1.0), fits_rifle),
        None,
        "out of every reach, the hand lets go"
    );
    assert_eq!(dock_target(Hand::Right, &t, at(0.0), |_| false), None);
}

/// One of every kind of target, each from the provider that places it.
fn every_kind() -> Vec<GrabVolume> {
    let mut out = vec![
        HIP,
        GrabVolume::Foregrip,
        GrabVolume::Handle,
        GrabVolume::GrenadePouch,
        GrabVolume::MagazinePouch,
        GrabVolume::WeaponAction,
        GrabVolume::MagazineWell,
        GrabVolume::C4Pouch,
        GrabVolume::ChargeSurface,
        GrabVolume::VehicleControl(ControlKind::Wheel),
        GrabVolume::CockpitButton(Button::Horn),
        GrabVolume::ParachuteHandle(Hand::Left),
        GrabVolume::MountedHandle(Hand::Right),
        GrabVolume::Climb,
        GrabVolume::StuntHold,
        GrabVolume::WorldWeapon,
        GrabVolume::GrenadePin,
        GrabVolume::LiveGrenade,
        GrabVolume::WorldMagazine,
        GrabVolume::Snatch(RemoteKind::Weapon),
        GrabVolume::Snatch(RemoteKind::Object),
        GrabVolume::Snatch(RemoteKind::Door),
    ];
    out.extend(
        [
            PickupKind::Prop,
            PickupKind::Hinged,
            PickupKind::Weapon,
            PickupKind::Door,
            PickupKind::Character,
        ]
        .map(GrabVolume::Pickup),
    );
    // Adding a kind breaks this match until it is listed above.
    for volume in &out {
        match volume {
            GrabVolume::Socket(_)
            | GrabVolume::Foregrip
            | GrabVolume::Handle
            | GrabVolume::GrenadePouch
            | GrabVolume::MagazinePouch
            | GrabVolume::WeaponAction
            | GrabVolume::MagazineWell
            | GrabVolume::C4Pouch
            | GrabVolume::ChargeSurface
            | GrabVolume::VehicleControl(_)
            | GrabVolume::CockpitButton(_)
            | GrabVolume::ParachuteHandle(_)
            | GrabVolume::MountedHandle(_)
            | GrabVolume::Pickup(_)
            | GrabVolume::Climb
            | GrabVolume::StuntHold
            | GrabVolume::WorldWeapon
            | GrabVolume::GrenadePin
            | GrabVolume::LiveGrenade
            | GrabVolume::WorldMagazine
            | GrabVolume::Snatch(_) => {}
        }
    }
    out
}

#[test]
fn every_kind_of_target_goes_through_the_resolver_and_the_renderer() {
    for volume in every_kind() {
        let target = GripTarget::sphere(
            volume,
            None,
            Vec3::ZERO,
            0.1,
            Mark::ring(MarkAt::Cockpit(Vec3::ZERO), 0.05),
        );
        let resolved = resolve(&[target], [at(0.01), None]);
        assert_eq!(resolved[0].reaches.len(), 1, "{volume:?} is in reach");
        match target.tier {
            Tier::Touch => assert_eq!(resolved[0].chosen, None, "{volume:?}"),
            Tier::Target | Tier::Fallback | Tier::Remote => assert_eq!(
                resolved[0].chosen.map(|c| c.volume),
                Some(volume),
                "{volume:?} is taken"
            ),
        }
        let prims = markers::primitives(&[target], &resolved, &test_frame());
        assert!(!prims.is_empty(), "{volume:?} is drawn");
        let [r, g, b] = colour(volume.category());
        for prim in prims {
            let Primitive::Sprite { color, at, .. } = prim else {
                panic!("{volume:?} draws a sprite");
            };
            assert_eq!(
                &color[..3],
                &[r, g, b],
                "{volume:?} takes its kind's colour"
            );
            assert!(matches!(at, Anchor::Head(_)), "{volume:?} rides the head");
        }
        // Every kind names the subsystem that acts on it.
        let _ = volume.consumer();
    }
}

#[test]
fn every_kind_has_its_own_colour() {
    for (i, a) in Category::ALL.iter().enumerate() {
        for b in &Category::ALL[i + 1..] {
            assert_ne!(colour(*a), colour(*b), "{a:?} and {b:?}");
        }
    }
}

#[test]
fn a_marker_shows_as_its_target_says() {
    let frame = test_frame();
    let pouch = sphere(GrabVolume::GrenadePouch, 0.0, 0.1, None);
    let always = GripTarget {
        mark: pouch.mark.showing(Shown::Always),
        ..pouch
    };
    let never = GripTarget {
        mark: pouch.mark.showing(Shown::Never),
        ..pouch
    };
    let away = resolve(&[pouch, always, never], [at(1.0), None]);
    let alpha = |prims: Vec<Primitive>| match prims.as_slice() {
        [Primitive::Sprite { color, .. }] => Some(color[3]),
        [] => None,
        other => panic!("{other:?}"),
    };
    assert_eq!(alpha(markers::primitives(&[pouch], &away, &frame)), None);
    let idle = alpha(markers::primitives(&[always], &away, &frame)).unwrap();
    assert_eq!(alpha(markers::primitives(&[never], &away, &frame)), None);
    let near = resolve(&[always], [at(0.05), None]);
    let chosen = alpha(markers::primitives(&[always], &near, &frame)).unwrap();
    assert!(
        chosen > idle,
        "the chosen target is brighter than an idle one"
    );
    // A marker in the world is anchored there.
    let world = GripTarget {
        mark: Mark::ring(MarkAt::World(Vec3::new(1.0, 2.0, 3.0)), 0.05),
        ..pouch
    };
    let away_world = resolve(&[world], [at(1.0), None]);
    assert!(
        markers::primitives(&[world], &away_world, &frame).is_empty(),
        "shown only in reach"
    );
    let near_world = resolve(&[world], [at(0.0), None]);
    let prims = markers::primitives(&[world], &near_world, &frame);
    assert!(matches!(
        prims[0],
        Primitive::Sprite {
            at: Anchor::World(_),
            ..
        }
    ));
}

const UP: ButtonControl = ButtonControl {
    down: false,
    pressed: false,
    released: false,
};
const PRESS: ButtonControl = ButtonControl {
    down: true,
    pressed: true,
    released: false,
};
const HELD: ButtonControl = ButtonControl {
    down: true,
    pressed: false,
    released: false,
};
const TAP: ButtonControl = ButtonControl {
    down: false,
    pressed: true,
    released: true,
};

const DT: f32 = 1.0 / 30.0;

fn tick(grip: ButtonControl, target: Option<GrabVolume>) -> LatchTick {
    LatchTick {
        grip,
        target,
        bound: false,
        slow: true,
        dt: DT,
    }
}

fn is_fist(latch: GripLatch) -> bool {
    matches!(latch, GripLatch::Fist { .. })
}

#[test]
fn a_press_in_reach_takes_the_target_once_and_holds_it() {
    let mut latch = GripLatch::default();
    assert_eq!(latch.step(tick(PRESS, Some(HIP))), Some(HIP));
    assert_eq!(latch, GripLatch::Grab(HIP));
    // Held, it keeps what it took whatever the hand passes.
    assert_eq!(latch.step(tick(HELD, Some(GrabVolume::GrenadePouch))), None);
    assert_eq!(latch.step(tick(HELD, None)), None);
    assert_eq!(latch, GripLatch::Grab(HIP));
    assert_eq!(latch.step(tick(UP, Some(HIP))), None);
    assert_eq!(latch, GripLatch::Open);
}

#[test]
fn a_fist_takes_what_the_hand_comes_to() {
    let mut latch = GripLatch::default();
    assert_eq!(latch.step(tick(PRESS, None)), None);
    assert!(is_fist(latch));
    assert_eq!(latch.step(tick(HELD, None)), None);
    let handle = GrabVolume::VehicleControl(ControlKind::Wheel);
    assert_eq!(latch.step(tick(HELD, Some(handle))), Some(handle));
    assert_eq!(latch, GripLatch::Grab(handle));
}

#[test]
fn a_fist_kept_closed_and_swung_fast_takes_nothing_it_strikes() {
    let mut latch = GripLatch::default();
    latch.step(tick(PRESS, None));
    // Closed for longer than the window, with nothing in reach.
    for _ in 0..10 {
        latch.step(tick(HELD, None));
    }
    let character = GrabVolume::Pickup(PickupKind::Character);
    let swing = LatchTick {
        slow: false,
        ..tick(HELD, Some(character))
    };
    assert_eq!(latch.step(swing), None);
    assert!(is_fist(latch));
    // Slowed down on it, the fist takes it.
    assert_eq!(latch.step(tick(HELD, Some(character))), Some(character));
}

#[test]
fn a_hand_that_closes_just_before_it_arrives_takes_what_it_arrives_at() {
    let mut latch = GripLatch::default();
    latch.step(tick(PRESS, None));
    let handle = GrabVolume::ParachuteHandle(Hand::Left);
    // Still moving fast, but within the window after closing.
    let arriving = LatchTick {
        slow: false,
        ..tick(HELD, Some(handle))
    };
    assert_eq!(latch.step(arriving), Some(handle));
    const { assert!(super::latch::FIST_WINDOW_S >= 2.0 * DT) };
}

#[test]
fn a_grip_bound_to_a_game_action_keeps_to_it_for_the_press() {
    let mut latch = GripLatch::default();
    let bound = |grip, target| LatchTick {
        bound: true,
        ..tick(grip, target)
    };
    assert_eq!(latch.step(bound(PRESS, None)), None);
    assert_eq!(latch, GripLatch::Default);
    assert_eq!(latch.step(bound(HELD, Some(HIP))), None);
    assert_eq!(latch, GripLatch::Default);
    // A press in reach still takes the target, bound or not.
    latch.step(bound(UP, None));
    assert_eq!(latch.step(bound(PRESS, Some(HIP))), Some(HIP));
}

#[test]
fn a_tap_between_ticks_is_a_fresh_press() {
    let mut latch = GripLatch::default();
    latch.step(tick(PRESS, Some(HIP)));
    // Released and pressed again between two ticks: the new press takes what is in reach now.
    let pouch = GrabVolume::GrenadePouch;
    assert_eq!(latch.step(tick(TAP, Some(pouch))), Some(pouch));
    assert_eq!(latch.step(tick(UP, None)), None);
    assert_eq!(latch, GripLatch::Open);
    assert!(!latch.grabbing());
}
