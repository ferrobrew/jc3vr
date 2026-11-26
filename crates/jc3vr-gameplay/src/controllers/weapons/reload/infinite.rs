//! Unlimited ammunition under the true reload: the magazine never empties.
//!
//! The game's own unlimited ammunition makes only the reload free: a shot still empties the
//! magazine ([`WeaponBase::ConsumeAmmoBase`]), and the game's reload then refills it from nothing.
//! The true reload stops that reload, so with unlimited ammunition a gun in the hands would run dry
//! and stay dry until the player reloaded it by hand. Here instead a seated magazine is kept full:
//! each shot is put back at once ([`after_shot`]), and each input tick tops up a magazine that
//! came in short, such as a gun taken from a character ([`top_up`]). Reloading becomes optional:
//! the magazine can still be ejected or pulled, the pouch still gives one while the magazine is
//! out, and a seated magazine, from the pouch or the world, is full from then on. The chamber is
//! left to the reload: a bolt or a pump is still worked after each shot, and a gun that ran dry
//! before the ammunition became unlimited still has its action worked.
//!
//! Whether the rounds are unlimited is [`engine::unlimited`], the game's rule read for the local
//! player, so that it follows the game's unlimited-ammunition option for every gun in either hand,
//! whoever the gun belonged to.
//!
//! [`WeaponBase::ConsumeAmmoBase`]: jc3gi::equipment::weapon::WeaponBase::ConsumeAmmoBase

use jc3gi::equipment::weapon::WeaponBase;

use crate::controllers::weapons::reload::engine::{self, Reading};

/// The native loaded total that keeps the magazine of a weapon read as `reading` full, when it is
/// short of full and should be: unlimited rounds, a magazine in the gun, a magazine size, and a
/// weapon the true reload acts on. The chamber's rounds ride on top of a full magazine, as the
/// native total counts them.
pub(super) fn full_total(reading: Reading) -> Option<i32> {
    (reading.unlimited
        && !reading.out
        && !reading.exempt
        && reading.capacity > 0
        && reading.magazine_rounds() < reading.capacity)
        .then(|| reading.capacity + reading.chambers.max(0))
}

/// The native total to leave after a shot of the weapon at `address` that left `total`: full
/// again when its rounds are unlimited, otherwise `total`. The chamber partition follows the full
/// magazine; an emptied chamber stays empty for the action to feed.
///
/// # Safety
///
/// The weapon must be live, on its update thread, inside the shot.
pub(super) unsafe fn after_shot(address: usize, total: i32) -> i32 {
    let Some(weapon) = (unsafe { (address as *const WeaponBase).as_ref() }) else {
        return total;
    };
    let reading = Reading {
        magazine: total,
        ..unsafe { engine::read(weapon) }
    };
    full_total(reading).map_or(total, |full| engine::refill_partition(address, full))
}

/// Top up the magazine of the weapon at `address`, read this tick as `reading`, when its rounds are
/// unlimited and it is short. Returns the reading after the top-up.
///
/// # Safety
///
/// The weapon must be owned and live on the input thread, outside the world update.
pub(super) unsafe fn top_up(address: usize, reading: Reading) -> Reading {
    let Some(full) = full_total(reading) else {
        return reading;
    };
    if !unsafe { engine::set_loaded(address, full) } {
        return reading;
    }
    Reading {
        magazine: full,
        ..reading
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading() -> Reading {
        Reading {
            class: 0,
            name_hash: 0,
            capacity: 30,
            magazine: 12,
            chambers: 1,
            out: false,
            exempt: false,
            unlimited: true,
        }
    }

    #[test]
    fn an_unlimited_magazine_is_filled_with_its_chamber_on_top() {
        assert_eq!(full_total(reading()), Some(31));
        // A gun taken empty, its chamber empty too.
        assert_eq!(
            full_total(Reading {
                magazine: 0,
                chambers: 0,
                ..reading()
            }),
            Some(30)
        );
    }

    #[test]
    fn a_full_magazine_is_left_alone() {
        assert_eq!(
            full_total(Reading {
                magazine: 31,
                ..reading()
            }),
            None
        );
        assert_eq!(
            full_total(Reading {
                magazine: 30,
                chambers: 0,
                ..reading()
            }),
            None
        );
    }

    #[test]
    fn only_a_seated_magazine_of_unlimited_rounds_is_filled() {
        for short in [
            Reading {
                unlimited: false,
                ..reading()
            },
            Reading {
                out: true,
                ..reading()
            },
            Reading {
                exempt: true,
                ..reading()
            },
            Reading {
                capacity: -1,
                ..reading()
            },
            Reading {
                capacity: 0,
                ..reading()
            },
        ] {
            assert_eq!(full_total(short), None, "{short:?}");
        }
    }

    #[test]
    fn a_single_round_launcher_is_loaded_again_after_its_shot() {
        let launcher = Reading {
            capacity: 1,
            magazine: 0,
            chambers: 0,
            ..reading()
        };
        assert_eq!(full_total(launcher), Some(1));
    }

    #[test]
    fn a_single_pistol_fills_to_its_own_half_magazine() {
        // The reading's capacity is the single pistol's half of the tuned size.
        let pistol = Reading {
            capacity: 9,
            magazine: 3,
            chambers: 1,
            ..reading()
        };
        assert_eq!(full_total(pistol), Some(10));
    }
}
