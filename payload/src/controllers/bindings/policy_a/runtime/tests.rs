#[path = "rereview_tests.rs"]
mod rereview_tests;
#[path = "review_tests.rs"]
mod review_tests;

use std::{sync::Arc, time::Instant};

use tracking_format::input::{ClockDomain, GroupOrder, InputStamp, SourceQuery};

use super::*;
use crate::{controllers::frame::ActionSetKind, vr::input::semantic::vocabulary::OnFootAction};

const LS: &str = "/user/hand/left/input/thumbstick/click";
const RS: &str = "/user/hand/right/input/thumbstick/click";
const RB: &str = "/user/hand/right/input/b/click";
const A: &str = "/user/hand/right/input/a/click";

fn paths(paths: &[&str]) -> SourceQuery {
    SourceQuery::Paths(Arc::from(
        paths
            .iter()
            .map(|path| (*path).to_owned())
            .collect::<Vec<_>>(),
    ))
}

fn group(
    ms: u64,
    sequence: u64,
    down: [bool; 3],
    semantic: &[(ActionId, bool, SourceQuery)],
) -> InputGroup {
    let mut samples = [LS, RS, RB]
        .into_iter()
        .enumerate()
        .map(|(index, path)| RouteSample {
            route: InputRoute::Raw(index as u8),
            value: ValueBits::Bool(down[index]),
            active: true,
            down: down[index],
            change: None,
            sources: paths(&[path]),
        })
        .collect::<Vec<_>>();
    for set in [SemanticSet::Global, SemanticSet::OnFoot] {
        for (action, definition) in set.actions().iter().enumerate() {
            for hand in Hand::BOTH {
                let provided = semantic.iter().find(|(id, _, _)| {
                    id.set == set && id.index() == action && hand == Hand::Right
                });
                let (down, sources) = provided.map_or((false, paths(&[])), |(_, down, sources)| {
                    (*down, sources.clone())
                });
                let value = match definition.kind {
                    ActionKind::Bool => ValueBits::Bool(down),
                    ActionKind::Float => ValueBits::Float(if down { 1.0f32.to_bits() } else { 0 }),
                    ActionKind::Vec2 => ValueBits::Vec2([0; 2]),
                };
                samples.push(RouteSample {
                    route: InputRoute::Semantic {
                        set: set.index() as u8,
                        action: action as u16,
                        hand: hand.index() as u8,
                    },
                    value,
                    active: provided.is_some(),
                    down,
                    change: None,
                    sources,
                });
            }
        }
    }
    InputGroup {
        stamp: InputStamp {
            epoch: 1,
            clock: ClockDomain::Synthetic,
            time_ns: ms * 1_000_000,
            sync: sequence,
            sequence,
        },
        order: GroupOrder::UnorderedSync,
        source_generation: 1,
        binding_generation: 1,
        action_schema: SCHEMA_VERSION,
        context: 0,
        focused: true,
        samples,
    }
}

fn evaluate(runtime: &mut Runtime, group: InputGroup, gameplay: bool) -> Tick {
    let frame = ControllerFrame {
        sequence: group.stamp.sequence,
        display_time_ns: 0,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands: Default::default(),
    };
    runtime.step(TickInput {
        publications: vec![Publication {
            observed_at: Instant::now(),
            frame_sequence: Some(frame.sequence),
            record: InputRecord::Group(group),
        }],
        replay: false,
        frame: Some(&frame),
        context: 0,
        gameplay,
        usable: true,
        config: &BindingsConfig::new(),
    })
}

fn captured_trace(groups: &[InputGroup]) -> Vec<Publication> {
    let _serial = history::TEST_LOCK.lock();
    history::begin_capture();
    let mut runtime = Runtime::default();
    let mut out = Vec::new();
    for group in groups {
        evaluate(&mut runtime, group.clone(), true);
        let policy = history::take_capture()
            .into_iter()
            .filter(|entry| matches!(entry.record, InputRecord::Policy(_)))
            .next_back()
            .expect("one outcome per group");
        out.push(Publication {
            observed_at: Instant::now(),
            frame_sequence: None,
            record: InputRecord::Group(group.clone()),
        });
        out.push(policy);
    }
    out
}

fn batch(runtime: &mut Runtime, groups: &[InputGroup]) -> Tick {
    let sequence = groups
        .last()
        .map_or(runtime.last_frame.unwrap_or(1), |group| {
            group.stamp.sequence
        });
    let frame = ControllerFrame {
        sequence,
        display_time_ns: 0,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands: Default::default(),
    };
    runtime.step(TickInput {
        publications: groups
            .iter()
            .map(|group| Publication {
                observed_at: Instant::now(),
                frame_sequence: Some(group.stamp.sequence),
                record: InputRecord::Group(group.clone()),
            })
            .collect(),
        replay: false,
        frame: Some(&frame),
        context: 0,
        gameplay: true,
        usable: true,
        config: &BindingsConfig::new(),
    })
}

fn replay(runtime: &mut Runtime, publications: Vec<Publication>) -> Tick {
    let frame = ControllerFrame {
        sequence: 1,
        display_time_ns: 0,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands: Default::default(),
    };
    runtime.step(TickInput {
        publications,
        replay: true,
        frame: Some(&frame),
        context: 0,
        gameplay: true,
        usable: true,
        config: &BindingsConfig::new(),
    })
}

fn button(tick: &Tick, id: ActionId) -> ButtonControl {
    button_hand(tick, id, Hand::Right)
}

fn button_hand(tick: &Tick, id: ActionId, hand: Hand) -> ButtonControl {
    tick.routes
        .iter()
        .find(|(route, route_hand, _)| *route == id && *route_hand == hand)
        .map_or_else(ButtonControl::default, |(_, _, intent)| intent.button)
}

#[test]
fn complete_inventory_delivers_the_provided_hand_through_semantic_filter() {
    let id = OnFootAction::Jump.id();
    let mut runtime = Runtime::default();
    evaluate(
        &mut runtime,
        group(1, 1, [false; 3], &[(id, false, paths(&[A]))]),
        true,
    );
    let tick = evaluate(
        &mut runtime,
        group(2, 2, [false; 3], &[(id, true, paths(&[A]))]),
        true,
    );
    assert_eq!(button_hand(&tick, id, Hand::Left), ButtonControl::default());
    assert!(button_hand(&tick, id, Hand::Right).pressed);
    let set = |set: SemanticSet| crate::controllers::bindings::intents::SetInput {
        set,
        actions: set
            .actions()
            .iter()
            .map(|_| crate::controllers::bindings::intents::ActionInput {
                hands: Default::default(),
                bound: Default::default(),
            })
            .collect(),
    };
    let mut input = SemanticInput {
        global: set(SemanticSet::Global),
        mode: set(SemanticSet::OnFoot),
    };
    tick.filter_semantic(&mut input);
    let delivered = input.get(id).unwrap();
    assert_eq!(
        delivered.hands[Hand::Left.index()].button,
        ButtonControl::default()
    );
    assert_eq!(
        delivered.hands[Hand::Right.index()].button,
        button(&tick, id)
    );
}

#[test]
fn unordered_panel_is_consumed_but_ordered_panel_works_while_gameplay_stands_down() {
    let mut runtime = Runtime::default();
    evaluate(&mut runtime, group(1, 1, [false; 3], &[]), false);
    let ambiguous = evaluate(&mut runtime, group(20, 2, [true, false, true], &[]), false);
    assert!(ambiguous.commands.is_empty());
    assert!(ambiguous.raw.iter().all(|button| !button.held()));
    evaluate(&mut runtime, group(30, 3, [false; 3], &[]), false);
    evaluate(&mut runtime, group(40, 4, [true, false, false], &[]), false);
    let formed = evaluate(&mut runtime, group(50, 5, [true, false, true], &[]), false);
    assert_eq!(formed.commands, [SystemCommand::TogglePanel]);
    assert!(formed.raw.iter().all(|button| !button.held()));
}

#[test]
fn raw_click_never_asserts_false_semantics_and_accepted_hold_starts_with_a_press() {
    let id = OnFootAction::Detonate.id();
    let mut runtime = Runtime::default();
    evaluate(
        &mut runtime,
        group(1, 1, [false; 3], &[(id, false, paths(&[RS]))]),
        true,
    );
    evaluate(
        &mut runtime,
        group(10, 2, [false, true, false], &[(id, false, paths(&[RS]))]),
        true,
    );
    assert!(
        !button(
            &evaluate(
                &mut runtime,
                group(120, 3, [false, true, false], &[(id, false, paths(&[RS]))]),
                true
            ),
            id
        )
        .held()
    );
    evaluate(
        &mut runtime,
        group(130, 4, [false; 3], &[(id, false, paths(&[RS]))]),
        true,
    );
    evaluate(
        &mut runtime,
        group(140, 5, [false, true, false], &[(id, true, paths(&[RS]))]),
        true,
    );
    let accepted = evaluate(
        &mut runtime,
        group(250, 6, [false, true, false], &[(id, true, paths(&[RS]))]),
        true,
    );
    assert!(button(&accepted, id).pressed);
    assert!(button(&accepted, id).down);
}

#[test]
fn merged_semantic_requires_its_own_neutral_and_independent_route_survives() {
    let detonate = OnFootAction::Detonate.id();
    let jump = OnFootAction::Jump.id();
    let mut runtime = Runtime::default();
    let sample = |merged, independent| {
        vec![
            (detonate, merged, paths(&[RS, A])),
            (jump, independent, paths(&[A])),
        ]
    };
    evaluate(
        &mut runtime,
        group(1, 1, [false; 3], &sample(false, false)),
        true,
    );
    evaluate(
        &mut runtime,
        group(10, 2, [true, true, false], &sample(true, true)),
        true,
    );
    let consumed = evaluate(
        &mut runtime,
        group(20, 3, [false; 3], &sample(true, true)),
        true,
    );
    assert!(!button(&consumed, detonate).held());
    assert!(button(&consumed, jump).held());
    assert!(
        !button(
            &evaluate(
                &mut runtime,
                group(30, 4, [false; 3], &sample(true, true)),
                true
            ),
            detonate
        )
        .held()
    );
    evaluate(
        &mut runtime,
        group(40, 5, [false; 3], &sample(false, false)),
        true,
    );
    assert!(
        button(
            &evaluate(
                &mut runtime,
                group(50, 6, [false; 3], &sample(true, true)),
                true
            ),
            detonate
        )
        .pressed
    );
}

#[test]
fn failed_and_unknown_queries_quarantine_but_successful_empty_never_uses_defaults() {
    let id = OnFootAction::Detonate.id();
    for source in [
        SourceQuery::Failed,
        SourceQuery::NotEnumerated,
        paths(&["/unknown/device/button"]),
        paths(&[]),
    ] {
        let mut runtime = Runtime::default();
        evaluate(
            &mut runtime,
            group(1, 1, [false; 3], &[(id, false, source.clone())]),
            true,
        );
        let tick = evaluate(
            &mut runtime,
            group(2, 2, [false; 3], &[(id, true, source)]),
            true,
        );
        assert!(!button(&tick, id).held());
    }
}

#[test]
fn direct_command_claims_exact_source_without_canceling_an_unrelated_route() {
    let command = GlobalAction::RecenterView.id();
    let detonate = OnFootAction::Detonate.id();
    let jump = OnFootAction::Jump.id();
    let sample = |down| {
        vec![
            (command, down, paths(&[A])),
            (detonate, down, paths(&[A])),
            (jump, down, paths(&["/user/hand/right/input/trigger/click"])),
        ]
    };
    let mut runtime = Runtime::default();
    evaluate(&mut runtime, group(1, 1, [false; 3], &sample(false)), true);
    let accepted = evaluate(&mut runtime, group(2, 2, [false; 3], &sample(true)), true);
    assert_eq!(accepted.commands, [SystemCommand::Recenter]);
    assert!(!button(&accepted, detonate).held());
    assert!(button(&accepted, jump).held());
    assert!(
        evaluate(&mut runtime, group(3, 3, [false; 3], &sample(true)), true)
            .commands
            .is_empty()
    );
}

#[test]
fn short_semantic_tap_is_an_ordered_press_then_release() {
    let id = OnFootAction::Detonate.id();
    let mut runtime = Runtime::default();
    evaluate(
        &mut runtime,
        group(1, 1, [false; 3], &[(id, false, paths(&[RS]))]),
        true,
    );
    evaluate(
        &mut runtime,
        group(10, 2, [false, true, false], &[(id, true, paths(&[RS]))]),
        true,
    );
    let tap = evaluate(
        &mut runtime,
        group(20, 3, [false; 3], &[(id, false, paths(&[RS]))]),
        true,
    );
    assert_eq!(
        button(&tap, id),
        ButtonControl {
            down: true,
            pressed: true,
            released: false
        }
    );
    let release = evaluate(
        &mut runtime,
        group(30, 4, [false; 3], &[(id, false, paths(&[RS]))]),
        true,
    );
    assert_eq!(
        button(&release, id),
        ButtonControl {
            down: false,
            pressed: false,
            released: true
        }
    );
}

#[test]
fn a_committed_single_is_not_withdrawn_for_a_late_partner() {
    let id = OnFootAction::Detonate.id();
    let mut runtime = Runtime::default();
    evaluate(
        &mut runtime,
        group(1, 1, [false; 3], &[(id, false, paths(&[RS]))]),
        true,
    );
    evaluate(
        &mut runtime,
        group(10, 2, [false, true, false], &[(id, true, paths(&[RS]))]),
        true,
    );
    assert!(
        button(
            &evaluate(
                &mut runtime,
                group(120, 3, [false, true, false], &[(id, true, paths(&[RS]))]),
                true
            ),
            id
        )
        .pressed
    );
    let late = evaluate(
        &mut runtime,
        group(130, 4, [true, true, false], &[(id, true, paths(&[RS]))]),
        true,
    );
    assert!(late.commands.is_empty());
    assert!(button(&late, id).down);
}

#[test]
fn replay_uses_recorded_policy_timing_before_dispatch() {
    let config = BindingsConfig::new();
    let groups = [
        group(1, 1, [false; 3], &[]),
        group(10, 2, [true, false, false], &[]),
        group(20, 3, [true, false, true], &[]),
    ];
    let publications = captured_trace(&groups);
    let mut runtime = Runtime::default();
    let mut changed = config;
    changed.panel_chord_window_s = 0.001;
    let frame = ControllerFrame {
        sequence: 1,
        display_time_ns: 0,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands: Default::default(),
    };
    let tick = runtime.step(TickInput {
        publications,
        replay: true,
        frame: Some(&frame),
        context: 0,
        gameplay: true,
        usable: true,
        config: &changed,
    });
    assert_eq!(tick.commands, [SystemCommand::TogglePanel]);
}

#[test]
fn accepted_transitions_are_delivered_in_order_without_physical_dwell_credit() {
    let mut delivery = Delivery::default();
    delivery.accept(ButtonControl {
        down: true,
        pressed: true,
        released: false,
    });
    delivery.accept(ButtonControl {
        down: false,
        pressed: false,
        released: true,
    });
    delivery.accept(ButtonControl {
        down: true,
        pressed: true,
        released: false,
    });
    assert!(delivery.take().pressed);
    assert!(delivery.take().released);
    assert!(delivery.take().pressed);
    assert_eq!(
        delivery.take(),
        ButtonControl {
            down: true,
            ..Default::default()
        }
    );
    delivery.accept(ButtonControl {
        down: false,
        pressed: true,
        released: true,
    });
    assert_eq!(
        delivery.take(),
        ButtonControl {
            down: true,
            pressed: true,
            released: false
        }
    );
    assert_eq!(
        delivery.take(),
        ButtonControl {
            down: false,
            pressed: false,
            released: true
        }
    );
    assert_eq!(delivery.take(), ButtonControl::default());
}
