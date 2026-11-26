//! The game's reload of a weapon armed in a hand that the true reload leaves to the game.
//!
//! Every animated reload the game starts reloads the wielded weapon
//! ([`Character::StartReloadingWieldingWeapon`]), and an armed weapon's own ammunition check asks
//! for exactly that, which the per-hand tick refuses for the local player
//! ([`crate::controllers::weapons::hold::tick`]). So an armed weapon whose reload is the game's
//! (one the plan gives no hand reload, such as a special weapon, or any weapon with the true
//! reload off) would never reload at all: the weapon beside the wielded one, and every heavy
//! weapon, since the table never puts one in its slot and so never wields it.
//!
//! Such a weapon is refilled through the game's own round move instead ([`WeaponBase::Reload`],
//! the same call a seated magazine loads through) once its magazine has stayed empty for
//! [`REFILL_DELAY`], about as long as the game's reload takes, while its reserve has rounds or
//! the ammunition is unlimited. The reload's own rules (the reserve, unlimited ammunition, the
//! upgrades, the HUD) all apply. A weapon without a magazine limit, as every heavy weapon is,
//! fires straight from the reserve and is never refilled; nor is one of the game's untimed or
//! unlimited reloads, whose own timer or kind looks after it.
//!
//! [`Character::StartReloadingWieldingWeapon`]: jc3gi::character::character::Character::StartReloadingWieldingWeapon

use std::time::{Duration, Instant};

use jc3gi::{character::character::Character, equipment::weapon::WeaponBase};
use jc3vr_input::frame::Hand;
use parking_lot::Mutex;

use crate::controllers::weapons::{
    config::WeaponSettings,
    dual_wield::single,
    engine::reserve,
    hold::armed,
    reload::{
        engine,
        plan::{self, WeaponFacts},
        tuning::Reload,
        wells,
    },
};

/// How long an armed weapon's magazine stays empty before the game's reload refills it.
pub(crate) const REFILL_DELAY: Duration = Duration::from_millis(1500);

/// Refill each armed weapon whose reload is the game's once its magazine has stayed empty for
/// [`REFILL_DELAY`]. `reload_acts` is whether the true reload acts this tick.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub(crate) unsafe fn refill_armed(owner: &Character, config: &WeaponSettings, reload_acts: bool) {
    let reload = Reload::of(&config.weapons.reload);
    let now = Instant::now();
    let mut pending = PENDING.lock();
    for hand in Hand::BOTH {
        let slot = &mut pending[hand.index()];
        let Some(mut handle) = (unsafe { armed::weapon_handle(owner, hand) }) else {
            *slot = None;
            continue;
        };
        let address = handle.as_ptr() as usize;
        // SAFETY: the armed weapon's slot or carry-set source keeps it alive for this borrow.
        let weapon = unsafe { handle.as_mut() };
        let facts = unsafe { facts_of(weapon, &reload, reload_acts) };
        let (next, due) = step(*slot, address, wants_refill(facts), now);
        *slot = next;
        if due {
            let refilled = unsafe { refill(weapon) };
            tracing::info!(
                target: "controllers",
                hand = hand.label(),
                refilled,
                magazine = weapon.m_MagazineAmmo,
                "reload: the game's reload refilled a weapon armed in a hand"
            );
        }
    }
}

/// Forget the pending refills. Called when the weapon handling stands down.
pub(crate) fn stand_down() {
    *PENDING.lock() = [None; 2];
}

/// What decides whether an armed weapon is refilled.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct ArmedFacts {
    /// The true reload reloads it by hand this tick.
    by_hand: bool,
    /// The magazine size; negative or zero means no limit.
    capacity: i32,
    magazine: i32,
    /// The game's magazine-out flag: a magazine the player took out is the true reload's.
    out: bool,
    /// One of the game's untimed or unlimited reloads.
    exempt: bool,
    /// The reserve has rounds, or the ammunition is unlimited.
    stocked: bool,
}

/// A weapon whose magazine ran empty, waiting out [`REFILL_DELAY`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Pending {
    address: usize,
    since: Instant,
}

static PENDING: Mutex<[Option<Pending>; 2]> = Mutex::new([None; 2]);

/// Read `weapon`'s facts.
///
/// # Safety
///
/// `weapon` must be live, on the game thread.
unsafe fn facts_of(weapon: &WeaponBase, reload: &Reload, reload_acts: bool) -> ArmedFacts {
    let reading = unsafe { engine::read(weapon) };
    let by_hand = reload.enabled
        && reload_acts
        && plan::plan(
            WeaponFacts {
                class: reading.class,
                capacity: reading.capacity,
                exempt: reading.exempt,
            },
            wells::lookup(reading.name_hash),
            reload.well_offset_m,
            reload.action.default_travel_m,
        )
        .is_some();
    ArmedFacts {
        by_hand,
        capacity: reading.capacity,
        magazine: reading.magazine,
        out: reading.out,
        exempt: reading.exempt,
        stocked: reading.unlimited || unsafe { reserve(weapon) } > 0,
    }
}

/// Whether a weapon with `facts` is one the game's reload would refill now: left to the game,
/// with a magazine limit, its magazine in and empty, and rounds to fill it from.
fn wants_refill(facts: ArmedFacts) -> bool {
    !facts.by_hand
        && !facts.exempt
        && facts.capacity > 0
        && facts.magazine <= 0
        && !facts.out
        && facts.stocked
}

/// Advance the refill of the weapon at `address`: start waiting when it wants one, keep waiting
/// while it still does, and refill once [`REFILL_DELAY`] has passed. A different weapon, or one
/// no longer wanting a refill, starts over. Returns the next state and whether the refill is due.
fn step(
    pending: Option<Pending>,
    address: usize,
    wants: bool,
    now: Instant,
) -> (Option<Pending>, bool) {
    if !wants {
        return (None, false);
    }
    match pending.filter(|p| p.address == address) {
        Some(p) if now.saturating_duration_since(p.since) >= REFILL_DELAY => (None, true),
        Some(p) => (Some(p), false),
        None => (
            Some(Pending {
                address,
                since: now,
            }),
            false,
        ),
    }
}

/// Refill `weapon` through its own reload. Returns whether the magazine holds rounds after.
///
/// # Safety
///
/// `weapon` must be live and owned by the local player, on the game thread before the world
/// update.
unsafe fn refill(weapon: &mut WeaponBase) -> bool {
    unsafe { armed::release_fire(weapon) };
    unsafe { weapon.Reload() };
    // The game's reload fills a single pistol to the pair's size; it holds half.
    // SAFETY: `m_User` is a `weak_ptr<CCharacter>` whose first word is the character.
    if let Some(user) = unsafe { (weapon.m_User[0] as *mut Character).as_mut() } {
        unsafe { single::clamp_magazine(user, weapon) };
    }
    weapon.m_KeepReloading = false;
    weapon.m_MagazineAmmo > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_SPECIAL: ArmedFacts = ArmedFacts {
        by_hand: false,
        capacity: 10,
        magazine: 0,
        out: false,
        exempt: false,
        stocked: true,
    };

    #[test]
    fn an_empty_weapon_left_to_the_game_is_refilled() {
        assert!(wants_refill(EMPTY_SPECIAL));
    }

    #[test]
    fn a_heavy_weapon_without_a_magazine_limit_is_never_refilled() {
        for capacity in [-1, 0] {
            assert!(!wants_refill(ArmedFacts {
                capacity,
                ..EMPTY_SPECIAL
            }));
        }
    }

    #[test]
    fn the_true_reloads_own_weapons_and_the_games_exempt_reloads_are_left_alone() {
        assert!(!wants_refill(ArmedFacts {
            by_hand: true,
            ..EMPTY_SPECIAL
        }));
        assert!(!wants_refill(ArmedFacts {
            exempt: true,
            ..EMPTY_SPECIAL
        }));
        assert!(!wants_refill(ArmedFacts {
            out: true,
            ..EMPTY_SPECIAL
        }));
    }

    #[test]
    fn a_weapon_with_rounds_or_nothing_to_fill_from_waits() {
        assert!(!wants_refill(ArmedFacts {
            magazine: 1,
            ..EMPTY_SPECIAL
        }));
        assert!(!wants_refill(ArmedFacts {
            stocked: false,
            ..EMPTY_SPECIAL
        }));
    }

    #[test]
    fn the_refill_waits_out_the_delay_for_the_same_weapon() {
        let start = Instant::now();
        let (pending, due) = step(None, 1, true, start);
        assert!(!due);
        let (pending, due) = step(pending, 1, true, start + REFILL_DELAY / 2);
        assert!(!due);
        assert!(pending.is_some());
        let (pending, due) = step(pending, 1, true, start + REFILL_DELAY);
        assert!(due);
        assert_eq!(pending, None);
    }

    #[test]
    fn another_weapon_or_a_lapse_starts_the_wait_over() {
        let start = Instant::now();
        let (pending, _) = step(None, 1, true, start);
        let (pending, due) = step(pending, 2, true, start + REFILL_DELAY);
        assert!(!due, "a different weapon in the hand waits on its own");
        assert_eq!(pending.map(|p| p.address), Some(2));
        let (pending, due) = step(pending, 2, false, start + REFILL_DELAY * 2);
        assert!(!due);
        assert_eq!(pending, None);
        let (_, due) = step(pending, 2, true, start + REFILL_DELAY * 3);
        assert!(!due, "a weapon that wants a refill again waits again");
    }
}
