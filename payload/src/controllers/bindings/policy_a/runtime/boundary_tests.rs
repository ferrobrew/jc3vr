//! Sampler admission through ordered arbitration and semantic table execution.

use jc3gi::input::input_action_map::Action;

use super::*;
use crate::{
    controllers::{
        bindings::{
            controls::Controls,
            tables::Bindings,
            trigger_ownership::{TriggerMask, mask_routes},
        },
        config::ControllersConfig,
        frame::{HandFrame, InteractionProfile, TrackedPose},
        mode::{AirborneVariant, Mode, OnFootVariant},
    },
    vr::input::semantic::{
        admission,
        bound::of_sources,
        frame::{ActionValue, digital},
        vocabulary::WingsuitAction,
    },
};

fn hands() -> [HandFrame; 2] {
    std::array::from_fn(|_| HandFrame {
        active: true,
        grip: TrackedPose {
            position_valid: true,
            orientation_valid: true,
            ..Default::default()
        },
        ..Default::default()
    })
}

fn sampled(sequence: u64, id: ActionId, down: bool, hand: Hand) -> InputGroup {
    let path = match hand {
        Hand::Left => "/user/hand/left/input/a/click",
        Hand::Right => RIGHT_SQUEEZE,
    };
    let mut group = group(sequence * 10, sequence, [false; CONTROLS], &[]);
    let sample = group
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
    sample.sources = paths(&[path]);
    sample.active = true;
    sample.down = down;
    sample.value = ValueBits::Bool(down);
    group
}

#[test]
fn either_absent_hand_leaves_the_other_route_live_and_loss_requires_real_neutral() {
    let id = OnFootAction::Jump.id();
    for lost in Hand::BOTH {
        for absent in [false, true] {
            let live = lost.other();
            let mut runtime = Runtime::default();
            let mut both = hands();
            for sequence in 1..=2 {
                let mut group = sampled(sequence, id, false, lost);
                let other = sampled(sequence, id, false, live);
                group.samples.iter_mut().filter(|sample| matches!(sample.route, InputRoute::Semantic { hand, .. } if hand == live.index() as u8))
                    .zip(other.samples.iter().filter(|sample| matches!(sample.route, InputRoute::Semantic { hand, .. } if hand == live.index() as u8)))
                    .for_each(|(sample, other)| *sample = other.clone());
                admission::apply(&both, &mut group.samples);
                evaluate(&mut runtime, group, true);
            }
            let mut held = sampled(3, id, true, lost);
            let live_neutral = sampled(3, id, false, live);
            for sample in &mut held.samples {
                if matches!(sample.route, InputRoute::Semantic { hand, .. } if hand == live.index() as u8)
                {
                    *sample = live_neutral
                        .samples
                        .iter()
                        .find(|other| other.route == sample.route)
                        .unwrap()
                        .clone();
                }
            }
            admission::apply(&both, &mut held.samples);
            assert!(button_hand(&evaluate(&mut runtime, held, true), id, lost).down);
            both[lost.index()].active = !absent;
            both[lost.index()].grip.position_valid = false;
            let mut loss = sampled(4, id, true, live);
            admission::apply(&both, &mut loss.samples);
            let tick = evaluate(&mut runtime, loss, true);
            assert!(!button_hand(&tick, id, lost).held());
            assert!(button_hand(&tick, id, live).pressed);
            both = hands();
            for sequence in 5..=6 {
                let mut recovered = sampled(sequence, id, true, lost);
                admission::apply(&both, &mut recovered.samples);
                assert!(!button_hand(&evaluate(&mut runtime, recovered, true), id, lost).held());
            }
            evaluate(&mut runtime, sampled(7, id, false, lost), true);
            assert!(
                button_hand(
                    &evaluate(&mut runtime, sampled(8, id, true, lost), true),
                    id,
                    lost
                )
                .pressed
            );
        }
    }
}

#[test]
fn startup_with_either_hand_absent_accepts_the_live_hands_neutral_then_press() {
    for absent in Hand::BOTH {
        let live = absent.other();
        let id = OnFootAction::Jump.id();
        let mut runtime = Runtime::default();
        let mut hands = hands();
        hands[absent.index()] = HandFrame::default();
        for sequence in 1..=3 {
            let mut group = sampled(sequence, id, sequence == 3, live);
            admission::apply(&hands, &mut group.samples);
            let tick = evaluate(&mut runtime, group, true);
            if sequence == 3 {
                assert!(button_hand(&tick, id, live).pressed);
                assert!(!button_hand(&tick, id, absent).held());
            }
        }
    }
}

#[test]
fn missing_reserved_raw_source_does_not_disable_unrelated_semantic_route() {
    let id = OnFootAction::Jump.id();
    for control in Control::ALL {
        let mut runtime = Runtime::default();
        for sequence in 1..=3 {
            let mut group = sampled(sequence, id, sequence == 3, Hand::Right);
            group.samples[control.index()].active = false;
            let tick = evaluate(&mut runtime, group, true);
            if sequence == 3 {
                assert!(button(&tick, id).pressed);
                assert!(!tick.raw[control.index()].held());
            }
        }
    }
}

fn chord_loss_trace() -> Vec<InputGroup> {
    let mut trace = vec![
        group(1, 1, [false; CONTROLS], &[]),
        group(10, 2, [true, false, false, false], &[]),
        group(20, 3, [true, false, false, false], &[]),
        group(30, 4, [true, false, true, false], &[]),
        group(40, 5, [true, false, true, false], &[]),
        group(50, 6, [false; CONTROLS], &[]),
        group(60, 7, [true, false, false, false], &[]),
        group(70, 8, [true, false, true, false], &[]),
    ];
    for (index, group) in trace.iter_mut().enumerate() {
        let mut hands = hands();
        if index == 2 {
            hands[Hand::Right.index()].grip.position_valid = false;
        }
        admission::apply(&hands, &mut group.samples);
    }
    trace
}

#[test]
fn pending_chord_middle_loss_is_retained_even_when_latest_pose_recovers() {
    let trace = chord_loss_trace();
    for split in 1..=5 {
        let mut runtime = Runtime::default();
        assert!(batch(&mut runtime, &trace[..split]).commands.is_empty());
        assert!(batch(&mut runtime, &trace[split..5]).commands.is_empty());
        assert_eq!(
            batch(&mut runtime, &trace[5..]).commands,
            [SystemCommand::TogglePanel]
        );
    }
}

#[test]
fn pending_chord_member_loss_cannot_commit_a_delayed_single_or_held_command() {
    let mut runtime = Runtime::default();
    evaluate(&mut runtime, group(1, 1, [false; CONTROLS], &[]), true);
    evaluate(
        &mut runtime,
        group(10, 2, [true, false, false, false], &[]),
        true,
    );
    let mut loss = group(20, 3, [true, false, false, false], &[]);
    loss.samples[2].active = false;
    assert!(evaluate(&mut runtime, loss, true).commands.is_empty());
    for (ms, sequence, down) in [
        (200, 4, [true, false, false, false]),
        (210, 5, [false; CONTROLS]),
    ] {
        let tick = evaluate(&mut runtime, group(ms, sequence, down, &[]), true);
        assert!(tick.commands.is_empty());
        assert!(!tick.raw[0].held());
    }
    evaluate(&mut runtime, group(220, 6, [false; CONTROLS], &[]), true);
    evaluate(
        &mut runtime,
        group(230, 7, [true, false, false, false], &[]),
        true,
    );
    assert_eq!(
        evaluate(
            &mut runtime,
            group(240, 8, [true, false, true, false], &[]),
            true
        )
        .commands,
        [SystemCommand::TogglePanel]
    );
}

#[test]
fn unavailable_unused_raw_control_does_not_cancel_a_disjoint_chord() {
    let mut runtime = Runtime::default();
    evaluate(&mut runtime, group(1, 1, [false; CONTROLS], &[]), true);
    let mut start = group(10, 2, [true, false, false, false], &[]);
    start.samples[1].active = false;
    evaluate(&mut runtime, start, true);
    let mut joined = group(20, 3, [true, false, true, false], &[]);
    joined.samples[1].active = false;
    assert_eq!(
        evaluate(&mut runtime, joined, true).commands,
        [SystemCommand::TogglePanel]
    );
}

#[test]
fn captured_loss_round_trips_and_replay_matches_live_commands() {
    let trace = chord_loss_trace();
    let publications = captured_trace(&trace);
    let records = rereview_tests::captured_wire(publications);
    assert!(records.iter().any(|record| matches!(&record.record, InputRecord::Group(group) if group.stamp.sequence == 3 && !group.samples[2].active)));
    let mut runtime = Runtime::default();
    let mut commands = Vec::new();
    for pair in records.chunks(2) {
        commands.extend(
            replay(
                &mut runtime,
                pair.iter()
                    .map(|captured| Publication {
                        observed_at: Instant::now(),
                        frame_sequence: None,
                        record: captured.record.clone(),
                    })
                    .collect(),
            )
            .commands,
        );
    }
    assert_eq!(commands, [SystemCommand::TogglePanel]);
}

#[test]
fn queued_live_publications_keep_middle_loss_through_capture_and_replay() {
    let _flight = crate::vr::input::flight_pose::test_guard();
    let _serial = history::TEST_LOCK.lock();
    history::end_replay();
    history::take_gameplay();
    history::begin_capture();
    let trace = chord_loss_trace();
    let mut runtime = Runtime::default();
    for group in &trace[..5] {
        history::publish_observation(
            history::LiveSample {
                samples: group.samples.clone(),
                context: group.context,
                focused: group.focused,
                generation: group.binding_generation,
                observed_at: history::observation_time(),
                frame_sequence: group.stamp.sequence,
            },
            true,
        );
    }
    let frame = ControllerFrame {
        sequence: 5,
        display_time_ns: 0,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands: hands(),
    };
    let (_, publications) = history::take_gameplay();
    assert_eq!(publications.len(), 5);
    let tick = runtime.step(TickInput {
        publications,
        replay: false,
        frame: Some(&frame),
        context: 0,
        gameplay: true,
        usable: true,
        config: &BindingsConfig::new(),
    });
    assert!(tick.commands.is_empty());
    for group in trace[5..].iter().cloned() {
        let tick = rereview_tests::publish_captured(&mut runtime, group);
        if runtime.last_frame == Some(8) {
            assert_eq!(tick.commands, [SystemCommand::TogglePanel]);
        } else {
            assert!(tick.commands.is_empty());
        }
    }
    let records = rereview_tests::captured_wire(history::take_capture());
    assert!(records.iter().any(|record| matches!(&record.record, InputRecord::Group(group) if group.stamp.sequence == 3 && !group.samples[2].active)));
    let mut replayed = Runtime::default();
    let publications = records
        .into_iter()
        .map(|captured| Publication {
            observed_at: Instant::now(),
            frame_sequence: None,
            record: captured.record,
        })
        .collect();
    assert_eq!(
        replay(&mut replayed, publications).commands,
        [SystemCommand::TogglePanel]
    );
}

#[test]
fn analog_grapple_without_default_uses_sampler_hysteresis_through_policy_and_tables() {
    for (id, mode) in [
        (
            OnFootAction::FireGrapple.id(),
            Mode::OnFoot(OnFootVariant::Ground),
        ),
        (
            WingsuitAction::FireGrapple.id(),
            Mode::Airborne(AirborneVariant::Wingsuit),
        ),
    ] {
        assert!(default_binding(id.set, id.index()).is_none());
        let mut runtime = Runtime::default();
        let mut bindings = Bindings::new();
        let rules = ControllersConfig::new()
            .input
            .press_rules(InteractionProfile::ValveIndex, Hand::Right);
        let controls = Controls {
            dominant: Hand::Right,
            ..Default::default()
        };
        let mut was_down = false;
        let mut results = Vec::new();
        for (index, value) in [
            0.0,
            0.0,
            1.0,
            (rules.trigger.press + rules.trigger.release) * 0.5,
            0.0,
        ]
        .into_iter()
        .enumerate()
        {
            let down = digital(
                id.set,
                id.index(),
                ActionValue::Float(value),
                &rules,
                was_down,
            );
            was_down = down;
            let mut group = group(
                (index as u64 + 1) * 10,
                index as u64 + 1,
                [false; CONTROLS],
                &[],
            );
            if id.set == SemanticSet::Wingsuit {
                group.samples.retain(|sample| !matches!(sample.route, InputRoute::Semantic { set, .. } if set == SemanticSet::OnFoot.index() as u8));
                for (action, definition) in id.set.actions().iter().enumerate() {
                    for hand in Hand::BOTH {
                        group.samples.push(RouteSample {
                            route: InputRoute::Semantic {
                                set: id.set.index() as u8,
                                action: action as u16,
                                hand: hand.index() as u8,
                            },
                            value: match definition.kind {
                                ActionKind::Bool => ValueBits::Bool(false),
                                ActionKind::Float => ValueBits::Float(0),
                                ActionKind::Vec2 => ValueBits::Vec2([0; 2]),
                            },
                            active: false,
                            down: false,
                            change: None,
                            sources: paths(&[]),
                        });
                    }
                }
                group.context = mode.action_set().index() as u64;
            }
            let sample = group
                .samples
                .iter_mut()
                .find(|sample| {
                    sample.route
                        == InputRoute::Semantic {
                            set: id.set.index() as u8,
                            action: id.index as u16,
                            hand: 1,
                        }
                })
                .unwrap();
            let source = "/user/hand/right/input/trigger/value";
            sample.sources = paths(&[source]);
            sample.value = ValueBits::Float(value.to_bits());
            sample.active = true;
            sample.down = down;
            admission::apply(&hands(), &mut group.samples);
            let frame = ControllerFrame {
                sequence: group.stamp.sequence,
                display_time_ns: 0,
                published_at: Instant::now(),
                focused: true,
                action_set: mode.action_set(),
                hands: hands(),
            };
            let tick = runtime.step(TickInput {
                publications: vec![Publication {
                    observed_at: Instant::now(),
                    frame_sequence: Some(frame.sequence),
                    record: InputRecord::Group(group),
                }],
                replay: false,
                frame: Some(&frame),
                context: mode.action_set().index() as u64,
                gameplay: true,
                usable: true,
                config: &BindingsConfig::new(),
            });
            let mut input =
                SemanticInput::rest(id.set, [InteractionProfile::ValveIndex; 2], Hand::Right);
            input.get_mut(id).unwrap().bound = of_sources([source]);
            tick.filter_semantic(&mut input);
            let button = input.get(id).unwrap().hands[1].button;
            results.push(button);
            if index == 2 {
                let mut reserved = input.clone();
                mask_routes(
                    &mut reserved,
                    mode,
                    [
                        TriggerMask::default(),
                        TriggerMask {
                            reserved: true,
                            ..Default::default()
                        },
                    ],
                );
                assert!(!reserved.get(id).unwrap().hands[1].button.held());
            }
            let step = bindings.step(mode, &controls, &input, 0.01, &BindingsConfig::new());
            assert_eq!(step.grapple_held, down);
            if index == 2 || index == 3 {
                assert!(
                    step.desired
                        .iter()
                        .any(|desired| desired.target.action == Action::FIRE_GRAPPLE)
                );
            }
        }
        assert!(results[2].pressed && results[2].down);
        assert!(results[3].down && !results[3].pressed);
        assert!(results[4].released && !results[4].down);
    }
}
