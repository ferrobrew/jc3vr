//! A mounted gun's recoil in the handles, behind
//! [`MountedConfig::recoil`](crate::controllers::vehicles::config::MountedConfig::recoil).
//!
//! Every shot of the mounted gun the hands aim (seen by the per-shot seam,
//! [`crate::controllers::weapons::fire`]) kicks a damped spring: back along the barrel and down,
//! as the gun's breech jumps back and its muzzle climbs about the pivot. The spring's offset moves
//! the handles the hands are drawn on, so the hands ride the kick, and each shot pulses the hands
//! that hold the handles. The aim is left alone: the gun's own pivot takes the kick in the game.
//!
//! The kick of a gun firing faster than the spring settles adds up, limited to a few shots' worth,
//! so a minigun holds the handles back and shakes them rather than driving them off.
//!
//! The shots arrive on the thread that updates the weapon and the offset is read by the hands at
//! every rate, so the spring lives in a leaf slot and runs on wall-clock time, advanced lazily by
//! whichever reads or kicks it, as the hand-held guns' recoil does
//! ([`crate::controllers::weapons::recoil`]).

use std::{
    sync::atomic::{AtomicU8, AtomicU64, Ordering},
    time::Instant,
};

use glam::{Vec2, Vec3};
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    haptics::{self, Event},
    vehicles::config::MountedConfig,
    weapons::recoil::Spring,
};

/// Kick the handles by one shot of the mounted gun, and pulse the hands holding them.
pub(crate) fn on_shot(config: &MountedConfig) {
    if !config.recoil {
        return;
    }
    SHOTS.fetch_add(1, Ordering::Relaxed);
    {
        let mut kick = KICK.lock();
        let now = Instant::now();
        advance(&mut kick, now);
        let peak = Vec3::new(config.kick_back_m, config.kick_dip_m, 0.0);
        kick.spring.kick(peak, STIFFNESS);
        let cap = peak * MAX_SHOTS;
        kick.spring.velocity = kick.spring.velocity.clamp(-cap * 40.0, cap * 40.0);
        kick.spring.position = kick.spring.position.clamp(-cap, cap);
    }
    let holds = HOLDS.load(Ordering::Relaxed);
    for hand in Hand::BOTH {
        if holds & (1 << hand.index()) != 0 {
            haptics::play(hand, Event::MountedKick);
        }
    }
}

/// The handles' kick now: back along the barrel and down, metres.
pub(super) fn offset() -> Vec2 {
    let mut kick = KICK.lock();
    advance(&mut kick, Instant::now());
    Vec2::new(kick.spring.position.x, kick.spring.position.y)
}

/// The kick `offset` (back, down) as a move of the handles in the cockpit frame, for a gun whose
/// barrel points along `barrel` there.
pub fn handle_shift(offset: Vec2, barrel: Vec3) -> Vec3 {
    let Some(barrel) = barrel.try_normalize() else {
        return Vec3::ZERO;
    };
    let up = Vec3::Y
        .reject_from_normalized(barrel)
        .try_normalize()
        .unwrap_or(Vec3::Y);
    -barrel * offset.x - up * offset.y
}

/// Which hands hold the handles, for the shot's pulse: bit [`Hand::index`] set for a holding hand.
pub(super) fn set_holds(holds: [bool; 2]) {
    let bits = Hand::BOTH
        .into_iter()
        .filter(|h| holds[h.index()])
        .fold(0, |bits, h| bits | (1 << h.index()));
    HOLDS.store(bits, Ordering::Relaxed);
}

/// Shots of the mounted gun that kicked the handles, since the payload loaded.
pub fn shots() -> u64 {
    SHOTS.load(Ordering::Relaxed)
}

struct Kick {
    spring: Spring,
    at: Option<Instant>,
}

static KICK: Mutex<Kick> = Mutex::new(Kick {
    spring: Spring {
        position: Vec3::ZERO,
        velocity: Vec3::ZERO,
    },
    at: None,
});
static HOLDS: AtomicU8 = AtomicU8::new(0);
static SHOTS: AtomicU64 = AtomicU64::new(0);

/// The handles' spring: stiff, so a single shot settles within a tenth of a second or so.
const STIFFNESS: f32 = 900.0;
const DAMPING_RATIO: f32 = 0.6;
/// How many shots' kick the handles can carry at once.
const MAX_SHOTS: f32 = 3.0;

fn advance(kick: &mut Kick, now: Instant) {
    if let Some(at) = kick.at {
        kick.spring.step(
            now.duration_since(at).as_secs_f32(),
            STIFFNESS,
            DAMPING_RATIO,
        );
    }
    kick.at = Some(now);
    if kick.spring.settled() {
        kick.spring = Spring::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kick_moves_the_handles_back_toward_the_gunner_and_down() {
        // The barrel points forward (-Z); back is toward the gunner (+Z).
        let shift = handle_shift(Vec2::new(0.02, 0.01), Vec3::NEG_Z);
        assert!(
            (shift - Vec3::new(0.0, -0.01, 0.02)).length() < 1e-6,
            "{shift}"
        );
        // Aimed up, the back is down the barrel and the dip is square to it.
        let up = Vec3::new(0.0, 1.0, -1.0).normalize();
        let shift = handle_shift(Vec2::new(0.02, 0.0), up);
        assert!(shift.dot(up) < 0.0);
        assert_eq!(handle_shift(Vec2::ONE, Vec3::ZERO), Vec3::ZERO);
    }

    #[test]
    fn a_kick_settles() {
        let mut spring = Spring::default();
        spring.kick(Vec3::new(0.02, 0.01, 0.0), STIFFNESS);
        let mut peak: f32 = 0.0;
        for _ in 0..30 {
            spring.step(0.01, STIFFNESS, DAMPING_RATIO);
            peak = peak.max(spring.position.x);
        }
        assert!(peak > 0.01 && peak < 0.03, "{peak}");
        assert!(spring.position.x.abs() < 0.002, "{}", spring.position.x);
    }
}
