use tracking_format::{
    Header, StartMarker, decode, encode_input, encode_input_preamble, input::CapturedInput,
};

use super::*;

pub(super) fn captured_wire(publications: Vec<Publication>) -> Vec<CapturedInput> {
    let origin = publications
        .iter()
        .map(|entry| entry.observed_at)
        .min()
        .unwrap();
    let header = Header {
        build: "policy fixture".to_owned(),
        recorded_at: "fixture".to_owned(),
        action_schema: SCHEMA_VERSION,
        start: StartMarker {
            transform: [0.0; 16],
            bookmark: None,
        },
        dominant_hand: tracking_format::model::Hand::Right,
        world_scale: 1.0,
        time_scale: 1.0,
    };
    let mut bytes = encode_input_preamble(&header);
    for publication in publications {
        let captured = CapturedInput {
            capture_time_ns: publication
                .observed_at
                .saturating_duration_since(origin)
                .as_nanos()
                .try_into()
                .unwrap(),
            frame_sequence: publication.frame_sequence,
            record: publication.record,
        };
        bytes.extend(encode_input(&captured).unwrap());
    }
    decode(&bytes).unwrap().input
}

pub(super) fn publish_captured(runtime: &mut Runtime, template: InputGroup) -> Tick {
    publish_configured(runtime, template, &BindingsConfig::new())
}

fn publish_configured(
    runtime: &mut Runtime,
    template: InputGroup,
    config: &BindingsConfig,
) -> Tick {
    history::publish_observation(
        history::LiveSample {
            samples: template.samples,
            context: template.context,
            focused: template.focused,
            generation: template.binding_generation,
            observed_at: history::observation_time(),
            frame_sequence: template.stamp.sequence,
        },
        true,
    );
    let publications = history::take_gameplay();
    let frame = ControllerFrame {
        sequence: template.stamp.sequence,
        display_time_ns: 0,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands: Default::default(),
    };
    runtime.step(TickInput {
        publications,
        frame: Some(&frame),
        context: 0,
        gameplay: true,
        usable: true,
        config,
        dominant: Hand::Right,
    })
}

#[test]
fn capture_transaction_excludes_foreign_policy_publications() {
    let serial = history::TEST_LOCK.lock();
    history::begin_capture();
    let mut runtime = Runtime::default();
    evaluate(&mut runtime, group(1, 1, [false; CONTROLS], &[]), true);
    let mut foreign = match history::take_capture().pop().unwrap().record {
        InputRecord::Policy(policy) => policy,
        _ => unreachable!(),
    };
    foreign.stamp.sequence = 999;
    let (started, started_rx) = std::sync::mpsc::channel();
    let (done, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        started.send(()).unwrap();
        history::record_policy(foreign);
        done.send(()).unwrap();
    });
    started_rx.recv().unwrap();
    evaluate(&mut runtime, group(2, 2, [false; CONTROLS], &[]), true);
    let captured = history::take_capture();
    assert_eq!(captured.len(), 1);
    assert!(
        matches!(&captured[0].record, InputRecord::Policy(policy) if policy.stamp.sequence == 2)
    );
    assert!(matches!(
        done_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    drop(serial);
    done_rx.recv().unwrap();
    worker.join().unwrap();
}

#[test]
fn actual_capture_writes_each_group_before_its_policy() {
    let _serial = history::TEST_LOCK.lock();
    history::take_gameplay();
    history::begin_capture();
    let command = GlobalAction::RecenterView.id();
    let mut runtime = Runtime::default();
    for (index, down) in [false, true, false, true, false].into_iter().enumerate() {
        publish_captured(
            &mut runtime,
            group(
                index as u64 + 1,
                index as u64 + 1,
                [false; CONTROLS],
                &[(command, down, paths(&[RIGHT_SQUEEZE]))],
            ),
        );
    }
    let records = captured_wire(history::take_capture());
    assert_eq!(records.len(), 10);
    for pair in records.as_chunks::<2>().0 {
        assert!(pair[0].capture_time_ns < pair[1].capture_time_ns);
        assert!(matches!(
            (&pair[0].record, &pair[1].record),
            (InputRecord::Group(group), InputRecord::Policy(policy)) if group.stamp == policy.stamp
        ));
    }
}

#[test]
fn direct_trigger_claims_cover_press_release_and_quiet_in_one_edges_window() {
    let command = GlobalAction::RecenterView.id();
    let trigger = "/user/hand/right/input/trigger/click";
    let sample = |down| [(command, down, paths(&[trigger]))];
    let mut runtime = Runtime::default();
    evaluate(
        &mut runtime,
        group(1, 1, [false; CONTROLS], &sample(false)),
        true,
    );
    let tick = batch(
        &mut runtime,
        &[
            group(2, 2, [false; CONTROLS], &sample(true)),
            group(3, 3, [false; CONTROLS], &sample(false)),
            group(4, 4, [false; CONTROLS], &sample(false)),
        ],
    );
    assert_eq!(tick.commands, [SystemCommand::Recenter]);
    assert_eq!(runtime.claims, 0);
    assert_ne!(tick.claims, 0);
    let mut frame = ControllerFrame {
        sequence: 4,
        display_time_ns: 0,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::Ui,
        hands: Default::default(),
    };
    let mut edges = Edges::default();
    edges.pressed[1].set(Button::TriggerPress, true);
    edges.released[1].set(Button::TriggerPress, true);
    tick.filter_raw(&mut frame, &mut edges);
    assert!(!edges.pressed(Hand::Right, Button::TriggerPress));
    assert!(!edges.released(Hand::Right, Button::TriggerPress));
    assert!(!frame.hands[1].buttons.contains(Button::TriggerPress));
    assert_eq!(batch(&mut runtime, &[]).claims, 0);
}

fn protected_group(ms: u64, sequence: u64, down: [bool; CONTROLS], control: Control) -> InputGroup {
    let command = GlobalAction::RecenterView.id();
    let mut group = group(ms, sequence, down, &[]);
    let (hand, _) = raw_location(control);
    let source = [LS, RS, RB, A][control.index()];
    for id in [command, OnFootAction::Jump.id()] {
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
        sample.active = true;
        sample.down = down[control.index()];
        sample.value = ValueBits::Bool(sample.down);
        sample.sources = paths(&[source]);
    }
    group
}

#[test]
fn protected_direct_commands_accept_taps_and_expired_singles_but_not_chord_members() {
    for control in Control::ALL {
        let index = control.index();
        for expire in [false, true] {
            let mut runtime = Runtime::default();
            evaluate(
                &mut runtime,
                protected_group(1, 1, [false; CONTROLS], control),
                true,
            );
            let mut down = [false; CONTROLS];
            down[index] = true;
            let pending = evaluate(&mut runtime, protected_group(10, 2, down, control), true);
            let immediate = matches!(control, Control::RightB | Control::RightA);
            assert_eq!(pending.commands.is_empty(), !immediate);
            let accepted = evaluate(
                &mut runtime,
                protected_group(
                    if expire { 400 } else { 20 },
                    3,
                    if expire { down } else { [false; CONTROLS] },
                    control,
                ),
                true,
            );
            assert_eq!(
                accepted.commands.len(),
                usize::from(!immediate),
                "{control:?}, expiry {expire}"
            );
            assert!(!accepted.raw[index].held());
            assert!(
                !accepted
                    .routes
                    .iter()
                    .any(|(id, _, intent)| *id == OnFootAction::Jump.id() && intent.button.held())
            );
        }
        let mut runtime = Runtime::default();
        evaluate(
            &mut runtime,
            protected_group(1, 1, [false; CONTROLS], control),
            true,
        );
        evaluate(
            &mut runtime,
            protected_group(10, 2, [true, false, false, false], control),
            true,
        );
        let chord = evaluate(
            &mut runtime,
            protected_group(
                20,
                3,
                match control {
                    Control::RightStick => [true, true, false, false],
                    Control::RightA => [true, false, false, true],
                    Control::LeftStick | Control::RightB => [true, false, true, false],
                },
                control,
            ),
            true,
        );
        assert_eq!(
            chord.commands,
            match control {
                Control::RightStick => vec![],
                Control::RightA => vec![SystemCommand::Mark],
                Control::LeftStick | Control::RightB => vec![SystemCommand::TogglePanel],
            }
        );
    }
}

#[test]
fn every_reset_discards_earlier_commands_and_accepted_edges_in_the_drain() {
    let command = GlobalAction::RecenterView.id();
    let action = OnFootAction::Jump.id();
    let sample = |down| {
        [
            (command, down, paths(&[RIGHT_SQUEEZE])),
            (
                action,
                down,
                paths(&["/user/hand/right/input/trigger/click"]),
            ),
        ]
    };
    for damage in 0..4 {
        let mut runtime = Runtime::default();
        evaluate(
            &mut runtime,
            group(1, 1, [false; CONTROLS], &sample(false)),
            true,
        );
        let first = group(2, 2, [false; CONTROLS], &sample(true));
        let mut reset = group(3, 3, [false; CONTROLS], &sample(false));
        if damage == 0 {
            reset.stamp.epoch += 1;
        }
        if damage == 1 {
            reset.source_generation += 1;
        }
        if damage == 2 {
            reset.stamp.clock = ClockDomain::HostMonotonic;
        }
        let frame = ControllerFrame {
            sequence: 3,
            display_time_ns: 0,
            published_at: Instant::now(),
            focused: true,
            action_set: ActionSetKind::OnFoot,
            hands: Default::default(),
        };
        let now = Instant::now();
        let second_observed = if damage == 3 {
            now + MAX_FRAME_AGE + Duration::from_millis(1)
        } else {
            now
        };
        let tick = runtime.step(TickInput {
            publications: vec![
                Publication {
                    observed_at: now,
                    frame_sequence: Some(2),
                    record: InputRecord::Group(first),
                },
                Publication {
                    observed_at: second_observed,
                    frame_sequence: Some(3),
                    record: InputRecord::Group(reset),
                },
            ],
            frame: Some(&frame),
            context: 0,
            gameplay: true,
            usable: true,
            config: &BindingsConfig::new(),
            dominant: Hand::Right,
        });
        assert!(tick.commands.is_empty(), "damage {damage}");
        assert!(!button(&tick, action).held(), "damage {damage}");
        assert!(
            !batch(&mut runtime, &[])
                .routes
                .iter()
                .any(|(_, _, intent)| intent.button.pressed)
        );
    }
}

#[test]
fn neutral_boundary_requires_complete_active_attributable_route_inventory() {
    let command = GlobalAction::RecenterView.id();
    let merged = OnFootAction::Detonate.id();
    let complete = group(
        1,
        1,
        [false; CONTROLS],
        &[
            (command, false, paths(&[RIGHT_SQUEEZE])),
            (merged, false, paths(&[RS, RIGHT_SQUEEZE])),
        ],
    );
    assert!(group_neutral(
        &complete,
        &BindingsConfig::new(),
        Hand::Right
    ));
    for damage in 0..8 {
        let mut candidate = complete.clone();
        let index = candidate
            .samples
            .iter()
            .position(|sample| {
                sample.route
                    == InputRoute::Semantic {
                        set: command.set.index() as u8,
                        action: command.index as u16,
                        hand: 1,
                    }
            })
            .unwrap();
        match damage {
            0 => candidate
                .samples
                .retain(|sample| matches!(sample.route, InputRoute::Raw(_))),
            1 => {
                candidate.samples.remove(index);
            }
            2 => candidate.samples[index].active = false,
            3 => candidate.samples[index].sources = SourceQuery::Failed,
            4 => {
                candidate.samples[index].down = true;
                candidate.samples[index].value = ValueBits::Bool(true);
            }
            5 => candidate.samples[index].sources = paths(&[]),
            6 => candidate.context = 5,
            _ => candidate.samples[index].value = ValueBits::Float(0),
        }
        assert!(
            !group_neutral(&candidate, &BindingsConfig::new(), Hand::Right),
            "damage {damage}"
        );
    }
}

#[test]
fn actual_capture_mid_pending_single_and_chord_waits_for_direct_and_merged_neutral() {
    let _serial = history::TEST_LOCK.lock();
    history::take_gameplay();
    let command = GlobalAction::RecenterView.id();
    let merged = OnFootAction::Detonate.id();
    let sample = |direct, semantic| {
        [
            (command, direct, paths(&[RIGHT_SQUEEZE])),
            (merged, semantic, paths(&[RS, RIGHT_SQUEEZE])),
        ]
    };
    for state in 0..2 {
        let mut runtime = Runtime::default();
        let config = BindingsConfig::new();
        publish_configured(
            &mut runtime,
            group(1, 1, [false; CONTROLS], &sample(false, false)),
            &config,
        );
        publish_configured(
            &mut runtime,
            group(10, 2, [false, true, false, false], &sample(true, true)),
            &config,
        );
        let held = if state == 1 {
            [true, true, false, false]
        } else {
            [false, true, false, false]
        };
        publish_configured(
            &mut runtime,
            group(20, 3, held, &sample(true, true)),
            &config,
        );
        history::begin_capture();
        let serial = runtime.reset_serial;
        publish_configured(
            &mut runtime,
            group(200, 4, held, &sample(true, true)),
            &config,
        );
        let mut capture = history::take_capture();
        assert!(
            matches!(capture.last().map(|entry| &entry.record), Some(InputRecord::Policy(policy)) if policy.outcome == PolicyOutcome::UnavailablePrefix)
        );
        publish_configured(
            &mut runtime,
            group(210, 5, [false; CONTROLS], &sample(true, true)),
            &config,
        );
        capture.extend(history::take_capture());
        assert!(
            matches!(capture.last().map(|entry| &entry.record), Some(InputRecord::Policy(policy)) if policy.outcome == PolicyOutcome::UnavailablePrefix)
        );
        publish_configured(
            &mut runtime,
            group(220, 6, [false; CONTROLS], &sample(false, true)),
            &config,
        );
        capture.extend(history::take_capture());
        assert!(
            matches!(capture.last().map(|entry| &entry.record), Some(InputRecord::Policy(policy)) if policy.outcome == PolicyOutcome::UnavailablePrefix)
        );
        publish_configured(
            &mut runtime,
            group(230, 7, [false; CONTROLS], &sample(false, false)),
            &config,
        );
        capture.extend(history::take_capture());
        assert!(
            matches!(capture.last().map(|entry| &entry.record), Some(InputRecord::Policy(policy)) if policy.outcome == PolicyOutcome::NeutralBoundary)
        );
        publish_configured(
            &mut runtime,
            group(240, 8, [false; CONTROLS], &sample(true, true)),
            &config,
        );
        capture.extend(history::take_capture());
        assert_eq!(runtime.reset_serial, serial);
    }
}
