//! The suit's typed commands as game actions: the deploy, the close, and the air brake, through
//! the narrow player-map actions the game's own wingsuit takes.

use jc3gi::input::input_action_map::Action::{CANCEL, OPEN_WINGSUIT, WINGSUIT_AIRBRAKE};
use jc3vr_body::controllers::intent::hold::{Desired, MapKind, Target, Want};

use crate::vehicles::wingsuit_gestures::{GestureCommands, Stage, SuitTransition};

/// Invalid command contexts emit no partial action list. The caller retains canopy precedence.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CommandRejection {
    CanopyRequested,
    WrongMode,
    ConflictingCommands,
}

/// Translate typed transitions through the existing narrow player-map action, with stage guards.
/// Both suit transitions use `OPEN_WINGSUIT`; the broad use/vehicle-entry family is not emitted. A
/// deploy goes in during the game's jump as in its fall: both take the suit's opening.
pub fn desired_for(
    commands: GestureCommands,
    stage: Stage,
    canopy_requested: bool,
) -> Result<Vec<Desired>, CommandRejection> {
    if canopy_requested {
        return Err(CommandRejection::CanopyRequested);
    }
    let suit = stage == Stage::Suit;
    if commands.transition.is_some() && commands.brake {
        return Err(CommandRejection::ConflictingCommands);
    }
    let transition = match commands.transition {
        Some(SuitTransition::Deploy) if stage == Stage::Fall => true,
        Some(SuitTransition::Close) if suit => true,
        Some(SuitTransition::Deploy | SuitTransition::Close) => {
            return Err(CommandRejection::WrongMode);
        }
        None => false,
    };
    if commands.brake && !suit {
        return Err(CommandRejection::WrongMode);
    }
    let mut desired = Vec::with_capacity(2);
    if transition {
        desired.push(Desired {
            target: Target {
                map: MapKind::Player,
                action: OPEN_WINGSUIT,
            },
            want: Want::Click,
        });
    }
    if commands.brake {
        desired.extend([CANCEL, WINGSUIT_AIRBRAKE].map(|action| Desired {
            target: Target {
                map: MapKind::Player,
                action,
            },
            want: Want::Hold(1.0),
        }));
    }
    Ok(desired)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_transitions_emit_only_the_narrow_player_action_in_the_correct_mode() {
        for (transition, stage) in [
            (SuitTransition::Deploy, Stage::Fall),
            (SuitTransition::Close, Stage::Suit),
        ] {
            let commands = GestureCommands {
                transition: Some(transition),
                ..Default::default()
            };
            assert_eq!(
                desired_for(commands, stage, false).unwrap(),
                vec![Desired {
                    target: Target {
                        map: MapKind::Player,
                        action: OPEN_WINGSUIT
                    },
                    want: Want::Click,
                }]
            );
            assert_eq!(
                desired_for(commands, Stage::Other, false),
                Err(CommandRejection::WrongMode)
            );
            assert_eq!(
                desired_for(commands, stage, true),
                Err(CommandRejection::CanopyRequested)
            );
        }
    }

    #[test]
    fn brake_uses_existing_held_pair_and_rejects_conflicts_or_nonflight_contexts() {
        let stage = Stage::Suit;
        let commands = GestureCommands {
            brake: true,
            ..Default::default()
        };
        let desired = desired_for(commands, stage, false).unwrap();
        assert_eq!(desired.len(), 2);
        assert_eq!(desired[0].target.action, CANCEL);
        assert_eq!(desired[1].target.action, WINGSUIT_AIRBRAKE);
        assert!(
            desired
                .iter()
                .all(|input| input.target.map == MapKind::Player && input.want == Want::Hold(1.0))
        );
        assert!(
            desired_for(GestureCommands::default(), stage, false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            desired_for(commands, Stage::Other, false),
            Err(CommandRejection::WrongMode)
        );
        assert_eq!(
            desired_for(
                GestureCommands {
                    transition: Some(SuitTransition::Close),
                    ..commands
                },
                stage,
                false
            ),
            Err(CommandRejection::ConflictingCommands)
        );
        assert_eq!(
            desired_for(
                GestureCommands {
                    transition: Some(SuitTransition::Deploy),
                    ..Default::default()
                },
                stage,
                false
            ),
            Err(CommandRejection::WrongMode)
        );
    }
}
