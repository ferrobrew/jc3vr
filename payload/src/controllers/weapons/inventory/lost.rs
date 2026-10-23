//! The lost-weapon return: a weapon lying in the world that the player lost rather than dropped
//! comes back to a free socket that fits it, else to the player's feet. The design is the
//! two-handed inventory's "Letting go" (`docs/mod/input/two-handed-inventory.md`).
//!
//! A weapon is lost when it is out of the world, below the sea's level by more than
//! `lost_depth_m`, which a weapon reaches only by sinking out of reach or falling through the
//! terrain; or when it was let go of at speed and is left behind, farther than `left_behind_m` from
//! the player. The player travels fast while their speed, measured each input tick, is at least
//! `lost_speed_mps` ([`live::set_fast`]), so a let-go then is one at speed. A weapon dropped at an
//! ordinary pace never comes back by itself, only out of the world.
//!
//! The judgement is pure ([`judge`]); the adapter ([`input_tick`]) tells the table of each loss
//! and makes the game follow it: a weapon returned to a socket is taken into the carry set there,
//! and one returned to the feet is taken into the carry set for a tick, placed on the body, and let
//! fall from it at the next.
//!
//! Everything here runs on the input tick, on the main thread outside the world update.

use std::time::Instant;

use glam::Vec3;
use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::controllers::{
    loadout::{Departure, LostWhy, ObjectAddress, Place, Socket, live},
    weapons::{
        hold::{equip, refs::Strong},
        inventory::{
            carry,
            engine::{self as inventory_engine, ObjectKey},
        },
    },
};

/// What the lost-weapon return judges by.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LostRules {
    /// The world's sea level, metres.
    pub sea_level_m: f32,
    /// How far below the sea's level a weapon is out of the world, metres.
    pub lost_depth_m: f32,
    /// How far from the player a weapon let go of at speed is left behind, metres.
    pub left_behind_m: f32,
}

/// A weapon the table has lying in the world, as the return judges it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Lying {
    pub object: ObjectAddress,
    pub departure: Departure,
    /// Where it is, when that could be read.
    pub at: Option<Vec3>,
}

/// Each of `lying` that is lost, and why, seen from the player at `player`. A weapon whose
/// position cannot be read is never lost, and one already brought back to the player is not lost
/// again for being left behind.
pub fn judge(lying: &[Lying], player: Vec3, rules: &LostRules) -> Vec<(ObjectAddress, LostWhy)> {
    lying
        .iter()
        .filter_map(|weapon| {
            let at = weapon.at.filter(|at| at.is_finite())?;
            if at.y < rules.sea_level_m - rules.lost_depth_m {
                return Some((weapon.object, LostWhy::OutOfWorld));
            }
            let behind =
                weapon.departure == Departure::AtSpeed && at.distance(player) > rules.left_behind_m;
            behind.then_some((weapon.object, LostWhy::LeftBehind))
        })
        .collect()
}

/// The player's speed from the last tick's position to `player` now, smoothed over a short time so
/// that a frame's jitter does not count as travelling fast.
pub fn speed(last: Option<(Vec3, f32)>, player: Vec3, dt_s: f32) -> f32 {
    let Some((from, smoothed)) = last else {
        return 0.0;
    };
    if !(dt_s > 0.0 && dt_s.is_finite() && player.is_finite() && from.is_finite()) {
        return smoothed;
    }
    let now = from.distance(player) / dt_s;
    let blend = (dt_s / SPEED_TIME_S).min(1.0);
    smoothed + (now - smoothed) * blend
}

/// The lost-weapon return's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct LostStatus {
    /// The player's measured speed, metres per second, and whether it counts as fast.
    pub speed_mps: f32,
    pub fast: bool,
    /// Weapons returned since the payload loaded: out of the world, left behind, and of those
    /// brought to the feet with no socket free.
    pub out_of_world: u64,
    pub left_behind: u64,
    pub to_feet: u64,
}

pub fn status() -> LostStatus {
    STATE.lock().status
}

/// What the adapter is handed each tick.
pub(crate) struct LostTick {
    pub enabled: bool,
    pub lost_speed_mps: f32,
    pub rules: LostRules,
    pub player: Vec3,
    /// The table's weapons lying in the world, with a strong reference to each that still lives.
    pub lying: Vec<(Lying, Option<Strong>)>,
}

/// Measure the player's speed, let fall the weapons brought to the feet last tick, and return
/// every weapon lost since.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(owner: &mut Character, tick: LostTick) {
    let now = Instant::now();
    let (fast, falling) = {
        let mut state = STATE.lock();
        let dt_s = state
            .last_at
            .map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32());
        let smoothed = speed(state.last, tick.player, dt_s);
        state.last = Some((tick.player, smoothed));
        state.last_at = Some(now);
        let fast = tick.enabled && smoothed >= tick.lost_speed_mps;
        state.status.speed_mps = smoothed;
        state.status.fast = fast;
        (fast, std::mem::take(&mut state.falling))
    };
    live::set_fast(fast);
    for object in falling {
        unsafe { let_fall(object) };
    }
    if !tick.enabled {
        return;
    }
    let judged: Vec<Lying> = tick.lying.iter().map(|(lying, _)| *lying).collect();
    let lost = judge(&judged, tick.player, &tick.rules);
    for (lying, strong) in tick.lying {
        let Some(&(object, why)) = lost.iter().find(|(object, _)| *object == lying.object) else {
            continue;
        };
        if let Some(strong) = strong {
            unsafe { bring_back(owner, object, why, strong) };
        }
    }
}

/// Forget the speed and the weapons waiting to fall: the inventory is off.
pub(crate) fn clear() {
    let mut state = STATE.lock();
    state.last = None;
    state.last_at = None;
    state.falling.clear();
    std::mem::drop(state);
    live::set_fast(false);
}

/// Whether `object` is a weapon brought to the feet, carried for the tick it takes to be placed on
/// the body before it falls: the carry set places it on its own socket meanwhile.
pub(crate) fn falling(object: ObjectKey) -> bool {
    STATE
        .lock()
        .falling
        .iter()
        .any(|falling| falling.get() == object)
}

/// How long the player's speed is smoothed over, seconds.
const SPEED_TIME_S: f32 = 0.25;

struct State {
    last: Option<(Vec3, f32)>,
    last_at: Option<Instant>,
    /// The weapons brought to the feet, carried until the next tick lets them fall.
    falling: Vec<ObjectAddress>,
    status: LostStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    last: None,
    last_at: None,
    falling: Vec::new(),
    status: LostStatus {
        speed_mps: 0.0,
        fast: false,
        out_of_world: 0,
        left_behind: 0,
        to_feet: 0,
    },
});

/// Return the lost weapon at `object`, kept alive by `strong`: tell the table, then take it into
/// the carry set on the socket the table returned it to, or on the body for a tick before it falls
/// at the feet.
unsafe fn bring_back(owner: &mut Character, object: ObjectAddress, why: LostWhy, strong: Strong) {
    // SAFETY: the strong reference keeps the weapon alive.
    let Some(weapon) = (unsafe { inventory_engine::weapon_of(&strong).as_ref() }) else {
        return;
    };
    let Some(weapon_type) = (unsafe { inventory_engine::weapon_type(weapon) }) else {
        return;
    };
    let Some(place) = live::lost(object, why) else {
        return;
    };
    let socket = match place {
        Place::Socket(socket) => socket,
        // With no socket free the weapon is carried on its type's first socket for the tick it
        // takes to be placed there, and then falls.
        Place::World(Departure::Returned) => live::body_preference(weapon_type)
            .first()
            .copied()
            .unwrap_or(Socket::BackOuter),
        Place::Hand(_) | Place::World(_) => return,
    };
    if let Err(refusal) = unsafe { carry::carry_loose(owner, strong, socket) } {
        tracing::info!(target: "controllers", %refusal, "inventory: a lost weapon could not be brought back");
        return;
    }
    let mut state = STATE.lock();
    match why {
        LostWhy::OutOfWorld => state.status.out_of_world += 1,
        LostWhy::LeftBehind => state.status.left_behind += 1,
    }
    if matches!(place, Place::World(_)) {
        state.status.to_feet += 1;
        state.falling.push(object);
    }
    std::mem::drop(state);
    tracing::info!(
        target: "controllers",
        why = why.label(),
        to = %place,
        "inventory: brought a lost weapon back to {place}"
    );
}

/// Let the weapon at `object`, carried on the body since the last tick, fall from it.
unsafe fn let_fall(object: ObjectAddress) {
    let Some(strong) = carry::release(object.get()) else {
        return;
    };
    // SAFETY: the strong reference keeps the weapon alive while it falls.
    let Some(weapon) = (unsafe { inventory_engine::weapon_of(&strong).as_mut() }) else {
        return;
    };
    unsafe { equip::drop_loose(weapon, 0.0) };
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use pretty_assertions::assert_eq;

    use super::*;

    const RULES: LostRules = LostRules {
        sea_level_m: 0.0,
        lost_depth_m: 10.0,
        left_behind_m: 30.0,
    };

    fn object(n: usize) -> ObjectAddress {
        ObjectAddress::new(NonZeroUsize::MIN.saturating_add(n * 0x10))
    }

    fn lying(n: usize, departure: Departure, at: Vec3) -> Lying {
        Lying {
            object: object(n),
            departure,
            at: Some(at),
        }
    }

    #[test]
    fn a_weapon_below_the_world_is_lost_however_it_left() {
        let lost = judge(
            &[
                lying(1, Departure::Dropped, Vec3::new(0.0, -11.0, 0.0)),
                lying(2, Departure::Dropped, Vec3::new(0.0, -9.0, 0.0)),
            ],
            Vec3::ZERO,
            &RULES,
        );
        assert_eq!(lost, vec![(object(1), LostWhy::OutOfWorld)]);
    }

    #[test]
    fn only_a_weapon_let_go_of_at_speed_is_left_behind() {
        let far = Vec3::new(40.0, 5.0, 0.0);
        let near = Vec3::new(20.0, 5.0, 0.0);
        let lost = judge(
            &[
                lying(1, Departure::AtSpeed, far),
                lying(2, Departure::Dropped, far),
                lying(3, Departure::AtSpeed, near),
                lying(4, Departure::Returned, far),
            ],
            Vec3::ZERO,
            &RULES,
        );
        assert_eq!(lost, vec![(object(1), LostWhy::LeftBehind)]);
    }

    #[test]
    fn a_weapon_whose_place_is_unknown_is_never_lost() {
        let unknown = Lying {
            object: object(1),
            departure: Departure::AtSpeed,
            at: None,
        };
        assert!(judge(&[unknown], Vec3::ZERO, &RULES).is_empty());
    }

    #[test]
    fn the_speed_follows_the_motion_and_ignores_a_bad_step() {
        let mut last = None;
        let mut at = Vec3::ZERO;
        for _ in 0..60 {
            at += Vec3::new(0.5, 0.0, 0.0);
            last = Some((at, speed(last, at, 0.05)));
        }
        let smoothed = last.unwrap().1;
        assert!((smoothed - 10.0).abs() < 0.1, "{smoothed}");
        assert_eq!(speed(last, Vec3::NAN, 0.05), smoothed);
        assert_eq!(speed(last, at, 0.0), smoothed);
        assert_eq!(speed(None, at, 0.05), 0.0);
    }
}
