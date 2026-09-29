//! The physical throw's stages, from the grab at the pouch to the grenade leaving the hand. Pure.
//!
//! Two threads drive the stages. The input tick reports the player's hand: a grab at the pouch, the
//! grip's release with the hand's velocity, and a stand-down when the throw can no longer act
//! ([`on_input`]). The grenade weapon's own update, on the player's post-camera pass, acts on the
//! weapon ([`before_update`], then [`after_update`] once the weapon's update has run).
//!
//! The game's own facts shape the order. Priming consumes a grenade and starts its fuse, so a grenade
//! held too long explodes in the hand, and a stand-down puts a held grenade back (the game refunds
//! it). A primed grenade gets its physics body one update after it spawns, and a launch before that
//! does nothing, so a release waits for the body. The weapon's update fires before it primes, so a
//! prime and a throw are never asked of the same update: the throw waits for the next one.

use glam::Vec3;

use crate::controllers::frame::Hand;

/// A throw's release.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Release {
    /// The hand's velocity at the release in world axes, metres per second, relative to the
    /// character (whose own velocity the launch adds).
    pub velocity: Vec3,
}

/// Where a throw is.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum Stage {
    #[default]
    Idle,
    /// A grab at the pouch asked for a grenade, which the weapon has not primed yet.
    Wanted {
        hand: Hand,
        release: Option<Release>,
    },
    /// The weapon's prime request is set for the update in progress.
    Priming {
        hand: Hand,
        release: Option<Release>,
    },
    /// A primed grenade is in the hand, its fuse running. With a release, it is thrown as soon as
    /// its physics body exists.
    Held {
        hand: Hand,
        release: Option<Release>,
    },
    /// The weapon's fire request is set for the update in progress.
    Throwing { hand: Hand, release: Release },
    /// The held grenade goes back to the inventory at the weapon's next update.
    Cancelling,
}

impl Stage {
    /// The hand the throw belongs to, from the grab until the grenade leaves it.
    pub fn hand(self) -> Option<Hand> {
        match self {
            Stage::Wanted { hand, .. }
            | Stage::Priming { hand, .. }
            | Stage::Held { hand, .. }
            | Stage::Throwing { hand, .. } => Some(hand),
            Stage::Idle | Stage::Cancelling => None,
        }
    }

    /// Whether a primed grenade is (or is about to be) in the hand, so that it follows the hand.
    pub fn grenade_in_hand(self) -> bool {
        matches!(
            self,
            Stage::Priming { .. } | Stage::Held { .. } | Stage::Throwing { .. }
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Idle => "idle",
            Stage::Wanted { release: None, .. } => "grenade asked for",
            Stage::Wanted { .. } => "grenade asked for, already released",
            Stage::Priming { .. } => "priming",
            Stage::Held { release: None, .. } => "held",
            Stage::Held { .. } => "released, waiting for the grenade's physics",
            Stage::Throwing { .. } => "throwing",
            Stage::Cancelling => "putting the grenade back",
        }
    }
}

/// What the input tick reports.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum InputEvent {
    /// A grip press at the grenade pouch.
    Grab(Hand),
    /// The grip of `hand` opened.
    Release(Hand, Release),
    /// The throw cannot act: its toggle is off, the mode has no free hands, or the bindings stopped
    /// injecting.
    StandDown,
}

/// Why the weapon cannot prime a grenade.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No grenades are left.
    NoGrenades,
    /// The character may not use grenades now (the game's permission is off).
    NotAllowed,
    /// The weapon already holds a primed grenade of its own throw.
    Busy,
}

impl Refusal {
    pub const fn label(self) -> &'static str {
        match self {
            Refusal::NoGrenades => "no grenades left",
            Refusal::NotAllowed => "grenades not allowed now",
            Refusal::Busy => "the game's own throw is under way",
        }
    }
}

/// The grenade weapon as its update begins.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct WeaponView {
    /// The weapon holds a live primed grenade.
    pub primed: bool,
    /// The primed grenade has its physics body, so a launch takes effect.
    pub physics_ready: bool,
    /// Why a grenade cannot be primed now, if it cannot.
    pub refusal: Option<Refusal>,
}

/// What to do to the weapon before its update.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum WeaponAction {
    None,
    /// Ask the update to prime a grenade.
    Prime,
    /// Ask the update to fire, which launches the primed grenade with this release.
    Fire(Release),
    /// Put the primed grenade back.
    Cancel,
}

/// A step's result worth reporting.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Outcome {
    /// A grenade is in the hand, its fuse running.
    Primed,
    Refused(Refusal),
    /// The game's ammunition pool had no room for another grenade.
    PoolFull,
    Thrown(Release),
    /// The held grenade is gone before its throw: its fuse ran out in the hand, or the game took
    /// it back.
    Gone,
    /// The held grenade went back to the inventory.
    Cancelled,
}

impl Outcome {
    pub fn label(self) -> String {
        match self {
            Outcome::Primed => "primed".to_owned(),
            Outcome::Refused(r) => format!("refused: {}", r.label()),
            Outcome::PoolFull => "refused: no room for another grenade".to_owned(),
            Outcome::Thrown(r) => format!("thrown at {:.1} m/s", r.velocity.length()),
            Outcome::Gone => "gone from the hand before the throw".to_owned(),
            Outcome::Cancelled => "put back".to_owned(),
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
            Stage::Priming {
                hand,
                release: None,
            },
            InputEvent::Release(h, release),
        ) if h == hand => Stage::Priming {
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
        (Stage::Priming { .. } | Stage::Held { .. }, InputEvent::StandDown) => Stage::Cancelling,
        (stage, _) => stage,
    }
}

/// Decide what the weapon's update is asked to do.
pub fn before_update(stage: Stage, view: WeaponView) -> (Stage, WeaponAction, Option<Outcome>) {
    match stage {
        Stage::Idle => (Stage::Idle, WeaponAction::None, None),
        Stage::Wanted { hand, release } => {
            let refusal = if view.primed {
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
                None => (Stage::Priming { hand, release }, WeaponAction::Prime, None),
            }
        }
        // Resolved by the previous update's `after_update`; seen here only if that update never
        // returned through the hook.
        Stage::Priming { hand, release } | Stage::Held { hand, release } => {
            if !view.primed {
                return (Stage::Idle, WeaponAction::None, Some(Outcome::Gone));
            }
            match release {
                Some(release) if view.physics_ready => (
                    Stage::Throwing { hand, release },
                    WeaponAction::Fire(release),
                    None,
                ),
                _ => (Stage::Held { hand, release }, WeaponAction::None, None),
            }
        }
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

/// Settle the stage once the weapon's update has run: `primed` is whether the weapon holds a live
/// primed grenade now.
pub fn after_update(stage: Stage, primed: bool) -> (Stage, Option<Outcome>) {
    match stage {
        Stage::Priming { hand, release } if primed => {
            (Stage::Held { hand, release }, Some(Outcome::Primed))
        }
        Stage::Priming { .. } => (Stage::Idle, Some(Outcome::PoolFull)),
        Stage::Throwing { release, .. } if !primed => (Stage::Idle, Some(Outcome::Thrown(release))),
        // The update did not launch it (the weapon skipped its fire this tick): try again.
        Stage::Throwing { hand, release } => (
            Stage::Held {
                hand,
                release: Some(release),
            },
            None,
        ),
        stage => (stage, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: Release = Release {
        velocity: Vec3::new(0.0, 3.0, -8.0),
    };

    fn view(primed: bool, physics_ready: bool) -> WeaponView {
        WeaponView {
            primed,
            physics_ready,
            refusal: None,
        }
    }

    /// Run one weapon update: before, then after with the grenade the action leaves.
    fn update(stage: Stage, v: WeaponView) -> (Stage, WeaponAction, Vec<Outcome>) {
        let (stage, action, first) = before_update(stage, v);
        let primed = match action {
            WeaponAction::Prime => true,
            WeaponAction::Fire(_) | WeaponAction::Cancel => false,
            WeaponAction::None => v.primed,
        };
        let (stage, second) = after_update(stage, primed);
        (stage, action, first.into_iter().chain(second).collect())
    }

    #[test]
    fn a_grab_primes_and_a_release_throws_once_the_physics_exists() {
        let s = on_input(Stage::Idle, InputEvent::Grab(Hand::Right));
        let (s, action, out) = update(s, view(false, false));
        assert_eq!(action, WeaponAction::Prime);
        assert_eq!(out, vec![Outcome::Primed]);
        assert_eq!(s.hand(), Some(Hand::Right));

        let s = on_input(s, InputEvent::Release(Hand::Right, RELEASE));
        // The body does not exist yet: nothing is fired.
        let (s, action, out) = update(s, view(true, false));
        assert_eq!(action, WeaponAction::None);
        assert!(out.is_empty());

        let (s, action, out) = update(s, view(true, true));
        assert_eq!(action, WeaponAction::Fire(RELEASE));
        assert_eq!(out, vec![Outcome::Thrown(RELEASE)]);
        assert_eq!(s, Stage::Idle);
    }

    #[test]
    fn a_release_before_the_prime_never_fires_in_the_priming_update() {
        let s = on_input(Stage::Idle, InputEvent::Grab(Hand::Left));
        let s = on_input(s, InputEvent::Release(Hand::Left, RELEASE));
        let (s, action, _) = update(s, view(false, false));
        assert_eq!(action, WeaponAction::Prime);
        let (_, action, _) = update(s, view(true, true));
        assert_eq!(action, WeaponAction::Fire(RELEASE));
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
    fn a_grenade_held_too_long_is_gone() {
        let s = Stage::Held {
            hand: Hand::Right,
            release: None,
        };
        let (s, action, out) = update(s, view(false, false));
        assert_eq!(action, WeaponAction::None);
        assert_eq!(out, vec![Outcome::Gone]);
        assert_eq!(s, Stage::Idle);
    }

    #[test]
    fn a_stand_down_puts_the_grenade_back_or_forgets_the_request() {
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
        assert_eq!(on_input(Stage::Idle, InputEvent::StandDown), Stage::Idle);
    }

    #[test]
    fn a_refused_prime_returns_to_idle() {
        let s = on_input(Stage::Idle, InputEvent::Grab(Hand::Right));
        let (s, action, out) = update(
            s,
            WeaponView {
                primed: false,
                physics_ready: false,
                refusal: Some(Refusal::NoGrenades),
            },
        );
        assert_eq!(action, WeaponAction::None);
        assert_eq!(out, vec![Outcome::Refused(Refusal::NoGrenades)]);
        assert_eq!(s, Stage::Idle);
        // The game's own throw holds a grenade: the grab leaves it alone.
        let s = on_input(Stage::Idle, InputEvent::Grab(Hand::Right));
        let (_, action, out) = update(s, view(true, true));
        assert_eq!(action, WeaponAction::None);
        assert_eq!(out, vec![Outcome::Refused(Refusal::Busy)]);
    }

    #[test]
    fn a_full_pool_is_reported() {
        let s = on_input(Stage::Idle, InputEvent::Grab(Hand::Right));
        let (s, action, _) = before_update(s, view(false, false));
        assert_eq!(action, WeaponAction::Prime);
        assert_eq!(
            after_update(s, false),
            (Stage::Idle, Some(Outcome::PoolFull))
        );
    }

    #[test]
    fn a_fire_the_weapon_skipped_is_retried() {
        let s = Stage::Held {
            hand: Hand::Right,
            release: Some(RELEASE),
        };
        let (s, action, _) = before_update(s, view(true, true));
        assert_eq!(action, WeaponAction::Fire(RELEASE));
        let (s, out) = after_update(s, true);
        assert_eq!(out, None);
        let (_, action, _) = before_update(s, view(true, true));
        assert_eq!(action, WeaponAction::Fire(RELEASE));
    }
}
