//! The reload's game-facing side, read and written on the threads that update the weapons: the
//! stop on the game's own reload starts, the per-gun fire hold, and the shots the input tick
//! learns from. Nothing here touches the VR state; the input tick publishes what the detours need
//! into [`SHARED`], a short lock that the detours take only for the local player's weapons.
//!
//! - The stop ([`stops_reload`], from the detour on [`Character::StartReloadingWieldingWeapon`]):
//!   every animated reload the game starts goes through that one call, from the reload button, the
//!   automatic reload of an empty or magazine-less gun, the predicted reload, a reserve refill, an
//!   animation's request, and the launchers' and the sniper rifles' own reloads. For the local
//!   player, while the input tick reloads the wielded weapon by hand and has done so recently, the
//!   start is refused, so no reload act is ever queued and the game's reload state machine is
//!   never entered. With the reload button left to the game, a start while the button is held goes
//!   through.
//! - The fire hold ([`holds_fire`], from the ammunition check in
//!   [`crate::controllers::weapons::empty`]): a gun whose chamber is empty, whose magazine is out,
//!   or that sits in the belt pouch cannot fire. The input tick already keeps its trigger from the
//!   game; the check clears the gun's shot as well, so that a buffered shot cannot slip through.
//! - The shots ([`on_shot`], from the per-shot seam in [`crate::controllers::weapons::fire`]): each
//!   shot of the reloaded weapon, the hand it came from, and the rounds left, which the input tick
//!   folds into the chamber and the split magazines.
//!
//! [`Character::StartReloadingWieldingWeapon`]: jc3gi::character::character::Character::StartReloadingWieldingWeapon

use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::controllers::frame::Hand;

/// What the input tick publishes for the detours.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(super) struct Published {
    /// The address of the weapon the input tick reloads by hand, or zero.
    pub weapon: usize,
    /// The reload button is left to the game's own reload.
    pub button_native: bool,
    /// The bolt or the pump is worked by hand after every shot, in place of the game's cycling.
    pub manual_action: bool,
    /// The hands whose guns may not fire, by [`Hand::index`].
    pub held: [bool; 2],
}

/// A shot of the reloaded weapon.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Shot {
    pub weapon: usize,
    pub hand: Hand,
    /// The weapon's magazine after the shot.
    pub magazine: i32,
}

/// Publish the input tick's state; `None` stops acting.
pub(super) fn publish(published: Option<Published>) {
    let mut shared = SHARED.lock();
    match published {
        Some(p) => {
            shared.published = p;
            shared.at = Some(Instant::now());
        }
        None => {
            shared.published = Published::default();
            shared.at = None;
            shared.shots.clear();
        }
    }
}

/// The shots since the last call.
pub(super) fn take_shots() -> Vec<Shot> {
    std::mem::take(&mut SHARED.lock().shots)
}

/// How many reload starts the stop has refused.
pub(super) fn refused() -> u64 {
    SHARED.lock().refused
}

/// Whether to refuse a reload start for `character` (the local player's when it equals `local`),
/// with the reload input at `reload_input`. Counts a refusal.
pub(super) fn stops_reload(character: usize, local: usize, reload_input: f32) -> bool {
    if character == 0 || character != local {
        return false;
    }
    let mut shared = SHARED.lock();
    let Some(p) = shared.fresh() else {
        return false;
    };
    let stop = decide_stop(p.weapon != 0, p.button_native, reload_input > 0.0);
    if stop {
        shared.refused += 1;
    }
    stop
}

/// Whether the gun of `weapon` in `hand` may not fire now.
pub(super) fn holds_fire(weapon: usize, hand: Hand) -> bool {
    let shared = SHARED.lock();
    shared
        .fresh()
        .is_some_and(|p| p.weapon == weapon && p.held[hand.index()])
}

/// Whether the game's own cycling of `weapon`'s bolt or pump is suppressed.
pub(super) fn manual_action(weapon: usize) -> bool {
    SHARED
        .lock()
        .fresh()
        .is_some_and(|p| p.weapon == weapon && p.manual_action)
}

/// Record a shot of `weapon` from `hand`, when it is the reloaded weapon.
pub(super) fn on_shot(weapon: usize, hand: Hand, magazine: i32) {
    let mut shared = SHARED.lock();
    if shared.fresh().is_none_or(|p| p.weapon != weapon) || shared.shots.len() >= MAX_SHOTS {
        return;
    }
    shared.shots.push(Shot {
        weapon,
        hand,
        magazine,
    });
}

/// Whether to stop a reload start: the weapon is reloaded by hand, unless the reload button is
/// left to the game and held.
pub(super) fn decide_stop(reloaded_by_hand: bool, button_native: bool, button_held: bool) -> bool {
    reloaded_by_hand && !(button_native && button_held)
}

/// How long the input tick's state stands without a refresh. Past it the detours stop acting, so
/// a stalled input tick hands the weapon back to the game.
const STALE: Duration = Duration::from_millis(500);
/// More shots than any weapon fires between two input ticks.
const MAX_SHOTS: usize = 64;

struct Shared {
    published: Published,
    at: Option<Instant>,
    shots: Vec<Shot>,
    refused: u64,
}

impl Shared {
    fn fresh(&self) -> Option<Published> {
        self.at
            .filter(|at| at.elapsed() < STALE)
            .map(|_| self.published)
    }
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    published: Published {
        weapon: 0,
        button_native: false,
        manual_action: false,
        held: [false; 2],
    },
    at: None,
    shots: Vec::new(),
    refused: 0,
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reload_start_is_stopped_unless_the_button_is_left_to_the_game() {
        assert!(decide_stop(true, false, false));
        assert!(decide_stop(true, false, true));
        assert!(decide_stop(true, true, false));
        assert!(!decide_stop(true, true, true));
        assert!(!decide_stop(false, false, false));
    }
}
