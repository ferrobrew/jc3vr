//! Accepted semantic episodes through ordered delivery and the actual gesture tables.

use jc3gi::input::input_action_map::Action;

use super::*;
use crate::{
    controllers::{
        bindings::{
            controls::Controls,
            grapple_button::GrappleContext,
            tables::{Bindings, Step},
        },
        frame::{HandFrame, InteractionProfile, TrackedPose},
        mode::{Mode, OnFootVariant},
    },
    vr::input::semantic::{admission, bound::of_sources},
};

const MODE: Mode = Mode::OnFoot(OnFootVariant::Ground);
const GRAPPLE: ActionId = OnFootAction::Grapple.id();
const PAUSE: ActionId = GlobalAction::Pause.id();

#[derive(Clone, Copy)]
enum Loss {
    Hand,
    Route,
    Missing,
    Rebound,
}

fn source(hand: Hand) -> &'static str {
    match hand {
        Hand::Left => "/user/hand/left/input/b/click",
        Hand::Right => RIGHT_SQUEEZE,
    }
}

fn sample(sequence: u64, routes: &[(ActionId, Hand, bool)]) -> InputGroup {
    let mut group = group(sequence * 10, sequence, [false; CONTROLS], &[]);
    for &(id, hand, down) in routes {
        let route = group
            .samples
            .iter_mut()
            .find(|sample| {
                sample.route
                    == InputRoute::Semantic {
                        set: id.set.index() as u8,
                        action: id.index as u16,
                        hand: hand.index() as u8,
                    }
            })
            .unwrap();
        route.active = true;
        route.sources = paths(&[source(hand)]);
        route.down = down;
        route.value = ValueBits::Bool(down);
    }
    group
}

fn lose(group: &mut InputGroup, id: ActionId, hand: Hand, loss: Loss) {
    let route = InputRoute::Semantic {
        set: id.set.index() as u8,
        action: id.index as u16,
        hand: hand.index() as u8,
    };
    match loss {
        Loss::Hand => {
            let mut hands = std::array::from_fn(|_| HandFrame {
                active: true,
                grip: TrackedPose {
                    position_valid: true,
                    orientation_valid: true,
                    ..Default::default()
                },
                ..Default::default()
            });
            hands[hand.index()].grip.position_valid = false;
            admission::apply(&hands, &mut group.samples);
        }
        Loss::Route => {
            group
                .samples
                .iter_mut()
                .find(|sample| sample.route == route)
                .unwrap()
                .active = false
        }
        Loss::Missing => group.samples.retain(|sample| sample.route != route),
        Loss::Rebound => {
            group
                .samples
                .iter_mut()
                .find(|sample| sample.route == route)
                .unwrap()
                .sources = paths(&[match hand {
                Hand::Left => "/user/hand/left/input/a/click",
                Hand::Right => "/user/hand/right/input/trigger/value",
            }])
        }
    }
}

fn tables(bindings: &mut Bindings, tick: &Tick, context: GrappleContext, dt: f32) -> Step {
    let mut input = SemanticInput::rest(
        SemanticSet::OnFoot,
        [InteractionProfile::ValveIndex; 2],
        Hand::Right,
    );
    for id in [GRAPPLE, PAUSE] {
        input.get_mut(id).unwrap().bound = of_sources(Hand::BOTH.map(source));
    }
    tick.filter_semantic(&mut input);
    bindings.cancel_routed_gestures(tick);
    bindings.step(
        MODE,
        &Controls {
            dominant: Hand::Right,
            grapple: context,
            ..Default::default()
        },
        &input,
        dt,
        &BindingsConfig::new(),
    )
}

fn no_derived(step: &Step) {
    for desired in &step.desired {
        assert!(
            !matches!(
                desired.target.action,
                Action::FIRE_GRAPPLE
                    | Action::RELEASE_GRAPPLE
                    | Action::VEHICLE_RELEASE_GRAPPLE
                    | Action::RETRACT_GRAPPLE
                    | Action::GUI_PAUSE
                    | Action::GUI_PDA
            ),
            "unexpected derived output: {desired:?}"
        );
    }
}

#[test]
fn accepted_face_grapple_and_pause_loss_never_fires_immediate_or_delayed_actions() {
    for id in [GRAPPLE, PAUSE] {
        for hand in Hand::BOTH {
            for loss in [Loss::Hand, Loss::Route, Loss::Missing, Loss::Rebound] {
                for middle in [false, true] {
                    for tethers in [false, true] {
                        let context = GrappleContext {
                            line: !tethers,
                            tethers,
                            ..Default::default()
                        };
                        let mut runtime = Runtime::default();
                        let mut bindings = Bindings::new();
                        for sequence in 1..=2 {
                            let tick =
                                batch(&mut runtime, &[sample(sequence, &[(id, hand, false)])]);
                            no_derived(&tables(&mut bindings, &tick, context, 0.01));
                        }
                        let tick = batch(&mut runtime, &[sample(3, &[(id, hand, true)])]);
                        assert!(
                            button_hand(&tick, id, hand).pressed,
                            "the episode must start with accepted delivery"
                        );
                        no_derived(&tables(&mut bindings, &tick, context, 0.01));
                        let mut lost = sample(4, &[(id, hand, true)]);
                        lose(&mut lost, id, hand, loss);
                        let mut groups = vec![lost];
                        if middle {
                            groups.push(sample(5, &[(id, hand, true)]));
                        }
                        let tick = batch(&mut runtime, &groups);
                        assert!(tick.cancelled(id, hand));
                        no_derived(&tables(&mut bindings, &tick, context, 0.01));
                        for sequence in 6..=15 {
                            let tick =
                                batch(&mut runtime, &[sample(sequence, &[(id, hand, false)])]);
                            no_derived(&tables(&mut bindings, &tick, context, 0.2));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn losing_one_hands_episode_preserves_the_other_hands_different_gesture() {
    for lost_id in [GRAPPLE, PAUSE] {
        for lost in Hand::BOTH {
            let live = lost.other();
            let live_id = if lost_id == GRAPPLE { PAUSE } else { GRAPPLE };
            let context = GrappleContext {
                line: true,
                ..Default::default()
            };
            let mut runtime = Runtime::default();
            let mut bindings = Bindings::new();
            for sequence in 1..=3 {
                let down = sequence == 3;
                let tick = batch(
                    &mut runtime,
                    &[sample(
                        sequence,
                        &[(lost_id, lost, down), (live_id, live, down)],
                    )],
                );
                if down {
                    assert!(button_hand(&tick, lost_id, lost).pressed);
                    assert!(button_hand(&tick, live_id, live).pressed);
                }
                no_derived(&tables(&mut bindings, &tick, context, 0.01));
            }
            let mut loss = sample(4, &[(lost_id, lost, true), (live_id, live, true)]);
            lose(&mut loss, lost_id, lost, Loss::Hand);
            let tick = batch(
                &mut runtime,
                &[
                    loss,
                    sample(5, &[(lost_id, lost, true), (live_id, live, true)]),
                ],
            );
            no_derived(&tables(&mut bindings, &tick, context, 0.01));
            let tick = batch(
                &mut runtime,
                &[sample(6, &[(lost_id, lost, true), (live_id, live, false)])],
            );
            let step = tables(&mut bindings, &tick, context, 0.01);
            let expected = if live_id == PAUSE {
                Action::GUI_PAUSE
            } else {
                Action::FIRE_GRAPPLE
            };
            assert!(
                step.desired
                    .iter()
                    .any(|desired| desired.target.action == expected)
            );
            assert!(
                !step
                    .desired
                    .iter()
                    .any(|desired| desired.target.action == Action::GUI_PDA)
            );
        }
    }
}

#[test]
fn loss_of_nonparticipant_hand_preserves_the_accepted_route_episode() {
    for id in [GRAPPLE, PAUSE] {
        for owner in Hand::BOTH {
            let mut runtime = Runtime::default();
            let mut bindings = Bindings::new();
            let context = GrappleContext {
                line: true,
                ..Default::default()
            };
            for sequence in 1..=3 {
                let tick = batch(
                    &mut runtime,
                    &[sample(sequence, &[(id, owner, sequence == 3)])],
                );
                no_derived(&tables(&mut bindings, &tick, context, 0.01));
            }
            let mut group = sample(4, &[(id, owner, true)]);
            lose(&mut group, id, owner.other(), Loss::Hand);
            let tick = batch(&mut runtime, &[group]);
            assert!(!tick.cancelled(id, owner));
            no_derived(&tables(&mut bindings, &tick, context, 0.01));
            let tick = batch(&mut runtime, &[sample(5, &[(id, owner, false)])]);
            let step = tables(&mut bindings, &tick, context, 0.01);
            let expected = if id == GRAPPLE {
                Action::FIRE_GRAPPLE
            } else {
                Action::GUI_PAUSE
            };
            assert!(
                step.desired
                    .iter()
                    .any(|desired| desired.target.action == expected)
            );
        }
    }
}

#[test]
fn route_loss_cancels_pending_tap_and_second_press_without_delayed_fire_or_release() {
    for second_press in [false, true] {
        for cancel in [false, true] {
            let mut runtime = Runtime::default();
            let mut bindings = Bindings::new();
            let context = GrappleContext {
                tethers: true,
                ..Default::default()
            };
            for sequence in 1..=4 {
                let tick = batch(
                    &mut runtime,
                    &[sample(sequence, &[(GRAPPLE, Hand::Left, sequence == 3)])],
                );
                if sequence == 3 {
                    assert!(button_hand(&tick, GRAPPLE, Hand::Left).pressed);
                }
                no_derived(&tables(&mut bindings, &tick, context, 0.01));
            }
            let mut sequence = 5;
            if second_press {
                let tick = batch(
                    &mut runtime,
                    &[sample(sequence, &[(GRAPPLE, Hand::Left, true)])],
                );
                assert!(button_hand(&tick, GRAPPLE, Hand::Left).pressed);
                no_derived(&tables(&mut bindings, &tick, context, 0.01));
                sequence += 1;
            }
            let mut group = sample(sequence, &[(GRAPPLE, Hand::Left, false)]);
            if cancel {
                lose(&mut group, GRAPPLE, Hand::Left, Loss::Route);
            }
            let tick = batch(&mut runtime, &[group]);
            let step = tables(&mut bindings, &tick, context, 0.5);
            if cancel {
                no_derived(&step);
            } else {
                let expected = if second_press {
                    Action::RELEASE_GRAPPLE
                } else {
                    Action::FIRE_GRAPPLE
                };
                assert!(
                    step.desired
                        .iter()
                        .any(|desired| desired.target.action == expected)
                );
            }
            for later in sequence + 1..sequence + 5 {
                let tick = batch(
                    &mut runtime,
                    &[sample(later, &[(GRAPPLE, Hand::Left, false)])],
                );
                no_derived(&tables(&mut bindings, &tick, context, 0.5));
            }
        }
    }
}

#[test]
fn route_cancellation_does_not_transfer_snatch_owned_press_before_real_neutral() {
    let mut runtime = Runtime::default();
    let mut bindings = Bindings::new();
    let context = GrappleContext {
        tethers: true,
        ..Default::default()
    };
    for sequence in 1..=2 {
        let tick = batch(
            &mut runtime,
            &[sample(sequence, &[(GRAPPLE, Hand::Left, false)])],
        );
        no_derived(&tables(&mut bindings, &tick, context, 0.01));
    }
    let tick = batch(&mut runtime, &[sample(3, &[(GRAPPLE, Hand::Left, true)])]);
    let press = button_hand(&tick, GRAPPLE, Hand::Left);
    assert!(press.pressed);
    assert!(bindings.claim_snatch(press, true, true, false));
    no_derived(&tables(&mut bindings, &tick, context, 0.01));
    let mut group = sample(4, &[(GRAPPLE, Hand::Left, true)]);
    lose(&mut group, GRAPPLE, Hand::Left, Loss::Route);
    let tick = batch(&mut runtime, &[group]);
    no_derived(&tables(&mut bindings, &tick, context, 1.0));
    assert!(bindings.claim_snatch(ButtonControl::default(), false, false, false));
    for sequence in 5..=6 {
        let down = sequence == 5;
        let tick = batch(
            &mut runtime,
            &[sample(sequence, &[(GRAPPLE, Hand::Left, down)])],
        );
        assert!(bindings.claim_snatch(
            ButtonControl {
                down,
                pressed: false,
                released: !down
            },
            false,
            true,
            !down
        ));
        no_derived(&tables(&mut bindings, &tick, context, 1.0));
    }
    assert!(!bindings.claim_snatch(ButtonControl::default(), false, true, true));
}

#[test]
fn pause_loss_before_long_threshold_cannot_open_pda_and_real_hold_still_does() {
    for loss in [false, true] {
        let mut runtime = Runtime::default();
        let mut bindings = Bindings::new();
        let context = GrappleContext::default();
        for sequence in 1..=3 {
            let tick = batch(
                &mut runtime,
                &[sample(sequence, &[(PAUSE, Hand::Left, sequence == 3)])],
            );
            no_derived(&tables(&mut bindings, &tick, context, 0.01));
        }
        let mut group = sample(4, &[(PAUSE, Hand::Left, true)]);
        if loss {
            lose(&mut group, PAUSE, Hand::Left, Loss::Route);
        }
        let tick = batch(&mut runtime, &[group]);
        let step = tables(
            &mut bindings,
            &tick,
            context,
            BindingsConfig::new().long_press_s + 0.1,
        );
        if loss {
            no_derived(&step);
        } else {
            assert!(
                step.desired
                    .iter()
                    .any(|desired| desired.target.action == Action::GUI_PDA)
            );
        }
    }
}
