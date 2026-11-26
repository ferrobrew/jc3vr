//! Who moves the root on a simulation tick. Pure.
//!
//! Several systems want the character's root: the game's own states, the hand climbing, the
//! animation's authored motions, and the player's head through the step drive. Exactly one owns it
//! each tick, chosen once in the root hook by [`select`], so that the drive never runs, or is
//! credited, on a tick something else moved the body. The player's head owns it in either posture:
//! a seated player's head moves only by the upper body, and the drive follows that the same way.

use crate::controllers::roomscale::{SeatPhase, posture::EffectivePosture};

/// The owner of the root on one tick, in precedence order.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RootOwner {
    /// The game: the on-foot movement task did not write the proxy's velocity (a vehicle, a
    /// ragdoll, a swim, the parachute or the wingsuit, the grapple, a jump, a sequence, a stunt
    /// position), or a seat transition is under way.
    Game,
    /// The hand climbing replaces the proxy's motion in the pre-simulation callback.
    Climb,
    /// The animation carries the body: an authored one-off, a hard landing, cover, or a hit
    /// reaction's hold. Its root motion is the motion, so the view rides it.
    Authored,
    /// The standing player's head, through the step drive, on top of the stick and the hands.
    Player,
    /// The seated player's head, through the step drive, on top of the stick and the hands, as
    /// [`RootOwner::Player`] does; the seated eye reference stands in for the standing height.
    Seated,
}

impl RootOwner {
    /// Whether the step drive moves the root on this tick.
    pub fn drives(self) -> bool {
        matches!(self, RootOwner::Player | RootOwner::Seated)
    }

    pub fn label(self) -> &'static str {
        match self {
            RootOwner::Game => "game",
            RootOwner::Climb => "climb",
            RootOwner::Authored => "authored",
            RootOwner::Player => "player",
            RootOwner::Seated => "seated",
        }
    }
}

/// What the root hook knows on a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct OwnerInput {
    /// Whether the on-foot movement task wrote the proxy's velocity this tick.
    pub task_wrote: bool,
    /// Whether a hand grips the world.
    pub climbing: bool,
    /// Whether the animation moves the body this tick.
    pub authored: bool,
    pub posture: EffectivePosture,
    pub seat: SeatPhase,
}

/// The root's owner on a tick.
pub fn select(input: &OwnerInput) -> RootOwner {
    if !input.task_wrote || input.seat != SeatPhase::Out {
        RootOwner::Game
    } else if input.climbing {
        RootOwner::Climb
    } else if input.authored {
        RootOwner::Authored
    } else if input.posture == EffectivePosture::Seated {
        RootOwner::Seated
    } else {
        RootOwner::Player
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON_FOOT: OwnerInput = OwnerInput {
        task_wrote: true,
        climbing: false,
        authored: false,
        posture: EffectivePosture::Standing,
        seat: SeatPhase::Out,
    };

    #[test]
    fn the_player_owns_an_ordinary_standing_tick() {
        assert_eq!(select(&ON_FOOT), RootOwner::Player);
        assert!(select(&ON_FOOT).drives());
    }

    #[test]
    fn the_precedence_holds() {
        let all = OwnerInput {
            task_wrote: false,
            climbing: true,
            authored: true,
            posture: EffectivePosture::Seated,
            seat: SeatPhase::Exiting,
        };
        assert_eq!(select(&all), RootOwner::Game);
        let all = OwnerInput {
            task_wrote: true,
            seat: SeatPhase::Out,
            ..all
        };
        assert_eq!(select(&all), RootOwner::Climb);
        let all = OwnerInput {
            climbing: false,
            ..all
        };
        assert_eq!(select(&all), RootOwner::Authored);
        let all = OwnerInput {
            authored: false,
            ..all
        };
        assert_eq!(select(&all), RootOwner::Seated);
        assert!(select(&all).drives());
    }

    #[test]
    fn a_seated_player_drives_on_foot() {
        let seated = OwnerInput {
            posture: EffectivePosture::Seated,
            ..ON_FOOT
        };
        assert_eq!(select(&seated), RootOwner::Seated);
        assert!(select(&seated).drives());
    }

    #[test]
    fn only_the_players_head_drives() {
        for owner in [RootOwner::Game, RootOwner::Climb, RootOwner::Authored] {
            assert!(!owner.drives(), "{}", owner.label());
        }
    }

    #[test]
    fn a_climb_never_drives() {
        let climbing = OwnerInput {
            climbing: true,
            ..ON_FOOT
        };
        assert_eq!(select(&climbing), RootOwner::Climb);
        assert!(!select(&climbing).drives());
    }

    #[test]
    fn a_seat_transition_belongs_to_the_game() {
        for seat in [SeatPhase::Entering, SeatPhase::Exiting, SeatPhase::Locked] {
            assert_eq!(select(&OwnerInput { seat, ..ON_FOOT }), RootOwner::Game);
        }
    }
}
