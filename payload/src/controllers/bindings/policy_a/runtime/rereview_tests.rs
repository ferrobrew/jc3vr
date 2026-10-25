use tracking_format::{
    Header, StartMarker, decode, encode_input, encode_input_preamble,
    input::{
        CapturedInput,
        replay::{InputReplay, PreparedInput},
    },
};

use super::*;

fn captured_wire(publications: Vec<Publication>) -> Vec<CapturedInput> {
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

fn publish_captured(runtime: &mut Runtime, template: InputGroup) -> Tick {
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
    let (replay, publications) = history::take_gameplay();
    assert!(!replay);
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
        replay: false,
        frame: Some(&frame),
        context: 0,
        gameplay: true,
        usable: true,
        config,
    })
}

fn scheduled(records: &[CapturedInput], deadlines: &[u64]) -> Vec<SystemCommand> {
    history::begin_replay(true);
    let prepared = PreparedInput::new(records);
    let mut cursor = InputReplay::default();
    let mut runtime = Runtime::default();
    let mut commands = Vec::new();
    for &deadline in deadlines {
        let range = cursor.advance(&prepared, deadline);
        history::publish_replayed(&records[range]);
        let (is_replay, publications) = history::take_gameplay();
        assert!(is_replay);
        commands.extend(replay(&mut runtime, publications).commands);
    }
    assert!(cursor.finished(records.len()));
    history::end_replay();
    commands
}

#[test]
fn capture_transaction_excludes_foreign_policy_publications() {
    let serial = history::TEST_LOCK.lock();
    history::begin_capture();
    let mut runtime = Runtime::default();
    evaluate(&mut runtime, group(1, 1, [false; 3], &[]), true);
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
    evaluate(&mut runtime, group(2, 2, [false; 3], &[]), true);
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
fn actual_capture_deadlines_between_group_and_policy_preserve_complete_units() {
    let _serial = history::TEST_LOCK.lock();
    history::end_replay();
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
                [false; 3],
                &[(command, down, paths(&[A]))],
            ),
        );
    }
    let records = captured_wire(history::take_capture());
    assert_eq!(records.len(), 10);
    let prepared = PreparedInput::new(&records);
    for pair in records.as_chunks::<2>().0 {
        assert!(pair[0].capture_time_ns < pair[1].capture_time_ns);
        let mut cursor = InputReplay::default();
        let before = cursor.advance(&prepared, pair[1].capture_time_ns - 1);
        assert!(!records[before].iter().any(|entry| matches!(&entry.record, InputRecord::Group(group) if matches!(&pair[0].record, InputRecord::Group(expected) if group.stamp == expected.stamp))));
    }
    let mut deadlines = records
        .iter()
        .flat_map(|entry| {
            [
                entry.capture_time_ns.saturating_sub(1),
                entry.capture_time_ns,
            ]
        })
        .collect::<Vec<_>>();
    deadlines.sort_unstable();
    deadlines.dedup();
    deadlines.push(u64::MAX);
    assert_eq!(
        scheduled(&records, &deadlines),
        [SystemCommand::Recenter; 2]
    );
    for split in 0..records.len() {
        assert_eq!(
            scheduled(&records, &[records[split].capture_time_ns, u64::MAX]),
            [SystemCommand::Recenter; 2],
            "split {split}"
        );
    }
}

#[test]
fn direct_trigger_claims_cover_press_release_and_quiet_in_one_edges_window() {
    let command = GlobalAction::RecenterView.id();
    let trigger = "/user/hand/right/input/trigger/click";
    let sample = |down| [(command, down, paths(&[trigger]))];
    let mut runtime = Runtime::default();
    evaluate(&mut runtime, group(1, 1, [false; 3], &sample(false)), true);
    let tick = batch(
        &mut runtime,
        &[
            group(2, 2, [false; 3], &sample(true)),
            group(3, 3, [false; 3], &sample(false)),
            group(4, 4, [false; 3], &sample(false)),
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

fn protected_group(ms: u64, sequence: u64, down: [bool; 3], control: Control) -> InputGroup {
    let command = GlobalAction::RecenterView.id();
    let mut group = group(ms, sequence, down, &[]);
    let (hand, _) = raw_location(control);
    let source = [LS, RS, RB][control.index()];
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
                protected_group(1, 1, [false; 3], control),
                true,
            );
            let mut down = [false; 3];
            down[index] = true;
            let pending = evaluate(&mut runtime, protected_group(10, 2, down, control), true);
            assert_eq!(pending.commands.is_empty(), control != Control::RightB);
            let accepted = evaluate(
                &mut runtime,
                protected_group(
                    if expire { 400 } else { 20 },
                    3,
                    if expire { down } else { [false; 3] },
                    control,
                ),
                true,
            );
            assert_eq!(
                accepted.commands.len(),
                usize::from(control != Control::RightB),
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
            protected_group(1, 1, [false; 3], control),
            true,
        );
        evaluate(
            &mut runtime,
            protected_group(10, 2, [true, false, false], control),
            true,
        );
        let chord = evaluate(
            &mut runtime,
            protected_group(
                20,
                3,
                if control == Control::RightStick {
                    [true, true, false]
                } else {
                    [true, false, true]
                },
                control,
            ),
            true,
        );
        assert_eq!(
            chord.commands,
            if control == Control::RightStick {
                vec![]
            } else {
                vec![SystemCommand::TogglePanel]
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
            (command, down, paths(&[A])),
            (
                action,
                down,
                paths(&["/user/hand/right/input/trigger/click"]),
            ),
        ]
    };
    for damage in 0..4 {
        let mut runtime = Runtime::default();
        evaluate(&mut runtime, group(1, 1, [false; 3], &sample(false)), true);
        let first = group(2, 2, [false; 3], &sample(true));
        let mut reset = group(3, 3, [false; 3], &sample(false));
        if damage == 0 {
            reset.stamp.epoch += 1;
        }
        if damage == 1 {
            reset.samples[0].active = false;
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
            replay: false,
            frame: Some(&frame),
            context: 0,
            gameplay: true,
            usable: true,
            config: &BindingsConfig::new(),
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
fn replay_neutral_recovery_retains_full_sync_and_group_order_watermark() {
    let trace = [group(1, 1, [false; 3], &[]), group(2, 2, [false; 3], &[])];
    let records = captured_trace(&trace);
    for damage in 0..3 {
        let mut runtime = Runtime::default();
        replay(&mut runtime, records[..2].to_vec());
        assert!(runtime.replay_ready);
        let watermark = runtime.watermark;
        runtime.reset(ResetReason::InvalidHistory);
        runtime.replay_ready = false;
        let mut next = records[2..].to_vec();
        if let InputRecord::Group(group) = &mut next[0].record {
            group.stamp.sync = if damage == 0 { 0 } else { 1 };
            if damage == 2 {
                group.order = GroupOrder::ObservedGroup;
            }
        }
        let stamp = match &next[0].record {
            InputRecord::Group(group) => group.stamp,
            _ => unreachable!(),
        };
        if let InputRecord::Policy(policy) = &mut next[1].record {
            policy.stamp = stamp;
            policy.outcome = PolicyOutcome::NeutralBoundary;
        }
        assert!(replay(&mut runtime, next).commands.is_empty());
        assert!(!runtime.replay_ready, "damage {damage}");
        assert_eq!(runtime.watermark, watermark);
    }
}

#[test]
fn rejected_neutral_priming_never_marks_replay_ready() {
    let mut records = captured_trace(&[group(1, 1, [false; 3], &[])]);
    if let InputRecord::Policy(policy) = &mut records[1].record {
        policy.context = 5;
    }
    let mut runtime = Runtime::default();
    assert!(replay(&mut runtime, records).commands.is_empty());
    assert!(!runtime.replay_ready);
    assert!(runtime.watermark.is_none());
}

#[test]
fn neutral_boundary_requires_complete_active_attributable_route_inventory() {
    let command = GlobalAction::RecenterView.id();
    let merged = OnFootAction::Detonate.id();
    let complete = group(
        1,
        1,
        [false; 3],
        &[
            (command, false, paths(&[A])),
            (merged, false, paths(&[RS, A])),
        ],
    );
    assert!(group_neutral(&complete, &BindingsConfig::new()));
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
            !group_neutral(&candidate, &BindingsConfig::new()),
            "damage {damage}"
        );
    }
}

#[test]
fn actual_capture_mid_pending_single_and_chord_waits_for_direct_and_merged_neutral() {
    let _serial = history::TEST_LOCK.lock();
    history::end_replay();
    history::take_gameplay();
    let command = GlobalAction::RecenterView.id();
    let merged = OnFootAction::Detonate.id();
    let sample = |direct, semantic| {
        [
            (command, direct, paths(&[A])),
            (merged, semantic, paths(&[RS, A])),
        ]
    };
    for state in 0..3 {
        let mut runtime = Runtime::default();
        let mut config = BindingsConfig::new();
        if state == 1 {
            config.stick_chord_window_s = 0.000_000_001;
            config.panel_chord_window_s = 0.000_000_001;
        }
        publish_configured(
            &mut runtime,
            group(1, 1, [false; 3], &sample(false, false)),
            &config,
        );
        publish_configured(
            &mut runtime,
            group(10, 2, [false, true, false], &sample(true, true)),
            &config,
        );
        let held = if state == 2 {
            [true, true, false]
        } else {
            [false, true, false]
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
            group(210, 5, [false; 3], &sample(true, true)),
            &config,
        );
        capture.extend(history::take_capture());
        assert!(
            matches!(capture.last().map(|entry| &entry.record), Some(InputRecord::Policy(policy)) if policy.outcome == PolicyOutcome::UnavailablePrefix)
        );
        publish_configured(
            &mut runtime,
            group(220, 6, [false; 3], &sample(false, true)),
            &config,
        );
        capture.extend(history::take_capture());
        assert!(
            matches!(capture.last().map(|entry| &entry.record), Some(InputRecord::Policy(policy)) if policy.outcome == PolicyOutcome::UnavailablePrefix)
        );
        publish_configured(
            &mut runtime,
            group(230, 7, [false; 3], &sample(false, false)),
            &config,
        );
        capture.extend(history::take_capture());
        assert!(
            matches!(capture.last().map(|entry| &entry.record), Some(InputRecord::Policy(policy)) if policy.outcome == PolicyOutcome::NeutralBoundary)
        );
        publish_configured(
            &mut runtime,
            group(240, 8, [false; 3], &sample(true, true)),
            &config,
        );
        capture.extend(history::take_capture());
        assert_eq!(runtime.reset_serial, serial);
        let records = captured_wire(capture);
        let mut deadlines = records
            .iter()
            .flat_map(|entry| {
                [
                    entry.capture_time_ns.saturating_sub(1),
                    entry.capture_time_ns,
                ]
            })
            .collect::<Vec<_>>();
        deadlines.sort_unstable();
        deadlines.dedup();
        deadlines.push(u64::MAX);
        assert_eq!(scheduled(&records, &deadlines), [SystemCommand::Recenter]);
    }
}
