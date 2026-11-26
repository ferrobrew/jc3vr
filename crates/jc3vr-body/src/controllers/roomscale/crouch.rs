//! Crouching: whether the character counts as crouched, and the capsule.
//!
//! The body folds with the head through the procedural body's hips
//! ([`crate::controllers::body::procedural`]). The capsule is the game's own crouching proxy state,
//! requested through the character's pending proxy state, which the engine applies on the
//! simulation thread after the state tasks run. The request is made only while the pending state is
//! the character's default or already the crouch, so the game's own proxy states (swimming, the
//! parachute, vehicles, ragdoll) always win, and it is released only when a ray up from the feet
//! finds the headroom to stand.

use jc3gi::physics::ProxyState;

/// Whether the character counts as crouched, with hysteresis on the head's drop.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct CrouchTracker {
    crouched: bool,
}

impl CrouchTracker {
    pub const fn new() -> Self {
        Self { crouched: false }
    }

    /// Fold in the head's `drop` below the standing eye height (metres): crouched once it passes
    /// `enter_m`, standing once it is back above `exit_m`.
    pub fn update(&mut self, drop: f32, enter_m: f32, exit_m: f32) -> bool {
        let exit_m = exit_m.min(enter_m);
        self.crouched = if self.crouched {
            drop > exit_m
        } else {
            drop > enter_m
        };
        self.crouched
    }
}

/// What the capsule request writes this tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct CapsuleStep {
    /// The value to write to the pending proxy state, if any.
    pub write: Option<i32>,
    /// Whether the mod holds the crouch request after the write.
    pub owned: bool,
}

/// Decide the capsule's pending proxy state. `want` is whether the character should be in the crouch
/// capsule, `owned` whether the mod's request is standing, and `pending` and `default` the
/// character's pending and default proxy states.
pub fn capsule_step(want: bool, owned: bool, pending: i32, default: i32) -> CapsuleStep {
    let crouching = ProxyState::CROUCHING as i32;
    match (want, pending) {
        // The game wants another state: stand down without touching it.
        (_, p) if p != default && p != crouching => CapsuleStep {
            write: None,
            owned: false,
        },
        // Re-asserted every tick, since a graph state's exit may restore the default mid-crouch.
        (true, p) if p == default => CapsuleStep {
            write: Some(crouching),
            owned: true,
        },
        (true, _) => CapsuleStep { write: None, owned },
        (false, p) if owned && p == crouching => CapsuleStep {
            write: Some(default),
            owned: false,
        },
        (false, _) => CapsuleStep {
            write: None,
            owned: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: i32 = ProxyState::PLAYER_STANDING as i32;
    const CROUCH: i32 = ProxyState::CROUCHING as i32;
    const SWIM: i32 = ProxyState::SWIMMING as i32;

    #[test]
    fn the_tracker_has_hysteresis() {
        let mut t = CrouchTracker::default();
        assert!(!t.update(0.4, 0.45, 0.3));
        assert!(t.update(0.5, 0.45, 0.3));
        assert!(t.update(0.35, 0.45, 0.3));
        assert!(!t.update(0.25, 0.45, 0.3));
    }

    #[test]
    fn a_crouch_is_requested_over_the_default_and_re_asserted() {
        let step = capsule_step(true, false, DEFAULT, DEFAULT);
        assert_eq!(step.write, Some(CROUCH));
        assert!(step.owned);
        let held = capsule_step(true, true, CROUCH, DEFAULT);
        assert_eq!(held.write, None);
        assert!(held.owned);
        let restored_by_the_graph = capsule_step(true, true, DEFAULT, DEFAULT);
        assert_eq!(restored_by_the_graph.write, Some(CROUCH));
    }

    #[test]
    fn the_games_own_states_win() {
        let step = capsule_step(true, true, SWIM, DEFAULT);
        assert_eq!(step.write, None);
        assert!(!step.owned);
        let release = capsule_step(false, true, SWIM, DEFAULT);
        assert_eq!(release.write, None);
    }

    #[test]
    fn standing_releases_only_the_mods_own_crouch() {
        let release = capsule_step(false, true, CROUCH, DEFAULT);
        assert_eq!(release.write, Some(DEFAULT));
        assert!(!release.owned);
        let not_ours = capsule_step(false, false, CROUCH, DEFAULT);
        assert_eq!(not_ours.write, None);
    }
}
