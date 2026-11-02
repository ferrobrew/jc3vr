//! Snatch ownership through the production binding tables and native hook gate.

use super::*;
use crate::{
    controllers::{
        bindings::grapple_button::GrappleContext,
        grab::RemoteKind,
        snatch::{ButtonHolds, should_reel_weapon},
    },
    vr::input::semantic::defaults::{Component, DefaultBinding},
};

fn rebound(set: SemanticSet, index: usize) -> Option<DefaultBinding> {
    let mut binding = default_binding(set, index)?;
    if set == SemanticSet::OnFoot && index == OnFootAction::Grapple.id().index as usize {
        binding.component = Component::LowerFace;
    }
    Some(binding)
}

fn release() -> ButtonControl {
    ButtonControl {
        released: true,
        ..Default::default()
    }
}

#[test]
fn ordinary_held_weapon_first_attachment_and_b_first_tether_keep_the_native_hold() {
    let mut bindings = Bindings::new();
    let mut c = controls(Hand::Right, |_, off| off.b = down());
    let owned = bindings.claim_snatch(c.off().b, false, true, false);
    let first = bindings.sim_step(ON_FOOT, &c, DT, &CONFIG);
    let holds = ButtonHolds::routed(first.grapple_held, owned);
    assert!(holds.ordinary);
    assert!(!holds.modifier);
    assert!(!should_reel_weapon(true, holds.ordinary));
    assert!(player(&first, FIRE_GRAPPLE).is_some());

    c.hands[Hand::Left.index()].b.pressed = false;
    c.grapple = GrappleContext {
        line: true,
        aim_target: true,
        ..Default::default()
    };
    let owned = bindings.claim_snatch(c.off().b, true, true, false);
    let attached = bindings.sim_step(ON_FOOT, &c, DT, &CONFIG);
    let holds = ButtonHolds::routed(attached.grapple_held, owned);
    assert!(!owned, "B before grip remains an ordinary episode");
    assert!(holds.ordinary);
    assert!(!holds.modifier);
    assert!(
        !should_reel_weapon(true, holds.ordinary),
        "first attached tick precedes native cached held latch"
    );
    assert!(player(&attached, FIRE_GRAPPLE).is_some());
    c.hands[Hand::Left.index()].b = release();
    let released = bindings.sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert!(!released.grapple_held);
    assert!(should_reel_weapon(
        true,
        ButtonHolds::routed(released.grapple_held, false).ordinary
    ));
}

#[test]
fn route_changes_consume_old_release_then_common_neutral_restores_normal_grapple() {
    for change in 0..3 {
        let mut bindings = Bindings::new();
        let mut c = controls(Hand::Right, |_, off| off.b = down());
        assert!(bindings.claim_snatch(c.off().b, true, true, false));
        assert!(player(&bindings.sim_step(ON_FOOT, &c, DT, &CONFIG), FIRE_GRAPPLE).is_none());
        bindings.reset_preserving_snatch();
        let layout: fn(SemanticSet, usize) -> Option<DefaultBinding> = if change == 1 {
            rebound
        } else {
            default_binding
        };
        if change == 0 {
            c.dominant = Hand::Left;
        }
        c.hands[0].b.pressed = false;
        let new_hand = c.dominant.other().index();
        if change == 1 {
            c.hands[new_hand].a = down();
        } else if change == 0 {
            c.hands[new_hand].b = down();
        }
        let mode = if change == 2 { Mode::Ui } else { ON_FOOT };
        let input = simulate(&c, SemanticSet::of(mode.action_set()), layout);
        let button = crate::controllers::bindings::grapple_button(&input, mode);
        assert!(bindings.claim_snatch(button, false, change != 2, false));
        assert!(player(&bindings.step(mode, &c, &input, DT, &CONFIG), FIRE_GRAPPLE).is_none());
        for hand in &mut c.hands {
            hand.b = release();
            hand.a = release();
        }
        let input = simulate(&c, SemanticSet::OnFoot, layout);
        let button = crate::controllers::bindings::grapple_button(&input, ON_FOOT);
        assert!(bindings.claim_snatch(button, false, true, false));
        let released = bindings.step(ON_FOOT, &c, &input, DT, &CONFIG);
        assert!(player(&released, FIRE_GRAPPLE).is_none());
        assert!(player(&released, RELEASE_GRAPPLE).is_none());
        assert!(player(&released, RETRACT_GRAPPLE).is_none());
        c.hands = Default::default();
        let input = simulate(&c, SemanticSet::OnFoot, layout);
        assert!(!bindings.claim_snatch(ButtonControl::default(), false, true, true));
        bindings.step(ON_FOOT, &c, &input, DT, &CONFIG);
        if change == 1 {
            c.hands[new_hand].a = down();
        } else {
            c.hands[new_hand].b = down();
        }
        let input = simulate(&c, SemanticSet::OnFoot, layout);
        let button = crate::controllers::bindings::grapple_button(&input, ON_FOOT);
        assert!(!bindings.claim_snatch(button, false, true, false));
        assert!(
            player(
                &bindings.step(ON_FOOT, &c, &input, DT, &CONFIG),
                FIRE_GRAPPLE
            )
            .is_some()
        );
    }
}

#[test]
fn a_grip_held_for_the_catch_closes_on_nothing_then_holds_what_it_caught() {
    let mut b = Bindings::default();
    let caught = GrabVolume::Snatch(RemoteKind::Weapon);
    // The dominant hand presses its grip on the chest socket while the snatch's line is out.
    let press = controls(Hand::Right, |dom, _| {
        dom.grip = down();
        dom.grab_volume = Some(GrabVolume::Socket(Socket::Chest));
        dom.catch_reserved = true;
    });
    let step = b.sim_step(ON_FOOT, &press, DT, &CONFIG);
    assert_eq!(step.grabs, [None; 2], "the reserved grip takes nothing");
    assert!(step.desired.is_empty(), "{:?}", step.desired);
    assert!(matches!(
        b.grip_latches()[Hand::Right.index()],
        GripLatch::Fist { .. }
    ));
    // Still closed and slow over the socket: the reserved fist does not take it either.
    let held = controls(Hand::Right, |dom, _| {
        dom.grip = ButtonControl {
            down: true,
            ..Default::default()
        };
        dom.grab_volume = Some(GrabVolume::Socket(Socket::Chest));
        dom.grab_slow = true;
        dom.catch_reserved = true;
    });
    assert_eq!(b.sim_step(ON_FOOT, &held, DT, &CONFIG).grabs, [None; 2]);
    // The hand caught the target: its grip holds it, and the hand takes nothing more it comes to.
    b.hold_catch(Hand::Right, caught);
    assert_eq!(
        b.grip_latches()[Hand::Right.index()],
        GripLatch::Grab(caught)
    );
    let after = controls(Hand::Right, |dom, _| {
        dom.grip = ButtonControl {
            down: true,
            ..Default::default()
        };
        dom.grab_volume = Some(GrabVolume::Socket(Socket::Chest));
        dom.grab_slow = true;
    });
    assert_eq!(b.sim_step(ON_FOOT, &after, DT, &CONFIG).grabs, [None; 2]);
    assert_eq!(
        b.grip_latches()[Hand::Right.index()],
        GripLatch::Grab(caught)
    );
}

#[test]
fn a_catch_never_overrides_a_grip_that_holds_something_else() {
    let mut b = Bindings::default();
    let press = controls(Hand::Right, |dom, _| {
        dom.grip = down();
        dom.grab_volume = Some(GrabVolume::Socket(Socket::Chest));
    });
    b.sim_step(ON_FOOT, &press, DT, &CONFIG);
    b.hold_catch(Hand::Right, GrabVolume::Snatch(RemoteKind::Weapon));
    assert_eq!(
        b.grip_latches()[Hand::Right.index()],
        GripLatch::Grab(GrabVolume::Socket(Socket::Chest))
    );
}
