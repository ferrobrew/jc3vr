use std::sync::Arc;

use super::*;
use crate::{
    controllers::{
        bindings::tables::{
            declare::TableCtx,
            describe::{DescribeCtx, Features, StickDir, describe_bound, describe_sources},
        },
        grab::GrabVolume,
        mode::OnFootVariant,
        summary::{
            eligibility::{self, Eligibility},
            localise::humanise,
            page_clock::{DWELL_S, PageClock},
            retain_wrist,
        },
    },
    vr::input::semantic::{
        SemanticSet,
        bound::{EnumeratedSource, SourceTable},
        vocabulary::OnFootAction,
    },
};

const MODE: Mode = Mode::OnFoot(OnFootVariant::Ground);

fn english(text: Text) -> String {
    match text {
        Text::Game(key) => humanise(key),
        Text::Mod(text) => text.english().to_owned(),
    }
}

fn context(dominant: Hand) -> DescribeCtx {
    DescribeCtx {
        mode: MODE,
        dominant,
        table: TableCtx::default(),
        features: Features::default(),
    }
}

fn sources(entries: &[(OnFootAction, &[&str])]) -> SourceTable {
    let mut actions = vec![Some(Vec::new()); OnFootAction::ALL.len()];
    for (action, paths) in entries {
        actions[action.id().index as usize] = Some(
            paths
                .iter()
                .map(|path| EnumeratedSource {
                    path,
                    localized: Some("Runtime input"),
                })
                .collect(),
        );
    }
    let mut table = SourceTable::default();
    table.insert(SemanticSet::OnFoot, actions);
    table
}

fn snapshot(ready: [bool; 2], triggers: [TriggerUse; 2]) -> Eligibility {
    Eligibility {
        routing: CurrentRouting { ready, triggers },
        ..Default::default()
    }
}

fn resolved(
    ctx: &DescribeCtx,
    table: &SourceTable,
    snapshot: &Eligibility,
    hints: &[ContextHint],
) -> CurrentSummary {
    let rows = describe_sources(ctx, table);
    let report = eligibility::resolve(&rows, snapshot);
    project(
        &rows,
        ctx.mode,
        &report.context,
        hints,
        &english,
        &|source| table.name(source).to_owned(),
        &|_| None,
    )
}

fn text(summary: &CurrentSummary) -> String {
    summary
        .cells
        .iter()
        .flatten()
        .map(|hint| hint.text.as_str())
        .collect::<Vec<_>>()
        .join(" / ")
}

#[test]
fn named_source_index_churn_preserves_wrist_revision_arc_page_and_dwell() {
    let make_table = |earlier_move: bool, jump_path: &str, jump_name: &str| {
        let mut actions = vec![Some(Vec::new()); OnFootAction::ALL.len()];
        if earlier_move {
            actions[OnFootAction::Move.id().index as usize] = Some(vec![EnumeratedSource {
                path: "/user/hand/left/input/thumbstick",
                localized: Some("Unavailable left movement"),
            }]);
        }
        actions[OnFootAction::Jump.id().index as usize] = Some(vec![EnumeratedSource {
            path: jump_path,
            localized: Some(jump_name),
        }]);
        let mut table = SourceTable::default();
        table.insert(SemanticSet::OnFoot, actions);
        table
    };
    let ctx = context(Hand::Right);
    let right_path = "/user/hand/right/input/pinch_ext/value";
    let original_table = make_table(false, right_path, "Right pinch");
    let shifted_table = make_table(true, right_path, "Right pinch");
    let right_ready = snapshot([false, true], [TriggerUse::Free; 2]);
    let original = resolved(&ctx, &original_table, &right_ready, &[]);
    let shifted = resolved(&ctx, &shifted_table, &right_ready, &[]);
    let original_source = &original.cells[1][0].sources[0];
    let shifted_source = &shifted.cells[1][0].sources[0];
    assert_ne!(original_source.control, shifted_source.control);
    assert_eq!(original_source.name, shifted_source.name);
    let reference = |table: &SourceTable| {
        describe_bound(&ctx, table).into_iter().find(|row| {
            matches!(row.control, Control::Named(Some(Hand::Right), _))
                && matches!(&row.effect, Effect::Actions(actions) if actions.contains(&Action::JUMP))
        }).unwrap().control
    };
    assert_ne!(reference(&original_table), reference(&shifted_table));
    let (revision, original) = retain_wrist(None, original);
    let mut clock = PageClock::new();
    clock.step(revision, 3, true, true, 0.0);
    let before = clock.step(revision, 3, true, true, DWELL_S + 1.0);
    assert_eq!(before.index, 1);
    let (shifted_revision, retained) = retain_wrist(Some((revision, &original)), shifted);
    assert_eq!(shifted_revision, revision);
    assert!(Arc::ptr_eq(&original, &retained));
    assert_eq!(clock.step(shifted_revision, 3, true, true, 0.0), before);

    let renamed_table = make_table(true, right_path, "Right squeeze");
    let renamed = resolved(&ctx, &renamed_table, &right_ready, &[]);
    let (renamed_revision, renamed) = retain_wrist(Some((revision, &retained)), renamed);
    assert_eq!(renamed_revision, revision + 1);
    assert!(!Arc::ptr_eq(&retained, &renamed));
    assert_eq!(clock.step(renamed_revision, 3, true, true, 0.0).index, 0);

    let moved_table = make_table(true, "/user/hand/left/input/pinch_ext/value", "Right pinch");
    let moved = resolved(
        &ctx,
        &moved_table,
        &snapshot([true; 2], [TriggerUse::Free; 2]),
        &[],
    );
    let (moved_revision, _) = retain_wrist(Some((revision, &retained)), moved);
    assert_eq!(moved_revision, revision + 1);
}

#[test]
fn production_sources_preserve_ready_physical_hands_before_projection() {
    let table = sources(&[(
        OnFootAction::Jump,
        &[
            "/user/hand/left/input/a/click",
            "/user/hand/right/input/a/click",
        ],
    )]);
    for dominant in Hand::BOTH {
        let ctx = context(dominant);
        assert!(
            describe_bound(&ctx, &table)
                .iter()
                .any(|row| row.control == Control::EitherA)
        );
        let rows = describe_sources(&ctx, &table);
        for hand in Hand::BOTH {
            assert!(rows.iter().any(|row| row.control == Control::A(hand)));
        }
        for ready in [[true, false], [false, true], [false, false], [true, true]] {
            let summary = resolved(&ctx, &table, &snapshot(ready, [TriggerUse::Free; 2]), &[]);
            for hand in Hand::BOTH {
                assert_eq!(!summary.cells[hand.index()].is_empty(), ready[hand.index()]);
                for hint in &summary.cells[hand.index()] {
                    assert!(
                        hint.sources
                            .iter()
                            .all(|source| source.control.hand() == Some(hand))
                    );
                }
            }
            assert!(summary.cells[CellKind::Available.index()].is_empty());
            assert_eq!(
                !summary.cells[CellKind::ViewMenu.index()].is_empty(),
                ready == [true; 2]
            );
        }
    }
}

#[test]
fn source_only_unknown_or_unbound_tables_cannot_acquire_default_buttons_or_chords() {
    for table in [SourceTable::default(), sources(&[])] {
        let summary = resolved(&context(Hand::Right), &table, &Eligibility::default(), &[]);
        assert!(summary.cells.iter().all(Vec::is_empty));
    }
}

#[test]
fn trigger_transfer_keeps_recognized_face_stick_and_current_grip_target() {
    let table = sources(&[
        (OnFootAction::Jump, &["/user/hand/left/input/a/click"]),
        (OnFootAction::Move, &["/user/hand/left/input/thumbstick"]),
        (OnFootAction::Fire, &["/user/hand/left/input/trigger/value"]),
    ]);
    let mut snapshot = snapshot([true; 2], [TriggerUse::Reserved, TriggerUse::Free]);
    snapshot.chosen[0] = Some(GrabVolume::Foregrip);
    snapshot.actions = vec![Action::FIRE_RIGHT, Action::R2];
    let summary = resolved(&context(Hand::Right), &table, &snapshot, &[]);
    let left = &summary.cells[0];
    assert!(left.iter().any(|hint| {
        hint.sources
            .iter()
            .any(|source| source.control == Control::A(Hand::Left))
    }));
    assert!(left.iter().any(|hint| {
        hint.sources
            .iter()
            .any(|source| matches!(source.control, Control::Stick(Hand::Left, _)))
    }));
    assert!(left.iter().any(|hint| {
        hint.sources
            .iter()
            .any(|source| source.control == Control::Grip(Hand::Left))
    }));
    assert!(!left.iter().any(|hint| {
        hint.sources
            .iter()
            .any(|source| source.control == Control::Trigger(Hand::Left))
    }));
}

#[test]
fn disabled_per_hand_substitution_preserves_rebound_semantic_fire_without_raw_trigger_hint() {
    let table = sources(&[(OnFootAction::Fire, &["/user/hand/right/input/a/click"])]);
    let mut snapshot = snapshot([true; 2], [TriggerUse::Free; 2]);
    snapshot.actions = vec![Action::FIRE_RIGHT, Action::R2];
    let summary = resolved(&context(Hand::Right), &table, &snapshot, &[]);
    assert!(summary.cells[1].iter().any(|hint| {
        hint.text == "Fire weapon"
            && hint
                .sources
                .iter()
                .all(|source| source.control == Control::A(Hand::Right))
    }));
    assert!(
        !summary
            .cells
            .iter()
            .flatten()
            .flat_map(|hint| &hint.sources)
            .any(|source| matches!(source.control, Control::Trigger(_)))
    );
}

#[test]
fn native_pair_fire_survives_named_and_merged_sources_only_as_usable_fire_subset() {
    for path in [
        "/user/hand/right/input/pinch_ext/value",
        "/user/hand/right/input/trigger/value",
    ] {
        let table = sources(&[(OnFootAction::Fire, &[path]), (OnFootAction::Jump, &[path])]);
        for permit_jump in [false, true] {
            let mut snapshot = snapshot([true; 2], [TriggerUse::Free, TriggerUse::NativePair]);
            snapshot.actions = vec![Action::FIRE_RIGHT, Action::R2];
            let ctx = context(Hand::Right);
            let mut rows = describe_sources(&ctx, &table);
            if !permit_jump {
                for row in &mut rows {
                    if let Effect::Actions(actions) = &mut row.effect {
                        actions.retain(|action| *action != Action::JUMP);
                    }
                }
            }
            let report = eligibility::resolve(&rows, &snapshot);
            let summary = project(
                &rows,
                MODE,
                &report.context,
                &[],
                &english,
                &|source| table.name(source).to_owned(),
                &|_| None,
            );
            assert!(text(&summary).contains("Fire weapon"));
            assert!(!text(&summary).contains("Jump"));
            assert_eq!(summary.cells[1].len(), 1);
            if path.contains("pinch") {
                assert!(matches!(
                    summary.cells[1][0].sources[0].control,
                    Control::Named(Some(Hand::Right), _)
                ));
                assert_eq!(
                    summary.cells[1][0].sources[0].name.as_deref(),
                    Some("Runtime input")
                );
            }
        }
    }
}

#[test]
fn duplicate_hints_merge_sources_and_normalize_hold_once() {
    let rows = [Control::A(Hand::Right), Control::B(Hand::Right)].map(|control| Row {
        control,
        gesture: Gesture::Hold,
        effect: Effect::Actions(vec![Action::SKIP_CUTSCENE]),
    });
    let mut snapshot = snapshot([true; 2], [TriggerUse::Free; 2]);
    snapshot.actions = vec![Action::SKIP_CUTSCENE];
    let report = eligibility::resolve(&rows, &snapshot);
    let hints = [ContextHint {
        id: 1,
        action: Action::SKIP_CUTSCENE,
        hold: true,
        description: 1,
    }; 2];
    let summary = project(
        &rows,
        MODE,
        &report.context,
        &hints,
        &english,
        &|_| String::new(),
        &|_| Some("Hold Hold Skip cutscene".to_owned()),
    );
    assert_eq!(summary.cells[1].len(), 1);
    assert_eq!(summary.cells[1][0].sources.len(), 2);
    assert_eq!(summary.cells[1][0].prefix.as_deref(), Some("Hold"));
    assert_eq!(summary.cells[1][0].text, "Skip cutscene");
}

#[test]
fn reload_tap_and_held_holster_permissions_keep_only_their_actual_sources() {
    for native_reload in [false, true] {
        for held_holster in [false, true] {
            let table = sources(&[(
                OnFootAction::Jump,
                &["/user/hand/right/input/pinch_ext/value"],
            )]);
            let named = describe_sources(&context(Hand::Left), &table)
                .into_iter()
                .find(|row| matches!(row.control, Control::Named(Some(Hand::Right), _)))
                .unwrap()
                .control;
            for control in [Control::A(Hand::Left), named] {
                let rows = [Row {
                    control,
                    gesture: Gesture::Press,
                    effect: Effect::Actions(vec![Action::RELOAD, Action::MC_RELOAD, Action::BTN_X]),
                }];
                let mut snapshot = snapshot([true; 2], [TriggerUse::Free; 2]);
                snapshot.actions = vec![Action::RELOAD, Action::MC_RELOAD];
                snapshot.native_reload = native_reload;
                snapshot.held_holster = held_holster;
                let report = eligibility::resolve(&rows, &snapshot);
                let hints = [ContextHint {
                    id: 1,
                    action: Action::RELOAD,
                    hold: true,
                    description: 1,
                }];
                let summary = project(
                    &rows,
                    MODE,
                    &report.context,
                    &hints,
                    &english,
                    &|source| table.name(source).to_owned(),
                    &|_| Some("Hold Reload / Holster weapon".to_owned()),
                );
                let cell = &summary.cells[control.hand().unwrap().index()];
                assert_eq!(
                    cell.len(),
                    usize::from(native_reload) + usize::from(held_holster)
                );
                assert_eq!(cell.iter().any(|hint| hint.text == "Reload"), native_reload);
                assert_eq!(
                    cell.iter().any(|hint| hint.text == "Holster weapon"),
                    held_holster
                );
                for hint in cell {
                    assert_eq!(
                        hint.prefix.as_deref(),
                        (hint.text == "Holster weapon").then_some("Hold")
                    );
                    assert_eq!(hint.sources.len(), 1);
                    assert_eq!(hint.sources[0].control, control);
                    assert_eq!(
                        hint.sources[0].name.as_deref(),
                        matches!(control, Control::Named(_, _)).then_some("Runtime input")
                    );
                }
                let reference = build(&rows, MODE, Hand::Right, &english, &|_| String::new());
                assert_eq!(
                    reference.groups[0].lines[0].text,
                    "Reload / Hold Holster weapon"
                );
            }
        }
    }
}

#[test]
fn reload_permissions_do_not_restore_missing_or_trigger_blocked_routes() {
    let mut snapshot = snapshot([true; 2], [TriggerUse::Reserved, TriggerUse::Free]);
    snapshot.native_reload = true;
    snapshot.held_holster = true;
    for rows in [
        Vec::new(),
        vec![Row {
            control: Control::Trigger(Hand::Left),
            gesture: Gesture::Press,
            effect: Effect::Actions(vec![Action::RELOAD]),
        }],
    ] {
        let report = eligibility::resolve(&rows, &snapshot);
        let summary = project(
            &rows,
            MODE,
            &report.context,
            &[],
            &english,
            &|_| String::new(),
            &|_| None,
        );
        assert!(summary.cells.iter().all(Vec::is_empty));
    }
}

#[test]
fn release_and_tighten_permissions_are_independent_even_for_exact_mechanic_rows() {
    let rows = [
        Row {
            control: Control::B(Hand::Left),
            gesture: Gesture::DoubleTap,
            effect: Effect::Mod(ModAction::ReleaseTethers),
        },
        Row {
            control: Control::B(Hand::Left),
            gesture: Gesture::Hold,
            effect: Effect::Mod(ModAction::TightenTethers),
        },
    ];
    for release in [false, true] {
        for tighten in [false, true] {
            let mut snapshot = snapshot([true; 2], [TriggerUse::Free; 2]);
            snapshot.tethers = release;
            snapshot.tighten = tighten;
            snapshot.mechanics = rows.to_vec();
            for described in [&rows[..], &[][..]] {
                let report = eligibility::resolve(described, &snapshot);
                assert_eq!(report.context.usable.contains(&rows[0]), release);
                assert_eq!(report.context.usable.contains(&rows[1]), tighten);
                let summary = project(
                    described,
                    MODE,
                    &report.context,
                    &[],
                    &english,
                    &|_| String::new(),
                    &|_| None,
                );
                let cell = &summary.cells[Hand::Left.index()];
                assert_eq!(cell.len(), usize::from(release) + usize::from(tighten));
                for hint in cell {
                    assert_eq!(hint.sources[0].control, Control::B(Hand::Left));
                    assert_eq!(
                        hint.prefix.as_deref(),
                        Some(if hint.text.contains("Release") {
                            "DOUBLE TAP"
                        } else {
                            "Hold"
                        })
                    );
                }
            }
        }
    }
}

#[test]
fn handless_chords_require_their_participants_and_transfer_does_not_disable_them() {
    for control in [Control::BothStickClicks, Control::LeftStickClickAndRightB] {
        for ready in [[false; 2], [true, false], [false, true], [true; 2]] {
            let routing = CurrentRouting {
                ready,
                triggers: [TriggerUse::Reserved; 2],
            };
            assert_eq!(routing.allows(control, Gesture::Press), ready == [true; 2]);
            assert_eq!(
                routing.allows(Control::A(Hand::Right), Gesture::WithBothGrips),
                ready == [true; 2]
            );
        }
    }
    assert!(
        CurrentRouting {
            ready: [true; 2],
            triggers: [TriggerUse::Reserved; 2]
        }
        .allows(Control::Stick(Hand::Left, StickDir::Any), Gesture::Press)
    );
}
