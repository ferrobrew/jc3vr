//! The reload's game-facing side, read and written on the threads that update the weapons: the
//! stop on the game's own reload starts, the per-gun fire hold, and the shots the input tick
//! learns from. Nothing here touches the VR state; the input tick publishes what the detours need
//! into [`SHARED`], a short lock that the detours take only for the local player's weapons.
//!
//! - The stop ([`stops_reload`], from the detour on [`Character::StartReloadingWieldingWeapon`]):
//!   every animated reload the game starts goes through that one call, from the reload button, the
//!   automatic reload of an empty or magazine-less gun, the predicted reload, a reserve refill, an
//!   animation's request, and the launchers' and the sniper rifles' own reloads. For the local
//!   player, while the input tick reloads either hand's weapon by hand and has done so recently, the
//!   start is refused, so no reload act is ever queued and the game's reload state machine is
//!   never entered.
//! - The fire hold ([`holds_fire`], from the ammunition check in
//!   [`crate::controllers::weapons::empty`] and the gun update detour in the reload's `engine`): a
//!   gun whose chamber is empty, whose magazine is out, or that sits in the belt pouch after a push
//!   reloaded it cannot fire. The input tick already keeps its trigger from the game; the check
//!   clears the gun's shot as well, so that a buffered shot cannot slip through.
//! - The shots ([`on_shot`], from the per-shot seam in [`crate::controllers::weapons::fire`]): each
//!   shot of a weapon reloaded by hand, the hand it came from, and the rounds left, which the input
//!   tick folds into that weapon's chambers and split magazines.
//!
//! [`Character::StartReloadingWieldingWeapon`]: jc3gi::character::character::Character::StartReloadingWieldingWeapon

use std::time::{Duration, Instant};

use jc3vr_core::published::{Stamp, Window};
use jc3vr_input::frame::Hand;
use parking_lot::Mutex;

use crate::controllers::weapons::reload::tuning::RELOAD;

/// What the input tick publishes for the detours.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(super) struct Published {
    /// The address of the weapon of each hand's gun that the input tick reloads by hand, or zero,
    /// by [`Hand::index`].
    pub weapons: [usize; 2],
    /// Each hand's weapon cycles its own bolt once the trigger is let go, by [`Hand::index`].
    pub(crate) self_cycling: [bool; 2],
    /// The hands whose guns may not fire, by [`Hand::index`].
    pub held: [bool; 2],
}

/// A shot of a weapon reloaded by hand.
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
            shared.at = Some(Stamp::now());
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

/// Whether to refuse a reload start for `character`, the local player's when it equals `local` (the
/// address the input tick publishes) or `is_local` (the character's own local flag): the weapon is
/// reloaded by hand. Counts a refusal.
pub(super) fn stops_reload(character: usize, local: usize, is_local: bool) -> bool {
    let mut shared = SHARED.lock();
    let decision = decide(character, local, is_local, shared.fresh());
    match decision {
        StartDecision::Refuse => {
            shared.refused += 1;
            true
        }
        StartDecision::NotLocal => false,
        StartDecision::Pass(why) => {
            if shared
                .allowed_logged
                .is_none_or(|at| at.elapsed() >= ALLOWED_LOG_INTERVAL)
            {
                // A start let through means the game's own reload runs for the local player's
                // weapon; the log is the only trace of it, since its act is not the mod's.
                shared.allowed_logged = Some(Instant::now());
                drop(shared);
                tracing::info!(target: "controllers", why = why.label(), "reload: the game's own reload starts for the local player");
            }
            false
        }
    }
}

/// What the stop does with a reload start.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum StartDecision {
    Refuse,
    /// Another character's start, which is the game's.
    NotLocal,
    /// The local player's start, let through.
    Pass(PassReason),
}

/// Why the stop lets the local player's reload start through.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum PassReason {
    /// The input tick has not published for longer than the stale timeout, or at all.
    Stale,
    /// The input tick reloads no weapon by hand.
    NoWeapon,
}

impl PassReason {
    const fn label(self) -> &'static str {
        match self {
            PassReason::Stale => "the input tick has not published lately",
            PassReason::NoWeapon => "no weapon is reloaded by hand",
        }
    }
}

/// The stop's decision for `character`, local by the published address `local` or by its own
/// flag `is_local`, with the input tick's fresh publication `fresh`.
fn decide(
    character: usize,
    local: usize,
    is_local: bool,
    fresh: Option<Published>,
) -> StartDecision {
    if character == 0 || !(is_local || character == local) {
        return StartDecision::NotLocal;
    }
    match fresh {
        None => StartDecision::Pass(PassReason::Stale),
        Some(p) if p.weapons.iter().all(|&w| w == 0) => StartDecision::Pass(PassReason::NoWeapon),
        Some(_) => StartDecision::Refuse,
    }
}

/// Whether the gun of `weapon` in `hand` may not fire now.
pub(super) fn holds_fire(weapon: usize, hand: Hand) -> bool {
    let shared = SHARED.lock();
    shared.fresh().is_some_and(|p| holds(&p, weapon, hand))
}

/// Whether this hand currently publishes ownership of the weapon, before the stale timeout.
pub(super) fn tracks(weapon: usize, hand: Hand) -> bool {
    SHARED
        .lock()
        .fresh()
        .is_some_and(|p| weapon != 0 && p.weapons[hand.index()] == weapon)
}

/// Whether the bolt of `weapon` is cycled on the trigger's release, in place of the game's bolt
/// act.
pub(super) fn self_cycling(weapon: usize) -> bool {
    SHARED
        .lock()
        .fresh()
        .is_some_and(|p| cycles_on_release(&p, weapon))
}

/// Record a shot of `weapon` from `hand`, when it is a weapon reloaded by hand.
pub(super) fn on_shot(weapon: usize, hand: Hand, magazine: i32) {
    let mut shared = SHARED.lock();
    if shared.fresh().is_none_or(|p| !reloads(&p, weapon)) || shared.shots.len() >= MAX_SHOTS {
        return;
    }
    shared.shots.push(Shot {
        weapon,
        hand,
        magazine,
    });
}

/// Whether `published` holds the gun of `weapon` in `hand` from firing.
fn holds(published: &Published, weapon: usize, hand: Hand) -> bool {
    weapon != 0 && published.weapons[hand.index()] == weapon && published.held[hand.index()]
}

/// Whether `published` has `weapon`'s bolt cycled on the trigger's release.
fn cycles_on_release(published: &Published, weapon: usize) -> bool {
    weapon != 0
        && Hand::BOTH.into_iter().any(|hand| {
            published.weapons[hand.index()] == weapon && published.self_cycling[hand.index()]
        })
}

/// Whether `published` reloads `weapon` by hand.
fn reloads(published: &Published, weapon: usize) -> bool {
    weapon != 0 && published.weapons.contains(&weapon)
}

/// More shots than any weapon fires between two input ticks.
const MAX_SHOTS: usize = 64;
/// The shortest interval between two logs of a reload start the stop let through.
const ALLOWED_LOG_INTERVAL: Duration = Duration::from_secs(1);

struct Shared {
    published: Published,
    at: Option<Stamp>,
    shots: Vec<Shot>,
    refused: u64,
    /// When a reload start the stop let through was last logged.
    allowed_logged: Option<Instant>,
}

impl Shared {
    /// The published state, while it is newer than the tuning's `gate.stale_ms`
    /// ([`GateTuning::stale_ms`](crate::controllers::weapons::reload::tuning::GateTuning::stale_ms)).
    fn fresh(&self) -> Option<Published> {
        Window::from_millis(RELOAD.get().gate.stale_ms)
            .holds(self.at, Stamp::now())
            .then_some(self.published)
    }
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    published: Published {
        weapons: [0; 2],
        self_cycling: [false; 2],
        held: [false; 2],
    },
    at: None,
    shots: Vec::new(),
    refused: 0,
    allowed_logged: None,
});

#[cfg(test)]
mod tests {
    use super::*;

    const WIELDED: usize = 0x1000;
    const SECOND: usize = 0x2000;

    fn two_weapons() -> Published {
        Published {
            weapons: [SECOND, WIELDED],
            self_cycling: [false, false],
            held: [false, true],
        }
    }

    #[test]
    fn each_hand_is_held_for_its_own_weapon() {
        let p = two_weapons();
        assert!(holds(&p, WIELDED, Hand::Right));
        assert!(!holds(&p, SECOND, Hand::Left));
        assert!(!holds(&p, WIELDED, Hand::Left));
        assert!(!holds(&p, 0, Hand::Left));
    }

    #[test]
    fn a_self_cycling_bolt_is_its_own_weapons() {
        let p = Published {
            self_cycling: [false, true],
            ..two_weapons()
        };
        assert!(cycles_on_release(&p, WIELDED));
        assert!(!cycles_on_release(&p, SECOND));
        assert!(!cycles_on_release(&p, 0));
        assert!(!cycles_on_release(&Published::default(), WIELDED));
    }

    #[test]
    fn the_local_player_s_start_is_refused_by_its_address_or_its_own_flag() {
        let fresh = Some(two_weapons());
        assert_eq!(decide(7, 7, false, fresh), StartDecision::Refuse);
        // The published address can lag the character the start names; its own flag still marks
        // it the player's.
        assert_eq!(decide(7, 9, true, fresh), StartDecision::Refuse);
        assert_eq!(decide(7, 9, false, fresh), StartDecision::NotLocal);
        assert_eq!(decide(0, 0, true, fresh), StartDecision::NotLocal);
        assert_eq!(
            decide(7, 7, false, None),
            StartDecision::Pass(PassReason::Stale)
        );
        assert_eq!(
            decide(7, 7, false, Some(Published::default())),
            StartDecision::Pass(PassReason::NoWeapon)
        );
    }

    #[test]
    fn shots_of_either_weapon_are_kept() {
        let p = two_weapons();
        assert!(reloads(&p, WIELDED));
        assert!(reloads(&p, SECOND));
        assert!(!reloads(&p, 0x3000));
        assert!(!reloads(&Published::default(), 0));
    }
}
