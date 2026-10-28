//! Advertised UI routes consume accepted delivery rather than their former raw controls.

use jc3gi::input::input_action_map::Action;

use super::*;
use crate::{
    controllers::{
        bindings::{
            controls::Controls,
            intents::{Mask, MaskScope},
            tables::Bindings,
        },
        mode::{Mode, OnFootVariant},
    },
    vr::input::semantic::bound::{EnumeratedSource, SourceTable, of_sources},
};

fn menu_tick(runtime: &mut Runtime, sequence: u64, select: bool, source: &str) -> Tick {
    let id = MenuAction::PointerSelect.id();
    let mut sample = group(sequence, sequence, [false; 3], &[]);
    sample.samples.retain(|sample| !matches!(sample.route, InputRoute::Semantic { set, .. } if set == SemanticSet::OnFoot.index() as u8));
    for (action, definition) in SemanticSet::Menu.actions().iter().enumerate() {
        for hand in Hand::BOTH {
            let active = action == id.index() && hand == Hand::Right;
            let down = active && select;
            sample.samples.push(RouteSample {
                route: InputRoute::Semantic {
                    set: SemanticSet::Menu.index() as u8,
                    action: action as u16,
                    hand: hand.index() as u8,
                },
                value: match definition.kind {
                    ActionKind::Bool => ValueBits::Bool(down),
                    ActionKind::Float => ValueBits::Float(0),
                    ActionKind::Vec2 => ValueBits::Vec2([0; 2]),
                },
                active,
                down,
                change: None,
                sources: if active { paths(&[source]) } else { paths(&[]) },
            });
        }
    }
    sample.context = 5;
    let frame = ControllerFrame {
        sequence,
        display_time_ns: 0,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::Ui,
        hands: Default::default(),
    };
    runtime.step(TickInput {
        publications: vec![Publication {
            observed_at: Instant::now(),
            frame_sequence: Some(sequence),
            record: InputRecord::Group(sample),
        }],
        replay: false,
        frame: Some(&frame),
        context: 5,
        gameplay: false,
        usable: true,
        config: &BindingsConfig::new(),
    })
}

#[test]
fn remapped_pointer_select_uses_a_in_debug_capture_without_raw_trigger_fallback() {
    let id = MenuAction::PointerSelect.id();
    let mut runtime = Runtime::default();
    menu_tick(&mut runtime, 1, false, A);
    menu_tick(&mut runtime, 2, false, A);
    let tick = menu_tick(&mut runtime, 3, true, A);
    assert!(tick.button(id, Hand::Right).pressed);
    let mut input = SemanticInput::rest(SemanticSet::Menu, Default::default(), Hand::Right);
    input.get_mut(id).unwrap().bound = crate::vr::input::semantic::bound::of_sources([A]);
    tick.filter_semantic(&mut input);
    let controls = Controls {
        pointer_hand: Some(Hand::Right),
        ..Default::default()
    };
    let masks = Bindings::new().effective_masks(Mode::Ui, &controls, &input);
    assert!(masks.iter().any(|mask| mask.covers(
        MenuAction::MenuConfirm.id(),
        crate::vr::input::semantic::bound::PhysicalSet::of(Physical::LowerFace)
    )));
    assert!(
        !masks
            .iter()
            .any(|mask| mask.covers(id, input.get(id).unwrap().bound[1]))
    );
    assert!(
        !menu_tick(&mut runtime, 4, false, A)
            .button(id, Hand::Right)
            .down
    );
    assert!(
        !menu_tick(&mut runtime, 5, false, A)
            .button(id, Hand::Right)
            .held()
    );
    assert!(
        !menu_tick(
            &mut runtime,
            6,
            true,
            "/user/hand/right/input/unknown/click"
        )
        .button(id, Hand::Right)
        .held()
    );
}

#[test]
fn commlink_remap_uses_accepted_press_and_existing_native_toggle_targets() {
    let id = GlobalAction::OpenCommlink.id();
    let mut runtime = Runtime::default();
    let mut bindings = Bindings::new();
    for (sequence, down, accepted) in [
        (1, true, false),
        (2, false, false),
        (3, true, true),
        (4, true, false),
        (5, false, false),
    ] {
        let tick = evaluate(
            &mut runtime,
            group(sequence, sequence, [false; 3], &[(id, down, paths(&[A]))]),
            true,
        );
        let mut input = SemanticInput::rest(SemanticSet::OnFoot, Default::default(), Hand::Right);
        input.get_mut(id).unwrap().bound = crate::vr::input::semantic::bound::of_sources([A]);
        tick.filter_semantic(&mut input);
        let step = bindings.step(
            Mode::OnFoot(OnFootVariant::Ground),
            &Controls::default(),
            &input,
            0.01,
            &BindingsConfig::new(),
        );
        let toggles: Vec<_> = step
            .desired
            .iter()
            .filter(|desired| desired.target.action == Action::GUI_PDA)
            .collect();
        assert_eq!(toggles.len(), if accepted { 2 } else { 0 });
        assert_eq!(tick.button(id, Hand::Right).pressed, accepted);
    }
}

#[test]
fn summary_cancels_only_unambiguous_accepted_touch_holds() {
    let id = GlobalAction::ShowControlsSummary.id();
    for hand in Hand::BOTH {
        let prefix = if hand == Hand::Left {
            "/user/hand/left/input"
        } else {
            "/user/hand/right/input"
        };
        let touch = format!("{prefix}/trackpad/touch");
        let force = format!("{prefix}/trackpad/force");
        let click = format!("{prefix}/trackpad/click");
        let face = format!("{prefix}/a/click");
        for (sources, cancel) in [
            (vec![touch.as_str()], true),
            (vec![force.as_str()], false),
            (vec![click.as_str()], false),
            (vec![face.as_str(), touch.as_str()], false),
        ] {
            let (tick, input) = summary_tick(id, &sources, hand);
            let mut table = summary_sources(id, &sources);
            let mut frame = ControllerFrame {
                sequence: 3,
                display_time_ns: 0,
                published_at: Instant::now(),
                focused: true,
                action_set: ActionSetKind::OnFoot,
                hands: Default::default(),
            };
            frame.hands[hand.index()].active = true;
            frame.hands[hand.index()].inputs.trackpad_touch = true;
            frame.hands[hand.index()].inputs.trackpad_force = 1.0;
            frame.hands[hand.index()]
                .buttons
                .set(Button::TrackpadPressLeft, true);
            assert_eq!(
                crate::controllers::summary::hold::accepted(&input, &tick, &[], &frame, &table),
                (true, cancel)
            );
            frame.hands[hand.index()].active = false;
            assert_eq!(
                crate::controllers::summary::hold::accepted(&input, &tick, &[], &frame, &table),
                (true, false)
            );
            frame.hands[hand.index()].active = true;
            let mut resting = input.clone();
            resting.get_mut(id).unwrap().hands = Default::default();
            assert_eq!(
                crate::controllers::summary::hold::accepted(&resting, &tick, &[], &frame, &table),
                (false, false)
            );
            frame.hands[hand.index()].inputs.trackpad_touch = false;
            assert_eq!(
                crate::controllers::summary::hold::accepted(&input, &tick, &[], &frame, &table),
                (true, false)
            );
            table = SourceTable::default();
            assert_eq!(
                crate::controllers::summary::hold::accepted(&input, &tick, &[], &frame, &table),
                (true, false)
            );
            let masks = [Mask {
                hand,
                scope: MaskScope::Hand,
                exempt: None,
            }];
            assert_eq!(
                crate::controllers::summary::hold::accepted(&input, &tick, &masks, &frame, &table),
                (false, false)
            );
            assert_eq!(
                crate::controllers::summary::hold::accepted(
                    &input,
                    &Tick::default(),
                    &[],
                    &frame,
                    &table
                ),
                (false, false)
            );
        }
    }
}

#[test]
fn pressed_touch_on_one_hand_does_not_cancel_another_accepted_face_hold() {
    let id = GlobalAction::ShowControlsSummary.id();
    for hand in Hand::BOTH {
        let (face, touch) = if hand == Hand::Left {
            (
                "/user/hand/left/input/a/click",
                "/user/hand/right/input/trackpad/touch",
            )
        } else {
            (A, "/user/hand/left/input/trackpad/touch")
        };
        let sources = [face, touch];
        let (mut tick, mut input) = summary_tick(id, &sources, hand);
        let other = hand.other();
        let intent = input.get(id).unwrap().hands[hand.index()];
        tick.permitted.push((id, other));
        tick.routes.push((id, other, intent));
        input.get_mut(id).unwrap().hands[other.index()] = intent;
        let mut frame = ControllerFrame {
            sequence: 3,
            display_time_ns: 0,
            published_at: Instant::now(),
            focused: true,
            action_set: ActionSetKind::OnFoot,
            hands: Default::default(),
        };
        frame.hands[other.index()].active = true;
        frame.hands[other.index()].inputs.trackpad_touch = true;
        frame.hands[other.index()]
            .buttons
            .set(Button::TrackpadPressRight, true);
        assert_eq!(
            crate::controllers::summary::hold::accepted(
                &input,
                &tick,
                &[],
                &frame,
                &summary_sources(id, &sources)
            ),
            (true, false)
        );
    }
}

fn summary_sources(id: ActionId, sources: &[&str]) -> SourceTable {
    let mut actions = vec![Some(Vec::new()); SemanticSet::Global.actions().len()];
    actions[id.index()] = Some(
        sources
            .iter()
            .map(|path| EnumeratedSource {
                path,
                localized: None,
            })
            .collect(),
    );
    let mut table = SourceTable::default();
    table.insert(SemanticSet::Global, actions);
    table
}

fn summary_tick(id: ActionId, sources: &[&str], hand: Hand) -> (Tick, SemanticInput) {
    let mut runtime = Runtime::default();
    let mut tick = Tick::default();
    for sequence in 1..=3 {
        let mut sample = group(
            sequence,
            sequence,
            [false; 3],
            &[(id, sequence == 3, paths(sources))],
        );
        for route in &mut sample.samples {
            if let InputRoute::Semantic {
                set,
                action,
                hand: side,
            } = route.route
                && set == id.set.index() as u8
                && action == id.index as u16
            {
                route.active = side == hand.index() as u8;
                route.down = route.active && sequence == 3;
                route.value = ValueBits::Bool(route.down);
                route.sources = paths(sources);
            }
        }
        tick = evaluate(&mut runtime, sample, true);
    }
    let mut input = SemanticInput::rest(SemanticSet::OnFoot, Default::default(), Hand::Right);
    input.get_mut(id).unwrap().bound = of_sources(sources.iter().copied());
    tick.filter_semantic(&mut input);
    (tick, input)
}

#[test]
fn summary_remap_is_a_boolean_hold_and_claimed_or_unresolved_sources_are_inert() {
    let id = GlobalAction::ShowControlsSummary.id();
    for source in [A, "/user/hand/right/input/unknown/click"] {
        let mut runtime = Runtime::default();
        for (sequence, down) in [(1, false), (2, false), (3, true), (4, true), (5, false)] {
            let tick = evaluate(
                &mut runtime,
                group(
                    sequence,
                    sequence,
                    [false; 3],
                    &[(id, down, paths(&[source]))],
                ),
                true,
            );
            assert_eq!(tick.button(id, Hand::Right).down, source == A && down);
        }
    }
    let mut runtime = Runtime::default();
    for sequence in 1..=2 {
        evaluate(
            &mut runtime,
            group(sequence, sequence, [false; 3], &[(id, false, paths(&[RB]))]),
            true,
        );
    }
    let tick = evaluate(
        &mut runtime,
        group(3, 3, [true, false, true], &[(id, true, paths(&[RB]))]),
        true,
    );
    assert!(!tick.button(id, Hand::Right).held());
}
