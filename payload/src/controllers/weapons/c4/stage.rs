//! The C4 throw's stages, from the grab at the pouch to the charge leaving the hand. Pure.
//!
//! The input tick reports the player's hand: a grab at the pouch, the grip's release with the
//! hand's velocity, and a stand-down when the throw can no longer act ([`on_input`]). The C4
//! weapon's own update acts on the weapon ([`before_update`], then [`after_action`] once the
//! action has run).
//!
//! The game's own facts shape the order. A primed charge gets its physics body one update after it
//! spawns, and dropping it needs the body, so a release waits for the body. Placing a charge beyond
//! the most that may be live at once leaves it out of the live list, where nothing would detonate
//! it, so a release with no room left takes the charge back instead of throwing it.

use crate::controllers::{frame::Hand, weapons::grenade::stage::Release};

/// Where a throw is.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum Stage {
    #[default]
    Idle,
    /// A grab at the pouch asked for a charge, which the weapon has not primed yet.
    Wanted {
        hand: Hand,
        release: Option<Release>,
    },
    /// A primed charge is in the hand. With a release, it is thrown as soon as its physics body
    /// exists.
    Held {
        hand: Hand,
        release: Option<Release>,
    },
    /// The weapon is throwing the charge in the update in progress.
    Throwing { hand: Hand, release: Release },
    /// The held charge is taken back at the weapon's next update.
    Cancelling,
}

impl Stage {
    /// The hand the throw belongs to, from the grab until the charge leaves it.
    pub fn hand(self) -> Option<Hand> {
        match self {
            Stage::Wanted { hand, .. }
            | Stage::Held { hand, .. }
            | Stage::Throwing { hand, .. } => Some(hand),
            Stage::Idle | Stage::Cancelling => None,
        }
    }

    /// Whether a primed charge is in the hand, so that it follows the hand.
    pub fn charge_in_hand(self) -> bool {
        matches!(self, Stage::Held { .. } | Stage::Throwing { .. })
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Idle => "idle",
            Stage::Wanted { release: None, .. } => "charge asked for",
            Stage::Wanted { .. } => "charge asked for, already released",
            Stage::Held { release: None, .. } => "held",
            Stage::Held { .. } => "released, waiting for the charge's physics",
            Stage::Throwing { .. } => "throwing",
            Stage::Cancelling => "taking the charge back",
        }
    }
}

/// What the input tick reports.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum InputEvent {
    /// A grip press at the C4 pouch.
    Grab(Hand),
    /// The grip of `hand` opened.
    Release(Hand, Release),
    /// The throw cannot act: its toggle is off, the mode has no free hands, or the bindings stopped
    /// injecting.
    StandDown,
}

/// Why the weapon cannot prime or throw a charge.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// As many charges are live as may be at once.
    NoRoom,
    /// The character may not plant explosives now (the game's permission is off, or the ability is
    /// not unlocked).
    NotAllowed,
    /// The game's own planting holds the weapon: it is in its planting mode or holds a primed
    /// charge of its own.
    Busy,
}

impl Refusal {
    pub const fn label(self) -> &'static str {
        match self {
            Refusal::NoRoom => "as many charges are placed as may be",
            Refusal::NotAllowed => "planting is not allowed now",
            Refusal::Busy => "the game's own planting is under way",
        }
    }
}

/// The C4 weapon as its update begins.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct WeaponView {
    /// The weapon holds a live primed charge.
    pub primed: bool,
    /// The primed charge has its physics body, so it can be dropped.
    pub physics_ready: bool,
    /// The weapon is in the game's own planting mode.
    pub planting: bool,
    /// Why a charge cannot be placed now, if it cannot: [`Refusal::NoRoom`] or
    /// [`Refusal::NotAllowed`].
    pub refusal: Option<Refusal>,
}

/// What to do to the weapon before its update.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum WeaponAction {
    None,
    /// Prime a charge.
    Prime,
    /// Make the primed charge live and drop it from the hand with this release.
    Throw(Release),
    /// Take the primed charge back.
    Cancel,
}

/// A step's result worth reporting.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Outcome {
    /// A charge is in the hand.
    Primed,
    Refused(Refusal),
    /// The weapon spawned no charge: no shot matrix, or the ammunition pool is full.
    SpawnFailed,
    Thrown(Release),
    /// The held charge is gone before its throw: the game took it back.
    Gone,
    /// The held charge was taken back.
    Cancelled,
}

impl Outcome {
    pub fn label(self) -> String {
        match self {
            Outcome::Primed => "primed".to_owned(),
            Outcome::Refused(r) => format!("refused: {}", r.label()),
            Outcome::SpawnFailed => "the game spawned no charge".to_owned(),
            Outcome::Thrown(r) => format!("thrown at {:.1} m/s", r.velocity.length()),
            Outcome::Gone => "gone from the hand before the throw".to_owned(),
            Outcome::Cancelled => "taken back".to_owned(),
        }
    }
}

/// Fold an input-tick event into the stage.
pub fn on_input(stage: Stage, event: InputEvent) -> Stage {
    match (stage, event) {
        (Stage::Idle, InputEvent::Grab(hand)) => Stage::Wanted {
            hand,
            release: None,
        },
        (
            Stage::Wanted {
                hand,
                release: None,
            },
            InputEvent::Release(h, release),
        ) if h == hand => Stage::Wanted {
            hand,
            release: Some(release),
        },
        (
            Stage::Held {
                hand,
                release: None,
            },
            InputEvent::Release(h, release),
        ) if h == hand => Stage::Held {
            hand,
            release: Some(release),
        },
        (Stage::Wanted { .. }, InputEvent::StandDown) => Stage::Idle,
        (Stage::Held { .. }, InputEvent::StandDown) => Stage::Cancelling,
        (stage, _) => stage,
    }
}

/// Decide what the weapon's update is asked to do. The stage returned is provisional until
/// [`after_action`] settles it.
pub fn before_update(stage: Stage, view: WeaponView) -> (Stage, WeaponAction, Option<Outcome>) {
    match stage {
        Stage::Idle => (Stage::Idle, WeaponAction::None, None),
        Stage::Wanted { hand, release } => {
            let refusal = if view.primed || view.planting {
                Some(Refusal::Busy)
            } else {
                view.refusal
            };
            match refusal {
                Some(refusal) => (
                    Stage::Idle,
                    WeaponAction::None,
                    Some(Outcome::Refused(refusal)),
                ),
                None => (Stage::Held { hand, release }, WeaponAction::Prime, None),
            }
        }
        Stage::Held { hand, release } => {
            if !view.primed {
                return (Stage::Idle, WeaponAction::None, Some(Outcome::Gone));
            }
            match release {
                Some(_) if view.physics_ready && view.refusal.is_some() => (
                    Stage::Idle,
                    WeaponAction::Cancel,
                    view.refusal.map(Outcome::Refused),
                ),
                Some(release) if view.physics_ready => (
                    Stage::Throwing { hand, release },
                    WeaponAction::Throw(release),
                    None,
                ),
                _ => (Stage::Held { hand, release }, WeaponAction::None, None),
            }
        }
        // Settled by the previous update's `after_action`; seen here only if that never ran.
        Stage::Throwing { hand, release } => before_update(
            Stage::Held {
                hand,
                release: Some(release),
            },
            view,
        ),
        Stage::Cancelling if view.primed => {
            (Stage::Idle, WeaponAction::Cancel, Some(Outcome::Cancelled))
        }
        Stage::Cancelling => (Stage::Idle, WeaponAction::None, None),
    }
}

/// Settle the stage once `action` has run: `primed` is whether the weapon holds a live primed
/// charge now.
pub fn after_action(stage: Stage, action: WeaponAction, primed: bool) -> (Stage, Option<Outcome>) {
    match (stage, action) {
        (Stage::Held { .. }, WeaponAction::Prime) if primed => (stage, Some(Outcome::Primed)),
        (Stage::Held { .. }, WeaponAction::Prime) => (Stage::Idle, Some(Outcome::SpawnFailed)),
        (Stage::Throwing { release, .. }, _) if !primed => {
            (Stage::Idle, Some(Outcome::Thrown(release)))
        }
        // The weapon kept the charge (its user was gone): try again on the next update.
        (Stage::Throwing { hand, release }, _) => (
            Stage::Held {
                hand,
                release: Some(release),
            },
            None,
        ),
        (stage, _) => (stage, None),
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    const RELEASE: Release = Release {
        velocity: Vec3::new(0.0, 2.0, -6.0),
    };

    fn view(primed: bool, physics_ready: bool) -> WeaponView {
        WeaponView {
            primed,
            physics_ready,
            planting: false,
            refusal: None,
        }
    }

    /// Run one weapon update: before, the action's effect on the weapon, then after.
    fn update(stage: Stage, v: WeaponView) -> (Stage, WeaponAction, Vec<Outcome>) {
        let (stage, action, first) = before_update(stage, v);
        let primed = match action {
            WeaponAction::Prime => true,
            WeaponAction::Throw(_) | WeaponAction::Cancel => false,
            WeaponAction::None => v.primed,
        };
        let (stage, second) = after_action(stage, action, primed);
        (stage, action, first.into_iter().chain(second).collect())
    }

    #[test]
    fn a_grab_primes_and_a_release_throws_once_the_physics_exists() {
        let s = on_input(Stage::Idle, InputEvent::Grab(Hand::Left));
        let (s, action, out) = update(s, view(false, false));
        assert_eq!(action, WeaponAction::Prime);
        assert_eq!(out, vec![Outcome::Primed]);
        assert!(s.charge_in_hand());
        assert_eq!(s.hand(), Some(Hand::Left));

        let s = on_input(s, InputEvent::Release(Hand::Left, RELEASE));
        let (s, action, out) = update(s, view(true, false));
        assert_eq!(action, WeaponAction::None);
        assert!(out.is_empty());
        let (s, action, out) = update(s, view(true, true));
        assert_eq!(action, WeaponAction::Throw(RELEASE));
        assert_eq!(out, vec![Outcome::Thrown(RELEASE)]);
        assert_eq!(s, Stage::Idle);
    }

    #[test]
    fn a_release_before_the_prime_is_kept_for_the_throw() {
        let s = on_input(Stage::Idle, InputEvent::Grab(Hand::Right));
        let s = on_input(s, InputEvent::Release(Hand::Right, RELEASE));
        let (s, action, _) = update(s, view(false, false));
        assert_eq!(action, WeaponAction::Prime);
        let (_, action, _) = update(s, view(true, true));
        assert_eq!(action, WeaponAction::Throw(RELEASE));
    }

    #[test]
    fn the_other_hand_releasing_does_not_throw() {
        let s = Stage::Held {
            hand: Hand::Right,
            release: None,
        };
        assert_eq!(on_input(s, InputEvent::Release(Hand::Left, RELEASE)), s);
    }

    #[test]
    fn a_grab_is_refused_without_room_permission_or_while_the_game_plants() {
        for (v, refusal) in [
            (
                WeaponView {
                    refusal: Some(Refusal::NoRoom),
                    ..view(false, false)
                },
                Refusal::NoRoom,
            ),
            (
                WeaponView {
                    refusal: Some(Refusal::NotAllowed),
                    ..view(false, false)
                },
                Refusal::NotAllowed,
            ),
            (
                WeaponView {
                    planting: true,
                    ..view(false, false)
                },
                Refusal::Busy,
            ),
            (view(true, true), Refusal::Busy),
        ] {
            let s = on_input(Stage::Idle, InputEvent::Grab(Hand::Right));
            let (s, action, out) = update(s, v);
            assert_eq!(action, WeaponAction::None);
            assert_eq!(out, vec![Outcome::Refused(refusal)]);
            assert_eq!(s, Stage::Idle);
        }
    }

    #[test]
    fn a_release_with_no_room_left_takes_the_charge_back() {
        let s = Stage::Held {
            hand: Hand::Right,
            release: Some(RELEASE),
        };
        let (s, action, out) = update(
            s,
            WeaponView {
                refusal: Some(Refusal::NoRoom),
                ..view(true, true)
            },
        );
        assert_eq!(action, WeaponAction::Cancel);
        assert_eq!(out, vec![Outcome::Refused(Refusal::NoRoom)]);
        assert_eq!(s, Stage::Idle);
    }

    #[test]
    fn a_failed_spawn_and_a_vanished_charge_return_to_idle() {
        let s = on_input(Stage::Idle, InputEvent::Grab(Hand::Right));
        let (s, action, _) = before_update(s, view(false, false));
        assert_eq!(action, WeaponAction::Prime);
        assert_eq!(
            after_action(s, action, false),
            (Stage::Idle, Some(Outcome::SpawnFailed))
        );
        let held = Stage::Held {
            hand: Hand::Right,
            release: None,
        };
        let (s, _, out) = update(held, view(false, false));
        assert_eq!(out, vec![Outcome::Gone]);
        assert_eq!(s, Stage::Idle);
    }

    #[test]
    fn a_stand_down_takes_the_charge_back_or_forgets_the_request() {
        let held = Stage::Held {
            hand: Hand::Right,
            release: None,
        };
        let s = on_input(held, InputEvent::StandDown);
        assert_eq!(s, Stage::Cancelling);
        let (s, action, out) = update(s, view(true, false));
        assert_eq!(action, WeaponAction::Cancel);
        assert_eq!(out, vec![Outcome::Cancelled]);
        assert_eq!(s, Stage::Idle);
        let wanted = on_input(Stage::Idle, InputEvent::Grab(Hand::Right));
        assert_eq!(on_input(wanted, InputEvent::StandDown), Stage::Idle);
    }

    #[test]
    fn a_throw_the_weapon_kept_is_retried() {
        let s = Stage::Held {
            hand: Hand::Right,
            release: Some(RELEASE),
        };
        let (s, action, _) = before_update(s, view(true, true));
        assert_eq!(action, WeaponAction::Throw(RELEASE));
        let (s, out) = after_action(s, action, true);
        assert_eq!(out, None);
        let (_, action, _) = before_update(s, view(true, true));
        assert_eq!(action, WeaponAction::Throw(RELEASE));
    }
}
