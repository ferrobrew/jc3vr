use jc3gi::input::input_action_map::Action::*;

use super::*;
use crate::controllers::{
    bindings::tables::coverage::{all_contexts, all_modes, injected},
    mode::{OnFootVariant, VehicleClass, VehicleSeat},
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
    assert!(trigger.contains(&ACCELERATE) && trigger.contains(&FIRE_RIGHT));
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
