//! Bounded current eligibility from mode routes, resolved grab choices, and explicit mechanic gates.

use jc3gi::input::input_action_map::Action;

use crate::controllers::{
    bindings::tables::describe::{Control, Effect, Gesture, ModAction, Row},
    frame::Hand,
    grab::GrabVolume,
    summary::current::{CurrentContext, CurrentRouting},
};

/// Post-finish gates supplied by the central input owner. No engine permission is inferred from
/// tracking or from the fact that a hand holds a gun.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Eligibility {
    pub routing: CurrentRouting,
    pub chosen: [Option<GrabVolume>; 2],
    /// Current contextual actions proved usable by their owning subsystem, including cutscene skip.
    pub actions: Vec<Action>,
    /// Actual current physical gestures not represented by a chosen grip target. Examples include
    /// an eligible belt reload or flight gesture. The producer supplies their real physical control.
    pub mechanics: Vec<Row>,
    /// The reload owner permits the native tap, independently of held holstering.
    pub native_reload: bool,
    /// The inventory owner permits the reload input's held holster action.
    pub held_holster: bool,
    /// The grapple-button owner permits release, rather than merely reporting a tether count.
    pub tethers: bool,
    /// The grapple-button owner permits tightening in its current phase and aim context.
    pub tighten: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Omission {
    MissingCurrentEligibility,
    HandUnavailable,
    NoUsableTarget,
}

/// Missing current eligibility stays inspectable instead of being presented as a known binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EligibilityReport {
    pub context: CurrentContext,
    pub omitted: Vec<(Row, Omission)>,
}

/// Admit mode-wide controls only from resolved table rows. Contextual actions require an explicit
/// current gate; physical grip hints use the resolver's chosen target and its physical hand.
pub(crate) fn resolve(rows: &[Row], snapshot: &Eligibility) -> EligibilityReport {
    let mut usable = Vec::new();
    let mut omitted = Vec::new();
    for row in rows {
        if !mechanic_routing(row, snapshot.routing) {
            omitted.push((row.clone(), Omission::HandUnavailable));
            continue;
        }
        if row.gesture == Gesture::Yank && !snapshot.mechanics.contains(row) {
            omitted.push((row.clone(), Omission::NoUsableTarget));
            continue;
        }
        let mut row = row.clone();
        match &mut row.effect {
            Effect::Actions(actions) => {
                actions.retain(|action| match action {
                    Action::RELOAD | Action::MC_RELOAD => {
                        snapshot.native_reload || snapshot.held_holster
                    }
                    _ => mode_wide(*action) || snapshot.actions.contains(action),
                });
                if actions.is_empty() {
                    omitted.push((row, Omission::MissingCurrentEligibility));
                    continue;
                }
            }
            Effect::Mod(action) => {
                let available = match action {
                    ModAction::Turn
                    | ModAction::SeatedCrouch
                    | ModAction::Recenter
                    | ModAction::DebugPanel
                    | ModAction::ShowSummary
                    | ModAction::MapZoom => true,
                    ModAction::ReleaseTethers => snapshot.tethers,
                    ModAction::TightenTethers => snapshot.tighten,
                    _ => snapshot.mechanics.iter().any(|mechanic| {
                        mechanic.control == row.control
                            && mechanic.gesture == row.gesture
                            && mechanic.effect == Effect::Mod(*action)
                    }),
                };
                if !available {
                    omitted.push((row, Omission::NoUsableTarget));
                    continue;
                }
            }
        }
        usable.push(row);
    }
    for hand in Hand::BOTH {
        if !snapshot.routing.ready[hand.index()] {
            continue;
        }
        let action = match snapshot.chosen[hand.index()] {
            Some(GrabVolume::Socket(_)) => Some(ModAction::Holster),
            Some(GrabVolume::Foregrip) => Some(ModAction::Foregrip),
            Some(GrabVolume::MagazinePouch | GrabVolume::MagazineWell) => Some(ModAction::Magazine),
            Some(GrabVolume::WeaponAction) => Some(ModAction::WorkAction),
            Some(GrabVolume::GrenadePouch) => Some(ModAction::Grenade),
            Some(GrabVolume::C4Pouch) => Some(ModAction::C4),
            Some(GrabVolume::ParachuteHandle(_)) => Some(ModAction::ParachuteHandles),
            _ => None,
        };
        if let Some(action) = action {
            usable.push(Row {
                control: Control::Grip(hand),
                gesture: Gesture::Press,
                effect: Effect::Mod(action),
            });
        }
    }
    for row in &snapshot.mechanics {
        let permitted = match row.effect {
            Effect::Mod(ModAction::ReleaseTethers) => snapshot.tethers,
            Effect::Mod(ModAction::TightenTethers) => snapshot.tighten,
            Effect::Mod(_) => true,
            Effect::Actions(_) => false,
        };
        if mechanic_routing(row, snapshot.routing) && permitted && !usable.contains(row) {
            usable.push(row.clone());
        }
    }
    EligibilityReport {
        context: CurrentContext {
            usable,
            routing: snapshot.routing,
            native_reload: snapshot.native_reload,
            held_holster: snapshot.held_holster,
        },
        omitted,
    }
}

fn mechanic_routing(row: &Row, routing: CurrentRouting) -> bool {
    if matches!(
        row.effect,
        Effect::Mod(ModAction::GestureDeploy | ModAction::GestureClose | ModAction::GestureBrake)
    ) {
        return row.control == Control::EitherMotion
            && row.gesture == Gesture::Press
            && routing.ready.into_iter().all(|ready| ready);
    }
    routing.allows(row.control, row.gesture)
}

fn mode_wide(action: Action) -> bool {
    !matches!(
        action,
        Action::SKIP_CUTSCENE
            | Action::FIRE_RIGHT
            | Action::R2
            | Action::RELOAD
            | Action::MC_RELOAD
            | Action::RETRACT_GRAPPLE
            | Action::RELEASE_GRAPPLE
            | Action::VEHICLE_RELEASE_GRAPPLE
            | Action::USE_ITEM
            | Action::ENTER_VEHICLE
            | Action::THROW_GRENADE
            | Action::PLANT_EXPLOSIVE
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::summary::current::TriggerUse;

    #[test]
    fn flight_rows_require_exact_mechanic_identity_and_both_ready_hands() {
        for action in [
            ModAction::GestureDeploy,
            ModAction::GestureClose,
            ModAction::GestureBrake,
        ] {
            let row = Row {
                control: Control::EitherMotion,
                gesture: Gesture::Press,
                effect: Effect::Mod(action),
            };
            let mut snapshot = Eligibility {
                routing: CurrentRouting {
                    ready: [true; 2],
                    triggers: [TriggerUse::Free; 2],
                },
                ..Default::default()
            };
            let rows = std::slice::from_ref(&row);
            assert!(resolve(rows, &snapshot).context.usable.is_empty());
            for wrong in [
                Row {
                    control: Control::Motion(Hand::Left),
                    ..row.clone()
                },
                Row {
                    control: Control::A(Hand::Right),
                    ..row.clone()
                },
                Row {
                    gesture: Gesture::Hold,
                    ..row.clone()
                },
                Row {
                    effect: Effect::Mod(ModAction::ArmSteering),
                    ..row.clone()
                },
            ] {
                snapshot.mechanics = vec![wrong];
                assert!(!resolve(rows, &snapshot).context.usable.contains(&row));
            }
            snapshot.mechanics = vec![row.clone()];
            assert_eq!(resolve(rows, &snapshot).context.usable, vec![row.clone()]);
            for ready in [[false, false], [true, false], [false, true]] {
                snapshot.routing.ready = ready;
                assert!(resolve(rows, &snapshot).context.usable.is_empty());
            }
        }
    }

    #[test]
    fn physical_yank_requires_the_execution_owners_exact_mechanic_row() {
        let yank = Row {
            control: Control::Motion(Hand::Left),
            gesture: Gesture::Yank,
            effect: Effect::Actions(vec![Action::FIRE_GRAPPLE]),
        };
        let mut snapshot = Eligibility {
            routing: CurrentRouting {
                ready: [true; 2],
                triggers: [TriggerUse::Free; 2],
            },
            ..Default::default()
        };
        assert!(
            resolve(std::slice::from_ref(&yank), &snapshot)
                .context
                .usable
                .is_empty()
        );
        snapshot.mechanics.push(yank.clone());
        assert_eq!(
            resolve(std::slice::from_ref(&yank), &snapshot)
                .context
                .usable,
            vec![yank]
        );
    }
}
