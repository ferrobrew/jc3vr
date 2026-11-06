//! The wrist's essentials: per mode, a short fixed list of the controls a player cannot find by
//! trying, shown below the contextual rows whether or not they are usable at that moment, so the
//! wrist always answers "how do I release the tethers" or "how do I get out".
//!
//! The rows come from the mode's own binding table with the runtime's sources
//! ([`crate::controllers::bindings::tables::describe::describe_current`]), filtered to the actions
//! listed here, and are labelled by the same projection as the contextual rows
//! ([`crate::controllers::summary::current::project`]); [`merge`] puts them below the contextual
//! rows of the same hand, leaving out one the contextual rows already show.

use jc3gi::input::input_action_map::Action::{self, *};

use crate::controllers::{
    bindings::tables::describe::{Control, Effect, ModAction, Row},
    mode::{AirborneVariant, Mode},
    summary::current::{CurrentContext, CurrentRouting, CurrentSummary},
};

/// The essential subset of `rows` in `mode`: each row with a known source, keeping only its
/// essential actions.
pub(crate) fn rows(mode: Mode, rows: &[Row]) -> Vec<Row> {
    rows.iter()
        .filter(|row| row.control != Control::Unbound)
        .filter_map(|row| {
            let effect = match &row.effect {
                Effect::Actions(actions) => {
                    let kept: Vec<Action> = actions
                        .iter()
                        .copied()
                        .filter(|action| essential_action(mode, *action))
                        .collect();
                    (!kept.is_empty()).then_some(Effect::Actions(kept))?
                }
                Effect::Mod(action) => {
                    essential_mod(mode, *action).then_some(Effect::Mod(*action))?
                }
            };
            Some(Row {
                control: row.control,
                gesture: row.gesture,
                effect,
            })
        })
        .collect()
}

/// The context that projects `rows` (from [`rows`]) as if every one were usable: both hands
/// ready, the triggers free, and the reload button's press shown.
pub(crate) fn context(rows: Vec<Row>) -> CurrentContext {
    CurrentContext {
        usable: rows,
        routing: CurrentRouting {
            ready: [true; 2],
            ..Default::default()
        },
        native_reload: true,
        held_holster: false,
    }
}

/// Put `essentials` (projected from [`context`]) below `current`'s rows, cell by cell, marked as
/// essentials. An essential with the same prefix and text as a row already in its cell adds its
/// sources to that row rather than repeating it.
pub(crate) fn merge(current: &mut CurrentSummary, essentials: CurrentSummary) {
    for (cell, extra) in current.cells.iter_mut().zip(essentials.cells) {
        for mut hint in extra {
            let same = |other: &crate::controllers::summary::current::Hint| {
                other.text.to_lowercase() == hint.text.to_lowercase()
                    && other.prefix.as_ref().map(|p| p.to_lowercase())
                        == hint.prefix.as_ref().map(|p| p.to_lowercase())
            };
            if let Some(existing) = cell.iter_mut().find(|other| same(other)) {
                for source in hint.sources {
                    if !existing.sources.contains(&source) {
                        existing.sources.push(source);
                    }
                }
                continue;
            }
            hint.essential = true;
            cell.push(hint);
        }
    }
}

/// Whether `action` is one of `mode`'s essentials.
fn essential_action(mode: Mode, action: Action) -> bool {
    // A tether release bound to a button of its own, besides the grapple's double tap.
    if mode != Mode::Ui && matches!(action, RELEASE_GRAPPLE | VEHICLE_RELEASE_GRAPPLE) {
        return true;
    }
    match mode {
        Mode::OnFoot(_) => matches!(
            action,
            FIRE_GRAPPLE | RETRACT_GRAPPLE | JUMP | USE_ITEM | RELOAD | DETONATE_EXPLOSIVE
        ),
        Mode::Vehicle(_) => matches!(
            action,
            FIRE_GRAPPLE
                | FIRE_VEHICLE_WEAPON_PRIMARY
                | FIRE_VEHICLE_WEAPON_SECONDARY
                | MC_FIRE
                | STUNT_JUMP
                | EXIT_VEHICLE
                // The exit's long press at speed, the parachute bail.
                | OPEN_PARACHUTE
        ),
        Mode::Airborne(AirborneVariant::Falling) => {
            matches!(action, FIRE_GRAPPLE | OPEN_PARACHUTE | OPEN_WINGSUIT)
        }
        Mode::Airborne(AirborneVariant::Wingsuit) => matches!(
            action,
            FIRE_GRAPPLE
                | OPEN_PARACHUTE
                | WINGSUIT_BOOST
                | FIRE_WINGSUIT_WEAPON_MAIN
                | FIRE_WINGSUIT_WEAPON_SECONDARY
        ),
        Mode::Airborne(AirborneVariant::Parachute) => {
            matches!(action, FIRE_GRAPPLE | RETRACT_GRAPPLE | OPEN_WINGSUIT)
        }
        Mode::Ui => false,
    }
}

/// Whether the mod's `action` is one of `mode`'s essentials. Releasing the tethers is everywhere
/// the grapple is, since nothing else on the wrist says how.
fn essential_mod(mode: Mode, action: ModAction) -> bool {
    match mode {
        Mode::OnFoot(_) => matches!(
            action,
            ModAction::ReleaseTethers | ModAction::WristDetonator
        ),
        Mode::Vehicle(_) => action == ModAction::ReleaseTethers,
        Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Wingsuit) => {
            matches!(action, ModAction::ReleaseTethers | ModAction::Ripcord)
        }
        Mode::Airborne(AirborneVariant::Parachute) => matches!(
            action,
            ModAction::ReleaseTethers | ModAction::ParachuteHandles
        ),
        Mode::Ui => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        controllers::{
            bindings::tables::{
                declare::TableCtx,
                describe::{DescribeCtx, Features, Gesture, describe_current},
            },
            frame::Hand,
            mode::{OnFootVariant, VehicleClass, VehicleSeat},
            summary::{
                current::{CellKind, Hint, Source, project},
                labels::Text,
                localise::humanise,
            },
        },
        vr::input::semantic::{
            SemanticSet,
            bound::{EnumeratedSource, SourceTable},
            vocabulary::OnFootAction,
        },
    };

    const GROUND: Mode = Mode::OnFoot(OnFootVariant::Ground);

    fn row(control: Control, gesture: Gesture, effect: Effect) -> Row {
        Row {
            control,
            gesture,
            effect,
        }
    }

    fn hint(control: Control, text: &str) -> Hint {
        Hint {
            sources: vec![Source {
                control,
                name: None,
            }],
            prefix: None,
            text: text.to_owned(),
            essential: false,
        }
    }

    #[test]
    fn only_the_modes_essential_actions_are_kept() {
        let rows = rows(
            GROUND,
            &[
                row(
                    Control::B(Hand::Left),
                    Gesture::Press,
                    Effect::Actions(vec![FIRE_GRAPPLE, BTN_Y]),
                ),
                row(
                    Control::Trigger(Hand::Right),
                    Gesture::Press,
                    Effect::Actions(vec![FIRE_RIGHT]),
                ),
                row(
                    Control::B(Hand::Left),
                    Gesture::DoubleTap,
                    Effect::Mod(ModAction::ReleaseTethers),
                ),
                row(
                    Control::Motion(Hand::Left),
                    Gesture::Press,
                    Effect::Mod(ModAction::Grenade),
                ),
                row(
                    Control::Unbound,
                    Gesture::Press,
                    Effect::Actions(vec![JUMP]),
                ),
            ],
        );
        assert_eq!(
            rows,
            [
                row(
                    Control::B(Hand::Left),
                    Gesture::Press,
                    Effect::Actions(vec![FIRE_GRAPPLE]),
                ),
                row(
                    Control::B(Hand::Left),
                    Gesture::DoubleTap,
                    Effect::Mod(ModAction::ReleaseTethers),
                ),
            ]
        );
    }

    #[test]
    fn every_mode_with_a_grapple_lists_the_tether_release() {
        let modes = [
            GROUND,
            Mode::OnFoot(OnFootVariant::Swimming),
            Mode::Vehicle(VehicleSeat {
                class: VehicleClass::Car,
                driver: true,
            }),
            Mode::Airborne(AirborneVariant::Falling),
            Mode::Airborne(AirborneVariant::Wingsuit),
            Mode::Airborne(AirborneVariant::Parachute),
        ];
        for mode in modes {
            assert!(essential_action(mode, FIRE_GRAPPLE), "{mode:?}");
            assert!(essential_mod(mode, ModAction::ReleaseTethers), "{mode:?}");
        }
        assert!(!essential_action(Mode::Ui, FIRE_GRAPPLE));
    }

    #[test]
    fn the_vehicle_essentials_are_the_guns_the_grapple_and_the_way_out() {
        let car = Mode::Vehicle(VehicleSeat {
            class: VehicleClass::Car,
            driver: true,
        });
        for action in [
            FIRE_VEHICLE_WEAPON_PRIMARY,
            FIRE_VEHICLE_WEAPON_SECONDARY,
            FIRE_GRAPPLE,
            STUNT_JUMP,
            EXIT_VEHICLE,
            OPEN_PARACHUTE,
        ] {
            assert!(essential_action(car, action), "{action:?}");
        }
        assert!(!essential_action(car, ACCELERATE));
    }

    #[test]
    fn essentials_go_below_the_contextual_rows_without_repeating_them() {
        let left = Control::B(Hand::Left);
        let mut current = CurrentSummary::default();
        current.cells[0] = vec![hint(left, "Release tethers")];
        let mut essentials = CurrentSummary::default();
        essentials.cells[0] = vec![
            hint(Control::StickClick(Hand::Left), "release TETHERS"),
            hint(left, "Fire grapple"),
        ];
        merge(&mut current, essentials);
        let cell = &current.cells[0];
        assert_eq!(cell.len(), 2);
        assert!(!cell[0].essential);
        assert_eq!(cell[0].sources.len(), 2);
        assert_eq!(cell[1].text, "Fire grapple");
        assert!(cell[1].essential);
    }

    #[test]
    fn the_on_foot_table_yields_the_essentials_on_their_bound_sources() {
        let path = |p: &'static str| EnumeratedSource {
            path: p,
            localized: Some("Runtime input"),
        };
        let bound = [
            (OnFootAction::Grapple, "/user/hand/left/input/b/click"),
            (
                OnFootAction::RetractTethers,
                "/user/hand/left/input/trigger/value",
            ),
            (OnFootAction::Reload, "/user/hand/left/input/a/click"),
            (OnFootAction::Jump, "/user/hand/right/input/a/click"),
            (OnFootAction::Use, "/user/hand/right/input/b/click"),
            (OnFootAction::Fire, "/user/hand/right/input/trigger/value"),
        ];
        let mut table = SourceTable::default();
        table.insert(
            SemanticSet::OnFoot,
            OnFootAction::ALL.iter().map(|action| {
                Some(
                    bound
                        .iter()
                        .filter(|(a, _)| a == action)
                        .map(|(_, p)| path(p))
                        .collect(),
                )
            }),
        );
        let ctx = DescribeCtx {
            mode: GROUND,
            dominant: Hand::Right,
            table: TableCtx::default(),
            features: Features::default(),
        };
        let essential = rows(GROUND, &describe_current(&ctx, &table, None));
        let actions: Vec<Action> = essential
            .iter()
            .flat_map(|row| match &row.effect {
                Effect::Actions(actions) => actions.clone(),
                Effect::Mod(_) => Vec::new(),
            })
            .collect();
        for action in [FIRE_GRAPPLE, RETRACT_GRAPPLE, RELOAD, JUMP, USE_ITEM] {
            assert!(actions.contains(&action), "{action:?} in {essential:?}");
        }
        assert!(!actions.contains(&FIRE_RIGHT));
        // The tether release is the grapple button's double tap, on the grapple's own source.
        assert!(essential.iter().any(|row| {
            row.gesture == Gesture::DoubleTap
                && row.effect == Effect::Mod(ModAction::ReleaseTethers)
                && row.control.hand() == Some(Hand::Left)
        }));
        let english = |text: Text| match text {
            Text::Game(key) => humanise(key),
            Text::Mod(text) => text.english().to_owned(),
        };
        let projected = project(
            &essential,
            GROUND,
            &context(essential.clone()),
            &[],
            &english,
            &|source| table.name(source).to_owned(),
            &|_| None,
        );
        let left = &projected.cells[CellKind::Left.index()];
        let right = &projected.cells[CellKind::Right.index()];
        assert!(!left.is_empty() && !right.is_empty());
        assert!(left.len() <= 4 && right.len() <= 4, "{projected:?}");
    }

    #[test]
    fn the_essential_context_projects_every_row_as_usable() {
        let rows = vec![row(
            Control::Trigger(Hand::Left),
            Gesture::Press,
            Effect::Actions(vec![RETRACT_GRAPPLE]),
        )];
        let context = context(rows.clone());
        assert_eq!(context.usable, rows);
        assert!(
            context
                .routing
                .allows(Control::Trigger(Hand::Left), Gesture::Press)
        );
        assert!(context.native_reload);
    }
}
