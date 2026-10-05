use jc3gi::input::input_action_map::Action::*;

use super::*;
use crate::{
    controllers::{
        bindings::tables::coverage::{all_contexts, all_modes, injected},
        mode::{OnFootVariant, VehicleClass, VehicleSeat},
    },
    vr::input::semantic::{SemanticSet, bound::EnumeratedSource},
};

const ON_FOOT: Mode = Mode::OnFoot(OnFootVariant::Ground);

fn ctx(mode: Mode, dominant: Hand, table: TableCtx) -> DescribeCtx {
    DescribeCtx {
        mode,
        dominant,
        table,
        features: Features::default(),
    }
}

fn actions_of(rows: &[Row], control: Control, gesture: Gesture) -> Vec<Action> {
    rows.iter()
        .filter(|r| r.control == control && r.gesture == gesture)
        .flat_map(|r| match &r.effect {
            Effect::Actions(actions) => actions.clone(),
            Effect::Mod(_) => Vec::new(),
        })
        .collect()
}

#[test]
fn every_injected_action_is_described() {
    for mode in all_modes() {
        for (dominant, table) in all_contexts() {
            let features = Features {
                yank: true,
                ..Features::default()
            };
            let rows = describe(&DescribeCtx {
                mode,
                dominant,
                table,
                features,
            });
            let described: Vec<Action> = rows
                .iter()
                .flat_map(|r| match &r.effect {
                    Effect::Actions(actions) => actions.clone(),
                    Effect::Mod(_) => Vec::new(),
                })
                .collect();
            for action in injected(mode, dominant, table) {
                assert!(
                    described.contains(&action),
                    "{mode:?} {dominant:?} {table:?}: {action:?} is injected but not described"
                );
            }
        }
    }
}

#[test]
fn the_dominant_hand_mirrors_the_description() {
    for dominant in Hand::BOTH {
        let rows = describe(&ctx(ON_FOOT, dominant, TableCtx::default()));
        let fire = actions_of(&rows, Control::Trigger(dominant), Gesture::Press);
        assert!(fire.contains(&FIRE_RIGHT), "{dominant:?}");
        let grapple = actions_of(&rows, Control::Grip(dominant.other()), Gesture::Press);
        assert!(grapple.contains(&FIRE_GRAPPLE), "{dominant:?}");
        assert!(actions_of(&rows, Control::Grip(dominant), Gesture::Press).is_empty());
    }
}

#[test]
fn dual_wielding_moves_the_retract_to_the_stick_click_and_fires_from_both_triggers() {
    let table = TableCtx {
        dual_wield: true,
        ..TableCtx::default()
    };
    let rows = describe(&ctx(ON_FOOT, Hand::Right, table));
    let off_trigger = actions_of(&rows, Control::Trigger(Hand::Left), Gesture::Press);
    assert!(off_trigger.contains(&FIRE_RIGHT));
    assert!(!off_trigger.contains(&RETRACT_GRAPPLE));
    let click = actions_of(&rows, Control::StickClick(Hand::Left), Gesture::Press);
    assert!(click.contains(&RETRACT_GRAPPLE));
    assert!(!click.contains(&NEXT_WEAPON));
}

#[test]
fn the_wingsuit_lists_the_evade_after_a_flick() {
    let rows = describe(&ctx(
        Mode::Airborne(AirborneVariant::Wingsuit),
        Hand::Right,
        TableCtx::default(),
    ));
    assert_eq!(
        actions_of(&rows, Control::A(Hand::Right), Gesture::AfterFlick),
        vec![WINGSUIT_EVADE]
    );
    assert!(actions_of(&rows, Control::A(Hand::Right), Gesture::Press).contains(&OPEN_PARACHUTE));
}

#[test]
fn a_seat_lists_its_class_and_the_passenger_fires() {
    let heli = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Helicopter,
        driver: true,
    });
    let rows = describe(&ctx(heli, Hand::Right, TableCtx::default()));
    assert!(
        actions_of(&rows, Control::Trigger(Hand::Right), Gesture::Press)
            .contains(&HELI_INC_ALTITUDE)
    );
    // The helicopter's dominant stick yaws, so it does not turn the body.
    assert!(
        !rows
            .iter()
            .any(|r| r.effect == Effect::Mod(ModAction::Turn))
    );
    let passenger = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Car,
        driver: false,
    });
    let rows = describe(&ctx(passenger, Hand::Right, TableCtx::default()));
    let trigger = actions_of(&rows, Control::Trigger(Hand::Right), Gesture::Press);
    // Out of the driver's seat the throttle is not read, and the trigger fires.
    assert!(!trigger.contains(&ACCELERATE) && trigger.contains(&FIRE_RIGHT));
}

#[test]
fn the_menu_trigger_of_the_pointer_hand_is_left_out() {
    let table = TableCtx {
        pointer_hand: Some(Hand::Right),
        ..TableCtx::default()
    };
    let rows = describe(&ctx(Mode::Ui, Hand::Right, table));
    assert!(actions_of(&rows, Control::Trigger(Hand::Right), Gesture::Press).is_empty());
    assert_eq!(
        actions_of(&rows, Control::Trigger(Hand::Left), Gesture::Press),
        vec![GUI_PAGE_PREV]
    );
}

#[test]
fn features_add_their_gestures_where_the_hand_weapons_are_used() {
    let features = Features {
        yank: true,
        holsters: true,
        melee: true,
        trackpad_shows_summary: true,
        ..Features::default()
    };
    let on_foot = describe(&DescribeCtx {
        features,
        ..ctx(ON_FOOT, Hand::Right, TableCtx::default())
    });
    assert!(
        on_foot
            .iter()
            .any(|r| r.control == Control::Motion(Hand::Left)
                && r.gesture == Gesture::Yank
                && r.effect == Effect::Actions(vec![FIRE_GRAPPLE, L1]))
    );
    assert!(
        on_foot
            .iter()
            .any(|r| r.effect == Effect::Mod(ModAction::Holster))
    );
    assert!(on_foot.iter().any(|r| r.control == Control::EitherMotion));
    let wingsuit = describe(&DescribeCtx {
        features,
        ..ctx(
            Mode::Airborne(AirborneVariant::Wingsuit),
            Hand::Right,
            TableCtx::default(),
        )
    });
    assert!(
        !wingsuit
            .iter()
            .any(|r| r.effect == Effect::Mod(ModAction::Holster))
    );
    assert!(
        wingsuit
            .iter()
            .any(|r| r.effect == Effect::Mod(ModAction::ShowSummary))
    );
}

#[test]
fn rows_sharing_an_input_merge_without_repeating_actions() {
    let car = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Car,
        driver: true,
    });
    let rows = describe(&ctx(car, Hand::Right, TableCtx::default()));
    let horizontal: Vec<_> = rows
        .iter()
        .filter(|r| {
            r.control == Control::Stick(Hand::Left, StickDir::Horizontal)
                && matches!(r.effect, Effect::Actions(_))
        })
        .collect();
    assert_eq!(horizontal.len(), 1);
    assert_eq!(
        horizontal[0].effect,
        Effect::Actions(vec![
            TURN_LEFT,
            TURN_RIGHT,
            BIKE_LEAN_LEFT,
            BIKE_LEAN_RIGHT,
            PLANE_ROLL_LEFT,
            PLANE_ROLL_RIGHT
        ])
    );
}

#[test]
fn the_menu_folds_both_hands_and_lists_the_grip_modifier() {
    for dominant in Hand::BOTH {
        let rows = describe(&ctx(Mode::Ui, dominant, TableCtx::default()));
        assert_eq!(
            actions_of(&rows, Control::EitherA, Gesture::Press),
            vec![GUI_OK]
        );
        assert_eq!(
            actions_of(&rows, Control::EitherB, Gesture::Press),
            vec![GUI_CANCEL]
        );
        assert_eq!(
            actions_of(&rows, Control::EitherA, Gesture::WithGrip),
            vec![GUI_SPECIAL_BUTTON, GUI_FILTER]
        );
        assert_eq!(
            actions_of(&rows, Control::EitherB, Gesture::WithGrip),
            vec![GUI_USE_BUTTON]
        );
        assert_eq!(
            actions_of(
                &rows,
                Control::EitherStick(StickDir::Vertical),
                Gesture::Press
            ),
            vec![GUI_LSTICK_UP, GUI_LSTICK_DOWN]
        );
        assert_eq!(
            actions_of(
                &rows,
                Control::EitherStick(StickDir::Horizontal),
                Gesture::Press
            ),
            vec![GUI_LSTICK_LEFT, GUI_LSTICK_RIGHT]
        );
        assert_eq!(
            actions_of(
                &rows,
                Control::EitherStick(StickDir::Horizontal),
                Gesture::WithGrip
            ),
            vec![GUI_TAB_PREV, GUI_TAB_NEXT]
        );
        for hand in Hand::BOTH {
            assert!(actions_of(&rows, Control::Grip(hand), Gesture::Press).is_empty());
            assert!(rows.iter().all(|r| r.control.hand() != Some(hand)
                || matches!(r.control, Control::Trigger(_) | Control::StickClick(_))));
        }
        assert!(rows.iter().all(|r| r.control != Control::EitherTrackpad));

        let table = TableCtx {
            trackpad_paging: true,
            ..TableCtx::default()
        };
        let rows = describe(&ctx(Mode::Ui, dominant, table));
        assert_eq!(
            actions_of(&rows, Control::EitherTrackpad, Gesture::PressLeftHalf),
            vec![GUI_TAB_PREV]
        );
        assert_eq!(
            actions_of(&rows, Control::EitherTrackpad, Gesture::PressRightHalf),
            vec![GUI_TAB_NEXT]
        );
    }
}

#[test]
fn the_chords_are_listed_in_every_mode() {
    for mode in all_modes() {
        let rows = describe(&ctx(mode, Hand::Right, TableCtx::default()));
        assert!(rows.iter().any(|r| r.control == Control::BothStickClicks
            && r.gesture == Gesture::Hold
            && r.effect == Effect::Mod(ModAction::Recenter)));
        assert!(
            rows.iter()
                .any(|r| r.control == Control::LeftStickClickAndRightB
                    && r.effect == Effect::Mod(ModAction::DebugPanel))
        );
    }
}

#[test]
fn gameplay_rows_of_different_hands_stay_apart() {
    let rows = describe(&ctx(ON_FOOT, Hand::Right, TableCtx::default()));
    assert!(rows.iter().all(|r| !matches!(
        r.control,
        Control::EitherA | Control::EitherB | Control::EitherStick(_)
    )));
}

#[test]
fn the_trackpad_touch_row_stays_on_the_off_hand_beside_the_paging() {
    let table = TableCtx {
        trackpad_paging: true,
        ..TableCtx::default()
    };
    let features = Features {
        trackpad_shows_summary: true,
        ..Features::default()
    };
    let rows = describe(&DescribeCtx {
        mode: Mode::Ui,
        dominant: Hand::Right,
        table,
        features,
    });
    assert!(
        rows.iter()
            .any(|r| r.control == Control::Trackpad(Hand::Left)
                && r.gesture == Gesture::Touch
                && r.effect == Effect::Mod(ModAction::ShowSummary))
    );
    assert!(
        rows.iter()
            .any(|r| r.control == Control::EitherTrackpad && r.gesture == Gesture::PressLeftHalf)
    );
}

#[test]
fn the_parachute_handles_are_described_only_under_the_parachute() {
    let features = Features {
        parachute_handles: true,
        ..Features::default()
    };
    let handles = |mode| {
        describe(&DescribeCtx {
            features,
            ..ctx(mode, Hand::Right, TableCtx::default())
        })
        .iter()
        .any(|r| {
            r.control == Control::EitherGrip && r.effect == Effect::Mod(ModAction::ParachuteHandles)
        })
    };
    assert!(handles(Mode::Airborne(AirborneVariant::Parachute)));
    assert!(!handles(Mode::Airborne(AirborneVariant::Wingsuit)));
    assert!(!handles(ON_FOOT));
}

/// A source table for the on-foot set where each action in `sources` has those sources (path and
/// localized name) and every other action is not known.
fn on_foot_sources(sources: &[(OnFootAction, &[(&str, &str)])]) -> SourceTable {
    let mut table = SourceTable::default();
    table.insert(
        SemanticSet::OnFoot,
        OnFootAction::ALL.iter().map(|action| {
            sources.iter().find(|(a, _)| a == action).map(|(_, paths)| {
                paths
                    .iter()
                    .map(|&(path, localized)| EnumeratedSource {
                        path,
                        localized: Some(localized),
                    })
                    .collect()
            })
        }),
    );
    table
}

#[test]
fn the_runtime_sources_move_the_rows_and_name_what_has_no_glyph() {
    let table = on_foot_sources(&[
        (
            OnFootAction::Fire,
            &[("/user/hand/left/input/trigger/value", "Left Trigger")],
        ),
        (
            OnFootAction::Jump,
            &[("/user/hand/right/input/pinch_ext/value", "Right Pinch")],
        ),
    ]);
    let rows = describe_bound(&ctx(ON_FOOT, Hand::Right, TableCtx::default()), &table);
    assert!(actions_of(&rows, Control::Trigger(Hand::Left), Gesture::Press).contains(&FIRE_RIGHT));
    assert!(
        !actions_of(&rows, Control::Trigger(Hand::Right), Gesture::Press).contains(&FIRE_RIGHT)
    );
    let named: Vec<_> = rows
        .iter()
        .filter_map(|r| match r.control {
            Control::Named(hand, name) => Some((hand, table.name(name), &r.effect)),
            _ => None,
        })
        .collect();
    assert_eq!(named.len(), 1, "{named:?}");
    let (hand, name, effect) = named[0];
    assert_eq!((hand, name), (Some(Hand::Right), "Right Pinch"));
    assert!(matches!(effect, Effect::Actions(actions) if actions.contains(&JUMP)));
    assert!(!actions_of(&rows, Control::A(Hand::Right), Gesture::Press).contains(&JUMP));
    // The actions the runtime did not report keep their default inputs.
    assert!(actions_of(&rows, Control::B(Hand::Right), Gesture::Press).contains(&USE_ITEM));
}

#[test]
fn actions_bound_to_nothing_share_one_unbound_row() {
    let table = on_foot_sources(&[(OnFootAction::Jump, &[]), (OnFootAction::ThrowGrenade, &[])]);
    let rows = describe_bound(&ctx(ON_FOOT, Hand::Right, TableCtx::default()), &table);
    let unbound: Vec<_> = rows
        .iter()
        .filter(|r| r.control == Control::Unbound)
        .collect();
    assert_eq!(unbound.len(), 1, "{unbound:?}");
    let unbound = actions_of(&rows, Control::Unbound, Gesture::Press);
    assert!(
        unbound.contains(&JUMP) && unbound.contains(&THROW_GRENADE),
        "{unbound:?}"
    );
    // On the defaults, the actions that ship unbound are listed as such too.
    let defaults = describe(&ctx(ON_FOOT, Hand::Right, TableCtx::default()));
    let unbound = actions_of(&defaults, Control::Unbound, Gesture::Press);
    assert_eq!(unbound, vec![THROW_GRENADE]);
}

#[test]
fn a_named_source_is_masked_only_with_its_whole_hand() {
    // The pointer hand's trigger is masked in the menus by input; a source the mod does not
    // recognise on that hand escapes the input mask.
    let rows = |sources: &[(&str, &str)]| {
        let mut table = SourceTable::default();
        table.insert(
            SemanticSet::Menu,
            MenuAction::ALL.iter().map(|action| {
                (*action == MenuAction::MenuPageNext).then(|| {
                    sources
                        .iter()
                        .map(|&(path, localized)| EnumeratedSource {
                            path,
                            localized: Some(localized),
                        })
                        .collect()
                })
            }),
        );
        let menu = TableCtx {
            pointer_hand: Some(Hand::Right),
            ..TableCtx::default()
        };
        let rows = describe_bound(&ctx(Mode::Ui, Hand::Right, menu), &table);
        (rows, table)
    };
    let (known, _) = rows(&[("/user/hand/right/input/trigger/value", "Right Trigger")]);
    assert!(
        known
            .iter()
            .all(|r| r.control != Control::Trigger(Hand::Right))
    );
    let (named, table) = rows(&[("/user/hand/right/input/pinch_ext/value", "Right Pinch")]);
    assert!(named.iter().any(|r| matches!(
        r.control,
        Control::Named(Some(Hand::Right), name) if table.name(name) == "Right Pinch"
    )));
}
