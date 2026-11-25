#[path = "boundary_tests.rs"]
mod boundary_tests;
#[path = "derived_tests.rs"]
mod derived_tests;
#[path = "hint_tests.rs"]
mod hint_tests;
#[path = "opaque_tests.rs"]
mod opaque_tests;
#[path = "rereview_tests.rs"]
mod rereview_tests;
#[path = "review_tests.rs"]
mod review_tests;
#[path = "snatch_tests.rs"]
mod snatch_tests;
#[path = "ui_tests.rs"]
mod ui_tests;

use std::{sync::Arc, time::Instant};

use tracking_format::input::{ClockDomain, GroupOrder, InputStamp, SourceQuery};

use super::*;
use crate::{
    controllers::frame::ActionSetKind,
    vr::input::semantic::vocabulary::{LandVehicleAction, OnFootAction},
};

const LS: &str = "/user/hand/left/input/thumbstick/click";
const RS: &str = "/user/hand/right/input/thumbstick/click";
const RB: &str = "/user/hand/right/input/b/click";
const A: &str = "/user/hand/right/input/a/click";
/// A right-hand source outside the arbiter's protected controls.
const RIGHT_SQUEEZE: &str = "/user/hand/right/input/squeeze/click";

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
    down: [bool; CONTROLS],
    semantic: &[(ActionId, bool, SourceQuery)],
) -> InputGroup {
    group_in(ActionSetKind::OnFoot, ms, sequence, down, semantic)
}

/// A group observed while `kind`'s action set is active, carrying that set's semantic routes.
fn group_in(
    kind: ActionSetKind,
    ms: u64,
    sequence: u64,
    down: [bool; CONTROLS],
    semantic: &[(ActionId, bool, SourceQuery)],
) -> InputGroup {
    // The physical A is down whenever a semantic action bound to it is, as it is live.
    let mut down = down;
    down[Control::RightA.index()] |= semantic.iter().any(|(_, pressed, sources)| {
        *pressed && matches!(sources, SourceQuery::Paths(paths) if paths.iter().any(|p| p == A))
    });
    let mut samples = [LS, RS, RB, A]
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
    for set in [SemanticSet::Global, SemanticSet::of(kind)] {
        for (action, definition) in set.actions().iter().enumerate() {
            for hand in Hand::BOTH {
                let provided = semantic.iter().find(|(id, _, sources)| {
                    id.set == set && id.index() == action && hand == source_hand(sources)
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
        context: kind as u64,
        focused: true,
        samples,
    }
}

/// The hand a provided semantic sample is read on: the left when every source is a left-hand
/// path, and the right otherwise.
fn source_hand(sources: &SourceQuery) -> Hand {
    match sources {
        SourceQuery::Paths(paths)
            if !paths.is_empty()
                && paths
                    .iter()
                    .all(|path| path.starts_with("/user/hand/left/")) =>
        {
            Hand::Left
        }
        _ => Hand::Right,
    }
}

fn evaluate(runtime: &mut Runtime, group: InputGroup, gameplay: bool) -> Tick {
    evaluate_in(runtime, group, gameplay, 0, &BindingsConfig::new())
}

/// Evaluates `group` under the policy context `context`, whose action set matches the group's.
fn evaluate_in(
    runtime: &mut Runtime,
    group: InputGroup,
    gameplay: bool,
    context: u64,
    config: &BindingsConfig,
) -> Tick {
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
        context,
        gameplay,
        usable: true,
        config,
        dominant: Hand::Right,
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
            .rfind(|entry| matches!(entry.record, InputRecord::Policy(_)))
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
        dominant: Hand::Right,
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
        dominant: Hand::Right,
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
        group(
            1,
            1,
            [false; CONTROLS],
            &[(id, false, paths(&[RIGHT_SQUEEZE]))],
        ),
        true,
    );
    let tick = evaluate(
        &mut runtime,
        group(
            2,
            2,
            [false; CONTROLS],
            &[(id, true, paths(&[RIGHT_SQUEEZE]))],
        ),
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
    evaluate(&mut runtime, group(1, 1, [false; CONTROLS], &[]), false);
    let ambiguous = evaluate(
        &mut runtime,
        group(20, 2, [true, false, true, false], &[]),
        false,
    );
    assert!(ambiguous.commands.is_empty());
    assert!(ambiguous.raw.iter().all(|button| !button.held()));
    evaluate(&mut runtime, group(30, 3, [false; CONTROLS], &[]), false);
    evaluate(
        &mut runtime,
        group(40, 4, [true, false, false, false], &[]),
        false,
    );
    let formed = evaluate(
        &mut runtime,
        group(50, 5, [true, false, true, false], &[]),
        false,
    );
    assert_eq!(formed.commands, [SystemCommand::TogglePanel]);
    assert!(formed.raw.iter().all(|button| !button.held()));
}

#[test]
fn vehicle_left_click_skips_the_panel_window_only_under_suppression() {
    let vehicle = ActionSetKind::LandVehicle;
    for suppress in [true, false] {
        let config = BindingsConfig {
            suppress_developer_chords_in_vehicles: suppress,
            ..BindingsConfig::new()
        };
        let mut runtime = Runtime::default();
        let mut step = |ms, sequence, down| {
            evaluate_in(
                &mut runtime,
                group_in(vehicle, ms, sequence, down, &[]),
                true,
                vehicle as u64,
                &config,
            )
        };
        step(1, 1, [false; CONTROLS]);
        step(10, 2, [true, false, false, false]);
        let joined = step(50, 3, [true, false, true, false]);
        let after_recenter = step(120, 4, [true, false, true, false]);
        if suppress {
            assert!(joined.commands.is_empty());
            assert!(joined.raw[Control::RightB.index()].pressed);
            assert!(after_recenter.raw[Control::LeftStick.index()].pressed);
        } else {
            assert_eq!(joined.commands, [SystemCommand::TogglePanel]);
            assert!(joined.raw.iter().all(|button| !button.held()));
            assert!(!after_recenter.raw[Control::LeftStick.index()].held());
        }
    }
}

#[test]
fn vehicle_grapple_press_waits_only_for_the_recenter_window_under_suppression() {
    let vehicle = ActionSetKind::LandVehicle;
    let id = LandVehicleAction::Grapple.id();
    for suppress in [true, false] {
        let config = BindingsConfig {
            suppress_developer_chords_in_vehicles: suppress,
            ..BindingsConfig::new()
        };
        let mut runtime = Runtime::default();
        let mut step = |ms, sequence, down: bool| {
            let tick = evaluate_in(
                &mut runtime,
                group_in(
                    vehicle,
                    ms,
                    sequence,
                    [down, false, false, false],
                    &[(id, down, paths(&[LS]))],
                ),
                true,
                vehicle as u64,
                &config,
            );
            button_hand(&tick, id, Hand::Left)
        };
        step(1, 1, false);
        assert!(!step(10, 2, true).held());
        assert!(!step(100, 3, true).held());
        let past_recenter = step(120, 4, true);
        assert_eq!(past_recenter.pressed, suppress, "suppress={suppress}");
        let past_panel = step(370, 5, true);
        assert_eq!(past_panel.pressed, !suppress, "suppress={suppress}");
        assert!(past_panel.down);
    }
}

#[test]
fn on_foot_left_click_keeps_the_panel_chord_under_suppression() {
    let config = BindingsConfig::new();
    assert!(config.suppress_developer_chords_in_vehicles);
    let mut runtime = Runtime::default();
    let on_foot = ActionSetKind::OnFoot as u64;
    evaluate_in(
        &mut runtime,
        group(1, 1, [false; CONTROLS], &[]),
        true,
        on_foot,
        &config,
    );
    let lone = evaluate_in(
        &mut runtime,
        group(10, 2, [true, false, false, false], &[]),
        true,
        on_foot,
        &config,
    );
    assert!(!lone.raw[Control::LeftStick.index()].held());
    let waiting = evaluate_in(
        &mut runtime,
        group(120, 3, [true, false, false, false], &[]),
        true,
        on_foot,
        &config,
    );
    assert!(!waiting.raw[Control::LeftStick.index()].held());
    let formed = evaluate_in(
        &mut runtime,
        group(130, 4, [true, false, true, false], &[]),
        true,
        on_foot,
        &config,
    );
    assert_eq!(formed.commands, [SystemCommand::TogglePanel]);
}

#[test]
fn raw_click_never_asserts_false_semantics_and_accepted_hold_starts_with_a_press() {
    let id = OnFootAction::Detonate.id();
    let mut runtime = Runtime::default();
    evaluate(
        &mut runtime,
        group(1, 1, [false; CONTROLS], &[(id, false, paths(&[RS]))]),
        true,
    );
    evaluate(
        &mut runtime,
        group(
            10,
            2,
            [false, true, false, false],
            &[(id, false, paths(&[RS]))],
        ),
        true,
    );
    assert!(
        !button(
            &evaluate(
                &mut runtime,
                group(
                    120,
                    3,
                    [false, true, false, false],
                    &[(id, false, paths(&[RS]))]
                ),
                true
            ),
            id
        )
        .held()
    );
    evaluate(
        &mut runtime,
        group(130, 4, [false; CONTROLS], &[(id, false, paths(&[RS]))]),
        true,
    );
    evaluate(
        &mut runtime,
        group(
            140,
            5,
            [false, true, false, false],
            &[(id, true, paths(&[RS]))],
        ),
        true,
    );
    let accepted = evaluate(
        &mut runtime,
        group(
            250,
            6,
            [false, true, false, false],
            &[(id, true, paths(&[RS]))],
        ),
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
            (detonate, merged, paths(&[RS, RIGHT_SQUEEZE])),
            (jump, independent, paths(&[RIGHT_SQUEEZE])),
        ]
    };
    evaluate(
        &mut runtime,
        group(1, 1, [false; CONTROLS], &sample(false, false)),
        true,
    );
    evaluate(
        &mut runtime,
        group(10, 2, [true, true, false, false], &sample(true, true)),
        true,
    );
    let consumed = evaluate(
        &mut runtime,
        group(20, 3, [false; CONTROLS], &sample(true, true)),
        true,
    );
    assert!(!button(&consumed, detonate).held());
    assert!(button(&consumed, jump).held());
    assert!(
        !button(
            &evaluate(
                &mut runtime,
                group(30, 4, [false; CONTROLS], &sample(true, true)),
                true
            ),
            detonate
        )
        .held()
    );
    evaluate(
        &mut runtime,
        group(40, 5, [false; CONTROLS], &sample(false, false)),
        true,
    );
    assert!(
        button(
            &evaluate(
                &mut runtime,
                group(50, 6, [false; CONTROLS], &sample(true, true)),
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
        paths(&["/user/hand/right/input/pinch_ext/value"]),
        paths(&[]),
    ] {
        let mut runtime = Runtime::default();
        evaluate(
            &mut runtime,
            group(1, 1, [false; CONTROLS], &[(id, false, source.clone())]),
            true,
        );
        let tick = evaluate(
            &mut runtime,
            group(2, 2, [false; CONTROLS], &[(id, true, source)]),
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
            (command, down, paths(&[RIGHT_SQUEEZE])),
            (detonate, down, paths(&[RIGHT_SQUEEZE])),
            (jump, down, paths(&["/user/hand/right/input/trigger/click"])),
        ]
    };
    let mut runtime = Runtime::default();
    evaluate(
        &mut runtime,
        group(1, 1, [false; CONTROLS], &sample(false)),
        true,
    );
    let accepted = evaluate(
        &mut runtime,
        group(2, 2, [false; CONTROLS], &sample(true)),
        true,
    );
    assert_eq!(accepted.commands, [SystemCommand::Recenter]);
    assert!(!button(&accepted, detonate).held());
    assert!(button(&accepted, jump).held());
    assert!(
        evaluate(
            &mut runtime,
            group(3, 3, [false; CONTROLS], &sample(true)),
            true
        )
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
        group(1, 1, [false; CONTROLS], &[(id, false, paths(&[RS]))]),
        true,
    );
    evaluate(
        &mut runtime,
        group(
            10,
            2,
            [false, true, false, false],
            &[(id, true, paths(&[RS]))],
        ),
        true,
    );
    let tap = evaluate(
        &mut runtime,
        group(20, 3, [false; CONTROLS], &[(id, false, paths(&[RS]))]),
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
        group(30, 4, [false; CONTROLS], &[(id, false, paths(&[RS]))]),
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
        group(1, 1, [false; CONTROLS], &[(id, false, paths(&[RS]))]),
        true,
    );
    evaluate(
        &mut runtime,
        group(
            10,
            2,
            [false, true, false, false],
            &[(id, true, paths(&[RS]))],
        ),
        true,
    );
    assert!(
        button(
            &evaluate(
                &mut runtime,
                group(
                    120,
                    3,
                    [false, true, false, false],
                    &[(id, true, paths(&[RS]))]
                ),
                true
            ),
            id
        )
        .pressed
    );
    let late = evaluate(
        &mut runtime,
        group(
            130,
            4,
            [true, true, false, false],
            &[(id, true, paths(&[RS]))],
        ),
        true,
    );
    assert!(late.commands.is_empty());
    assert!(button(&late, id).down);
}

#[test]
fn replay_uses_recorded_policy_timing_before_dispatch() {
    let config = BindingsConfig::new();
    let groups = [
        group(1, 1, [false; CONTROLS], &[]),
        group(10, 2, [true, false, false, false], &[]),
        group(20, 3, [true, false, true, false], &[]),
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
        dominant: Hand::Right,
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

#[test]
fn the_right_a_jumps_at_once_alone_and_marks_after_the_left_click() {
    let jump = OnFootAction::Jump.id();
    let pressed = |down| [(jump, down, paths(&[A]))];
    let mut runtime = Runtime::default();
    evaluate(
        &mut runtime,
        group(1, 1, [false; CONTROLS], &pressed(false)),
        true,
    );
    evaluate(
        &mut runtime,
        group(2, 2, [false; CONTROLS], &pressed(false)),
        true,
    );
    let alone = evaluate(
        &mut runtime,
        group(10, 3, [false; CONTROLS], &pressed(true)),
        true,
    );
    assert!(
        button(&alone, jump).pressed,
        "the jump lands on the A's own tick"
    );
    assert!(alone.commands.is_empty());
    evaluate(
        &mut runtime,
        group(20, 4, [false; CONTROLS], &pressed(false)),
        true,
    );

    evaluate(
        &mut runtime,
        group(100, 5, [true, false, false, false], &pressed(false)),
        true,
    );
    let chord = evaluate(
        &mut runtime,
        group(150, 6, [true, false, false, false], &pressed(true)),
        true,
    );
    assert_eq!(chord.commands, [SystemCommand::Mark]);
    assert!(!button(&chord, jump).held());
    assert!(!chord.raw[Control::RightA.index()].held());
    assert!(!chord.raw[Control::LeftStick.index()].held());
    // Letting go of the left click alone does not free the A.
    let partial = evaluate(
        &mut runtime,
        group(200, 7, [false; CONTROLS], &pressed(true)),
        true,
    );
    assert!(partial.commands.is_empty());
    assert!(!button(&partial, jump).held());
    // The common neutral frees the members, and the route's own next neutral rearms it.
    for (ms, sequence) in [(250, 8), (260, 9)] {
        evaluate(
            &mut runtime,
            group(ms, sequence, [false; CONTROLS], &pressed(false)),
            true,
        );
    }
    let fresh = evaluate(
        &mut runtime,
        group(300, 10, [false; CONTROLS], &pressed(true)),
        true,
    );
    assert!(button(&fresh, jump).pressed);
}
