use super::*;

#[test]
fn inactive_missing_and_wrong_kind_raw_never_prove_neutral() {
    for damage in 0..4 {
        let mut runtime = Runtime::default();
        evaluate(&mut runtime, group(1, 1, [false; 3], &[]), true);
        evaluate(&mut runtime, group(10, 2, [true, false, false], &[]), true);
        assert_eq!(
            evaluate(&mut runtime, group(20, 3, [true, false, true], &[]), true).commands,
            [SystemCommand::TogglePanel]
        );
        let mut unavailable = group(30, 4, [false; 3], &[]);
        match damage {
            0 => unavailable.samples[0].active = false,
            1 => unavailable.samples[2].active = false,
            2 => {
                unavailable.samples.remove(0);
            }
            _ => unavailable.samples[0].value = ValueBits::Float(0),
        }
        assert!(
            evaluate(&mut runtime, unavailable, true)
                .commands
                .is_empty()
        );
        assert!(
            evaluate(&mut runtime, group(40, 5, [true, false, true], &[]), true)
                .commands
                .is_empty()
        );
        evaluate(&mut runtime, group(50, 6, [false; 3], &[]), true);
        evaluate(&mut runtime, group(60, 7, [true, false, false], &[]), true);
        assert_eq!(
            evaluate(&mut runtime, group(70, 8, [true, false, true], &[]), true).commands,
            [SystemCommand::TogglePanel]
        );
    }
}

#[test]
fn accepted_taps_survive_new_pending_episode_at_every_batch_split() {
    let id = OnFootAction::Detonate.id();
    let states = [false, true, false, true, false, true];
    let trace = states
        .into_iter()
        .enumerate()
        .map(|(index, down)| {
            group(
                index as u64 * 10 + 1,
                index as u64 + 1,
                [false, down, false],
                &[(id, down, paths(&[RS]))],
            )
        })
        .collect::<Vec<_>>();
    for split in 1..trace.len() {
        let mut runtime = Runtime::default();
        let mut raw = Vec::new();
        let mut semantic = Vec::new();
        for tick in [
            batch(&mut runtime, &trace[..split]),
            batch(&mut runtime, &trace[split..]),
        ] {
            if tick.raw[1].pressed || tick.raw[1].released {
                raw.push(tick.raw[1]);
            }
            let value = button(&tick, id);
            if value.pressed || value.released {
                semantic.push(value);
            }
        }
        for _ in 0..8 {
            let tick = batch(&mut runtime, &[]);
            if tick.raw[1].pressed || tick.raw[1].released {
                raw.push(tick.raw[1]);
            }
            let value = button(&tick, id);
            if value.pressed || value.released {
                semantic.push(value);
            }
        }
        let expected = [
            ButtonControl {
                down: true,
                pressed: true,
                released: false,
            },
            ButtonControl {
                down: false,
                pressed: false,
                released: true,
            },
        ]
        .repeat(2);
        assert_eq!(raw, expected, "raw split {split}");
        assert_eq!(semantic, expected, "semantic split {split}");
    }
}

#[test]
fn direct_commands_conflict_on_one_component_and_alias_claims_mask_all_consumers() {
    let recenter = GlobalAction::RecenterView.id();
    let panel = GlobalAction::ToggleDebugPanel.id();
    let jump = OnFootAction::Jump.id();
    let samples = |down| {
        vec![
            (recenter, down, paths(&[A])),
            (panel, down, paths(&[A])),
            (jump, down, paths(&["/user/hand/right/input/trigger/value"])),
        ]
    };
    let mut runtime = Runtime::default();
    evaluate(&mut runtime, group(1, 1, [false; 3], &samples(false)), true);
    let tick = evaluate(&mut runtime, group(2, 2, [false; 3], &samples(true)), true);
    assert!(tick.commands.is_empty());
    assert!(button(&tick, jump).held());
    let samples = |down| {
        vec![
            (
                recenter,
                down,
                paths(&["/user/hand/right/input/trigger/click"]),
            ),
            (jump, down, paths(&["/user/hand/right/input/trigger/value"])),
        ]
    };
    let mut runtime = Runtime::default();
    evaluate(&mut runtime, group(1, 1, [false; 3], &samples(false)), true);
    let tick = evaluate(&mut runtime, group(2, 2, [false; 3], &samples(true)), true);
    assert_eq!(tick.commands, [SystemCommand::Recenter]);
    assert!(!button(&tick, jump).held());
    let mut frame = ControllerFrame {
        sequence: 1,
        display_time_ns: 0,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands: Default::default(),
    };
    frame.hands[1].inputs.trigger = 1.0;
    frame.hands[1].buttons.set(Button::TriggerPress, true);
    let mut edges = Edges::default();
    edges.pressed[1].set(Button::TriggerPress, true);
    tick.filter_raw(&mut frame, &mut edges);
    assert_eq!(frame.hands[1].inputs.trigger, 0.0);
    assert!(!edges.pressed[1].contains(Button::TriggerPress));
}

#[test]
fn independent_direct_commands_survive_and_unknown_component_is_unavailable() {
    let recenter = GlobalAction::RecenterView.id();
    let panel = GlobalAction::ToggleDebugPanel.id();
    for source in [
        "/user/hand/right/input/trigger/click",
        "/user/hand/right/input/trigger/not_real",
    ] {
        let samples = |down| {
            vec![
                (recenter, down, paths(&[A])),
                (panel, down, paths(&[source])),
            ]
        };
        let mut runtime = Runtime::default();
        evaluate(&mut runtime, group(1, 1, [false; 3], &samples(false)), true);
        let tick = evaluate(&mut runtime, group(2, 2, [false; 3], &samples(true)), true);
        assert_eq!(
            tick.commands.contains(&SystemCommand::Recenter),
            source.ends_with("/click")
        );
        assert_eq!(
            tick.commands.contains(&SystemCommand::TogglePanel),
            source.ends_with("/click")
        );
    }
}

#[test]
fn replay_checks_direct_decision_before_dispatch_and_recovers_only_at_neutral() {
    let command = GlobalAction::RecenterView.id();
    let trace = [
        group(1, 1, [false; 3], &[(command, false, paths(&[A]))]),
        group(2, 2, [false; 3], &[(command, true, paths(&[A]))]),
    ];
    let original = captured_trace(&trace);
    let mut changed = original.clone();
    if let InputRecord::Group(group) = &mut changed[2].record {
        let sample = group
            .samples
            .iter_mut()
            .find(|sample| {
                sample.route
                    == InputRoute::Semantic {
                        set: command.set.index() as u8,
                        action: command.index as u16,
                        hand: 1,
                    }
            })
            .unwrap();
        sample.down = false;
        sample.value = ValueBits::Bool(false);
    }
    let mut runtime = Runtime::default();
    assert!(replay(&mut runtime, changed).commands.is_empty());
    assert!(!runtime.replay_ready);
    let mut runtime = Runtime::default();
    assert_eq!(
        replay(&mut runtime, original).commands,
        [SystemCommand::Recenter]
    );
}

#[test]
fn replay_gap_missing_tail_duplicate_and_out_of_order_cancel_holds_and_resync() {
    let trace = [
        group(1, 1, [false; 3], &[]),
        group(10, 2, [false, true, false], &[]),
        group(120, 3, [false, true, false], &[]),
        group(130, 4, [false; 3], &[]),
        group(140, 5, [true, false, false], &[]),
        group(150, 6, [true, false, true], &[]),
    ];
    let original = captured_trace(&trace);
    for damage in 0..4 {
        let mut records = original.clone();
        match damage {
            0 => {
                records.remove(3);
            }
            1 => {
                records.insert(4, records[3].clone());
            }
            2 => {
                records.swap(3, 5);
            }
            _ => {
                records[2].record = InputRecord::Group(group(10, 2, [false, true, false], &[]));
                records.remove(3);
            }
        }
        let mut runtime = Runtime::default();
        let tick = replay(&mut runtime, records);
        assert_eq!(
            tick.commands,
            [SystemCommand::TogglePanel],
            "damage {damage}"
        );
    }
    let mut runtime = Runtime::default();
    replay(&mut runtime, original[..6].to_vec());
    let missing = replay(&mut runtime, vec![original[6].clone()]);
    assert!(!missing.raw[1].held());
    assert!(missing.commands.is_empty());
}

#[test]
fn malformed_policy_and_skipped_group_outcomes_never_dispatch_before_boundary() {
    let command = GlobalAction::RecenterView.id();
    let trace = [
        group(1, 1, [false; 3], &[(command, false, paths(&[A]))]),
        group(2, 2, [false; 3], &[(command, true, paths(&[A]))]),
    ];
    for damage in 0..5 {
        let mut records = captured_trace(&trace);
        if let InputRecord::Policy(policy) = &mut records[3].record {
            match damage {
                0 => policy.version += 1,
                1 => policy.action_schema += 1,
                2 => policy.timing.as_mut().unwrap().panel_join_ns = 0,
                3 => policy.commands ^= 2,
                _ => policy.direct[1].sources ^= 1,
            }
        }
        let mut runtime = Runtime::default();
        assert!(replay(&mut runtime, records).commands.is_empty());
        assert!(!runtime.replay_ready);
    }
    let _serial = history::TEST_LOCK.lock();
    for damage in 0..3 {
        history::begin_capture();
        let mut runtime = Runtime::default();
        let mut rejected = group(1, 1, [false; 3], &[]);
        if damage == 0 {
            rejected.action_schema += 1;
        }
        if damage == 1 {
            rejected.focused = false;
        }
        let frame = ControllerFrame {
            sequence: 1,
            display_time_ns: 0,
            published_at: Instant::now(),
            focused: true,
            action_set: ActionSetKind::OnFoot,
            hands: Default::default(),
        };
        runtime.step(TickInput {
            publications: vec![Publication {
                observed_at: if damage == 2 {
                    Instant::now() - MAX_FRAME_AGE - Duration::from_secs(1)
                } else {
                    Instant::now()
                },
                frame_sequence: Some(1),
                record: InputRecord::Group(rejected),
            }],
            replay: false,
            frame: Some(&frame),
            context: 0,
            gameplay: true,
            usable: true,
            config: &BindingsConfig::new(),
        });
        let outcomes = history::take_capture();
        assert!(
            outcomes
                .iter()
                .any(|entry| matches!(entry.record, InputRecord::Policy(_)))
        );
    }
}

#[test]
fn recording_mid_pending_single_and_chord_waits_for_real_neutral_without_live_reset() {
    let _serial = history::TEST_LOCK.lock();
    for state in 0..3 {
        let mut runtime = Runtime::default();
        evaluate(&mut runtime, group(1, 1, [false; 3], &[]), true);
        evaluate(&mut runtime, group(10, 2, [false, true, false], &[]), true);
        let held = match state {
            0 => [false, true, false],
            1 => {
                evaluate(&mut runtime, group(120, 3, [false, true, false], &[]), true);
                [false, true, false]
            }
            _ => {
                evaluate(&mut runtime, group(20, 3, [true, true, false], &[]), true);
                [true, true, false]
            }
        };
        history::begin_capture();
        let serial = runtime.reset_serial;
        evaluate(&mut runtime, group(200, 4, held, &[]), true);
        assert_eq!(runtime.reset_serial, serial);
        let prefix = history::take_capture();
        assert!(
            matches!(prefix.last().map(|entry| &entry.record), Some(InputRecord::Policy(policy)) if policy.outcome == PolicyOutcome::UnavailablePrefix)
        );
        evaluate(&mut runtime, group(210, 5, [false; 3], &[]), true);
        let boundary = history::take_capture();
        assert!(
            matches!(boundary.last().map(|entry| &entry.record), Some(InputRecord::Policy(policy)) if policy.outcome == PolicyOutcome::NeutralBoundary && policy.neutral)
        );
    }
}
