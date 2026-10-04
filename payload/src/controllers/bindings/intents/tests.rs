use super::*;
use crate::{
    controllers::bindings::controls::{Controls, HandControls},
    vr::input::semantic::{
        defaults::default_binding,
        vocabulary::{MenuAction, OnFootAction},
    },
};

const DOWN: ButtonControl = ButtonControl {
    down: true,
    pressed: true,
    released: false,
};

/// The on-foot input of right-handed raw controls with every button held and both grips squeezed.
fn everything_held() -> SemanticInput {
    let mut c = Controls {
        dominant: Hand::Right,
        ..Default::default()
    };
    for h in &mut c.hands {
        *h = HandControls {
            trigger: 1.0,
            trigger_press: DOWN,
            grip: DOWN,
            a: DOWN,
            b: DOWN,
            stick_click: DOWN,
            ..Default::default()
        };
    }
    simulate::simulate(&c, SemanticSet::OnFoot, default_binding)
}

fn build(input: &SemanticInput, masks: &[Mask], clicks: [ButtonControl; 2]) -> Intents {
    Intents::build(input, clicks, masks, Gestures::default(), Hand::Left)
}

#[test]
fn an_input_mask_rests_only_the_actions_bound_to_it_on_its_hand() {
    let input = everything_held();
    let grip = |hand| Mask {
        hand,
        scope: MaskScope::Input(Physical::Grip),
        exempt: None,
    };
    let fire_grapple = OnFootAction::FireGrapple.id();
    let open = build(&input, &[], [DOWN; 2]);
    assert!(open.button(fire_grapple).held());
    // The grapple is on the left grip: a mask on the right grip leaves it alone.
    let right = build(&input, &[grip(Hand::Right)], [DOWN; 2]);
    assert!(right.button(fire_grapple).held());
    let left = build(&input, &[grip(Hand::Left)], [DOWN; 2]);
    assert!(!left.button(fire_grapple).held());
    // The left hand's other actions are untouched.
    assert!(left.button(OnFootAction::ReleaseTethers.id()).held());
}

#[test]
fn an_exempt_action_keeps_its_masked_input() {
    let input = everything_held();
    let mask = Mask {
        hand: Hand::Left,
        scope: MaskScope::Input(Physical::Trigger),
        exempt: Some(OnFootAction::FireOffHandPistol.id()),
    };
    let intents = build(&input, &[mask], [DOWN; 2]);
    assert!(intents.button(OnFootAction::FireOffHandPistol.id()).held());
    assert!(!intents.button(OnFootAction::RetractTethers.id()).held());
    assert!(intents.button(OnFootAction::Fire.id()).held());
}

#[test]
fn a_hand_mask_rests_every_action_on_the_hand() {
    let input = everything_held();
    let mask = Mask {
        hand: Hand::Right,
        scope: MaskScope::Hand,
        exempt: None,
    };
    let intents = build(&input, &[mask], [DOWN; 2]);
    for action in [OnFootAction::Jump, OnFootAction::Fire, OnFootAction::Use] {
        assert!(!intents.button(action.id()).held(), "{action:?}");
    }
    assert!(intents.button(OnFootAction::Reload.id()).held());
}

#[test]
fn an_unknown_source_is_masked_only_with_its_hand() {
    let mut input = everything_held();
    let jump = OnFootAction::Jump.id();
    input.mode.actions[jump.index()].bound[Hand::Right.index()] = PhysicalSet::EMPTY.with_unknown();
    let face = Mask {
        hand: Hand::Right,
        scope: MaskScope::Input(Physical::LowerFace),
        exempt: None,
    };
    assert!(build(&input, &[face], [DOWN; 2]).button(jump).held());
    let hand = Mask {
        scope: MaskScope::Hand,
        ..face
    };
    assert!(!build(&input, &[hand], [DOWN; 2]).button(jump).held());
}

#[test]
fn a_stick_click_action_reads_the_arbitrated_click() {
    let input = everything_held();
    let withheld = build(&input, &[], [ButtonControl::default(); 2]);
    assert!(!withheld.button(OnFootAction::Detonate.id()).held());
    // Actions on other inputs keep their own state.
    assert!(withheld.button(OnFootAction::Jump.id()).held());
    let passed = build(&input, &[], [ButtonControl::default(), DOWN]);
    assert!(passed.button(OnFootAction::Detonate.id()).held());
}

#[test]
fn the_yank_presses_the_grapple_on_the_grapple_hand() {
    let c = Controls {
        dominant: Hand::Right,
        ..Default::default()
    };
    let input = simulate::simulate(&c, SemanticSet::OnFoot, default_binding);
    let intents = Intents::build(
        &input,
        [ButtonControl::default(); 2],
        &[],
        Gestures { yank: true },
        Hand::Left,
    );
    let grapple = intents.hand(OnFootAction::FireGrapple.id(), Hand::Left);
    assert!(grapple.button.held());
    assert!(!grapple.button.down);
    // An action of a set that is not the tick's reads at rest.
    assert_eq!(
        intents.button(MenuAction::MenuConfirm.id()),
        ButtonControl::default()
    );
}

#[test]
fn the_hands_combine_as_openxr_combines_bindings() {
    let mut c = Controls {
        dominant: Hand::Right,
        ..Default::default()
    };
    c.hands[0].stick = glam::Vec2::new(0.2, 0.0);
    c.hands[1].stick = glam::Vec2::new(0.0, -0.6);
    c.hands[0].a = DOWN;
    let input = simulate::simulate(&c, SemanticSet::Menu, default_binding);
    let intents = build(&input, &[], [ButtonControl::default(); 2]);
    assert_eq!(
        intents.vector(MenuAction::MenuNavigate.id()),
        glam::Vec2::new(0.0, -0.6)
    );
    assert!(intents.button(MenuAction::MenuConfirm.id()).held());
}

#[test]
fn a_stick_axis_has_a_signed_deadzone() {
    assert_eq!(signed_deadzone(0.1, 0.15), 0.0);
    assert_eq!(signed_deadzone(-1.0, 0.15), -1.0);
    assert!((signed_deadzone(-0.575, 0.15) + 0.5).abs() < 1e-5);
}
