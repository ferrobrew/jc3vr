//! Catching a live grenade and throwing it on.
//!
//! Every grenade launched, by the player or by anyone else, goes through the grenade's launch
//! ([`Grenade::PropelToward`]), and it has its physics body by then. The launch hook notes each one
//! ([`register`]) with its body's id and the body's owner as the launch found them. A body still
//! owned by the same object proves the grenade still exists (it has not detonated or been
//! removed), so the input tick reads each noted grenade's body through that check and forgets the
//! ones whose check fails.
//!
//! On the input tick, each live grenade within [`GrenadeCatchConfig::range_m`] of the player is a
//! grab volume ([`GrabVolume::LiveGrenade`]) for each free hand, in the cockpit frame the volumes
//! live in. A grip press there catches the grenade nearest the hand: from then on, each tick gives
//! its body the velocity that carries it to the hand's grip, so it stays in the hand with its own
//! physics, and its fuse keeps running. Opening the grip throws it: its body is given the hand's
//! release velocity, estimated as a throw from the pouch is, plus the character's own velocity.
//!
//! [`Grenade::PropelToward`]: jc3gi::equipment::explosive::Grenade::PropelToward
//! [`GrabVolume::LiveGrenade`]: crate::controllers::grab::GrabVolume::LiveGrenade
//! [`GrenadeCatchConfig::range_m`]: crate::controllers::weapons::grenade::config::GrenadeCatchConfig::range_m

use std::{num::NonZeroUsize, time::Instant};

use glam::Vec3;
use jc3gi::{equipment::explosive::Explosive, physics::PfxInstance};
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    grab::{Candidate, GrabVolume},
    hands::held::{self, Slot},
    physical_hands,
    weapons::grenade::config::GrenadeCatchConfig,
};

/// A live grenade the input tick saw, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Sighting {
    /// The grenade's address.
    pub explosive: usize,
    pub body: u32,
    pub owner: NonZeroUsize,
    /// The body's centre, cockpit frame.
    pub cockpit: Vec3,
}

/// A grenade a hand holds.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Caught {
    pub hand: Hand,
    pub explosive: usize,
    pub body: u32,
    pub owner: NonZeroUsize,
}

/// The catch's counts, for the readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct CatchCounts {
    /// Grenades noted at their launch and not yet gone.
    pub tracked: usize,
    /// Of those, the ones within reach of a catch on the last tick.
    pub in_reach: usize,
    pub caught: u64,
    pub thrown: u64,
    /// Caught grenades that detonated or vanished in the hand.
    pub lost: u64,
}

/// Note a grenade at its launch: `explosive` was launched with its physics body, and `own` is
/// whether the player's own throw launched it. Called from the launch hook, on whichever thread
/// updates the thrower's weapon.
///
/// # Safety
///
/// `explosive` must be live, as it is for its own launch.
pub(crate) unsafe fn register(explosive: *const Explosive, own: bool) {
    let Some(e) = (unsafe { explosive.as_ref() }) else {
        return;
    };
    let Some(body) = (unsafe { e.m_PfxInstance.as_ref() }) else {
        return;
    };
    // The rigid body wrapper is an engine physics instance, whose owner a query reports.
    let owner = unsafe { (*(e.m_PfxInstance as *const PfxInstance)).m_GameObject };
    let Some(owner) = NonZeroUsize::new(owner as usize) else {
        return;
    };
    let entry = Tracked {
        explosive: explosive as usize,
        body: body.m_BodyId,
        owner,
        launched: Instant::now(),
        own,
    };
    let mut state = STATE.lock();
    state.tracked.retain(|t| t.explosive != entry.explosive);
    if state.tracked.len() >= MAX_TRACKED {
        state.tracked.remove(0);
    }
    state.tracked.push(entry);
}

/// Read every noted grenade's body, forget the ones gone, and return the live ones within
/// `config.range_m` of `player` that may be caught, carried into the cockpit frame by
/// `to_cockpit`.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn sightings(
    player: Vec3,
    config: &GrenadeCatchConfig,
    to_cockpit: impl Fn(Vec3) -> Vec3,
) -> Vec<Sighting> {
    let now = Instant::now();
    let mut state = STATE.lock();
    let caught = state.caught.map(|c| c.explosive);
    let mut seen = Vec::new();
    state.tracked.retain(|t| {
        let Ok((centre, _)) = (unsafe { physical_hands::body_pose(t.body, t.owner) }) else {
            return false;
        };
        let age_s = now.saturating_duration_since(t.launched).as_secs_f32();
        if age_s > MAX_AGE_S {
            return false;
        }
        if catchable(t.own, age_s, config.own_grace_s)
            && Some(t.explosive) != caught
            && centre.distance(player) <= config.range_m
        {
            seen.push(Sighting {
                explosive: t.explosive,
                body: t.body,
                owner: t.owner,
                cockpit: to_cockpit(centre),
            });
        }
        true
    });
    state.counts.tracked = state.tracked.len();
    state.counts.in_reach = seen.len();
    seen
}

/// The grab volumes of the sightings, for the hands `free` marks.
pub fn candidates(seen: &[Sighting], radius_m: f32, free: [bool; 2]) -> Vec<Candidate> {
    seen.iter()
        .flat_map(|s| {
            Hand::BOTH
                .into_iter()
                .filter(move |h| free[h.index()])
                .map(move |hand| Candidate {
                    volume: GrabVolume::LiveGrenade,
                    centre: s.cockpit,
                    radius: radius_m,
                    hand: Some(hand),
                })
        })
        .collect()
}

/// The sighting nearest `grip`, the grabbing hand's grip in the cockpit frame, within `radius_m`.
pub fn nearest(seen: &[Sighting], grip: Vec3, radius_m: f32) -> Option<Sighting> {
    seen.iter()
        .map(|s| (s.cockpit.distance(grip), s))
        .filter(|(d, _)| d.is_finite() && *d <= radius_m)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, s)| *s)
}

/// Whether a grenade launched `age_s` ago may be caught: another's at once, the player's own once
/// `grace_s` has passed.
pub fn catchable(own: bool, age_s: f32, grace_s: f32) -> bool {
    !own || age_s >= grace_s
}

/// The velocity that carries a body at `centre` to `target` over `dt`, capped at `max_mps`.
pub fn carry_velocity(centre: Vec3, target: Vec3, dt: f32, max_mps: f32) -> Vec3 {
    if !(dt > 0.0 && centre.is_finite() && target.is_finite()) {
        return Vec3::ZERO;
    }
    ((target - centre) / dt).clamp_length_max(max_mps.max(0.0))
}

/// Hold the caught grenade `caught`.
pub(crate) fn set_caught(caught: Option<Caught>) {
    let mut state = STATE.lock();
    if caught.is_some() {
        state.counts.caught += 1;
    }
    state.caught = caught;
}

pub(crate) fn caught() -> Option<Caught> {
    STATE.lock().caught
}

/// Let go of the caught grenade: `thrown` when the hand threw it, otherwise it was lost.
pub(crate) fn let_go(thrown: bool) {
    held::hold(Slot::CaughtGrenade, None);
    let mut state = STATE.lock();
    if let Some(caught) = state.caught.take() {
        if thrown {
            state.counts.thrown += 1;
            state.thrown[caught.hand.index()] = true;
        } else {
            state.counts.lost += 1;
        }
    }
}

/// The hands that threw a caught grenade on since the last call, by [`Hand::index`].
pub(crate) fn take_thrown() -> [bool; 2] {
    std::mem::take(&mut STATE.lock().thrown)
}

pub fn counts() -> CatchCounts {
    STATE.lock().counts
}

/// The most grenades noted at once; the oldest is forgotten past it.
const MAX_TRACKED: usize = 32;

/// How long a noted grenade is kept at most, seconds: far longer than any fuse.
const MAX_AGE_S: f32 = 30.0;

#[derive(Debug, Copy, Clone)]
struct Tracked {
    explosive: usize,
    body: u32,
    owner: NonZeroUsize,
    launched: Instant,
    own: bool,
}

struct State {
    tracked: Vec<Tracked>,
    caught: Option<Caught>,
    /// The hands that threw a caught grenade on since the last read, by [`Hand::index`].
    thrown: [bool; 2],
    counts: CatchCounts,
}

static STATE: Mutex<State> = Mutex::new(State {
    tracked: Vec::new(),
    caught: None,
    thrown: [false; 2],
    counts: CatchCounts {
        tracked: 0,
        in_reach: 0,
        caught: 0,
        thrown: 0,
        lost: 0,
    },
});

#[cfg(test)]
mod tests {
    use super::*;

    fn sighting(x: f32) -> Sighting {
        Sighting {
            explosive: 0x1000 + x.to_bits() as usize,
            body: 1,
            owner: NonZeroUsize::new(0x2000).unwrap(),
            cockpit: Vec3::new(x, 0.0, 0.0),
        }
    }

    #[test]
    fn the_nearest_grenade_in_reach_is_caught() {
        let seen = [sighting(0.15), sighting(0.05), sighting(0.4)];
        assert_eq!(nearest(&seen, Vec3::ZERO, 0.2), Some(seen[1]));
        assert_eq!(nearest(&seen, Vec3::new(1.0, 0.0, 0.0), 0.2), None);
        assert_eq!(nearest(&[], Vec3::ZERO, 0.2), None);
    }

    #[test]
    fn each_free_hand_gets_a_volume_per_grenade() {
        let seen = [sighting(0.1), sighting(0.3)];
        let both = candidates(&seen, 0.18, [true, true]);
        assert_eq!(both.len(), 4);
        assert!(both.iter().all(|c| c.volume == GrabVolume::LiveGrenade));
        let one = candidates(&seen, 0.18, [false, true]);
        assert_eq!(one.len(), 2);
        assert!(one.iter().all(|c| c.hand == Some(Hand::Right)));
        assert!(candidates(&seen, 0.18, [false, false]).is_empty());
    }

    #[test]
    fn the_players_own_grenade_waits_out_its_grace() {
        assert!(catchable(false, 0.0, 0.4));
        assert!(!catchable(true, 0.1, 0.4));
        assert!(catchable(true, 0.4, 0.4));
    }

    #[test]
    fn the_carry_reaches_the_hand_in_one_tick_within_its_cap() {
        let v = carry_velocity(Vec3::ZERO, Vec3::new(0.0, 0.1, 0.0), 0.01, 25.0);
        assert!((v - Vec3::new(0.0, 10.0, 0.0)).length() < 1e-4);
        let capped = carry_velocity(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 0.01, 25.0);
        assert!((capped.length() - 25.0).abs() < 1e-3);
        assert_eq!(carry_velocity(Vec3::ZERO, Vec3::X, 0.0, 25.0), Vec3::ZERO);
        assert_eq!(carry_velocity(Vec3::NAN, Vec3::X, 0.01, 25.0), Vec3::ZERO);
    }
}
