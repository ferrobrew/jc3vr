//! The game's clock against the player's.
//!
//! The game runs its world on a clock that its slow motion slows down ([`Clock::m_UpdateSpeed`],
//! game seconds per real second), while the player's body moves in real time. The bindings tick's
//! step is the game's (the update context's scaled delta), as are the velocities of the physics
//! bodies and the characters; the controllers' poses, their display times, and [`Instant`] are
//! real. A quantity that measures the player's body (a swing, a pull, a gesture's dwell, a grip's
//! timing) is measured in real time, and one that drives the game world is in game time, converted
//! where the two meet with a [`TimeScale`].
//!
//! [`Instant`]: std::time::Instant

use glam::Vec3;
use jc3gi::clock::Clock;

use crate::director::time::{MAX_TIME_SCALE, MIN_TIME_SCALE};

/// The game's time scale: game seconds per real second, one at full speed and below one in slow
/// motion.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TimeScale(f32);

impl TimeScale {
    /// The world running in real time.
    pub const REAL: Self = Self(1.0);

    /// The time scale of a clock whose update speed is `update_speed`: one for a value that is not
    /// finite and positive, and otherwise bounded to the range the director's time scale reaches.
    pub fn new(update_speed: f32) -> Self {
        if update_speed.is_finite() && update_speed > 0.0 {
            Self(update_speed.clamp(MIN_TIME_SCALE, MAX_TIME_SCALE))
        } else {
            Self::REAL
        }
    }

    /// The game clock's time scale now, or [`REAL`](Self::REAL) without a clock. The slow-motion
    /// update rewrites the speed once a frame, possibly while the input job reads it on another
    /// thread, so it is read once, whole, as the aligned float it is.
    ///
    /// # Safety
    ///
    /// The game is running, so the clock singleton is live.
    pub unsafe fn current() -> Self {
        unsafe { Clock::get() }.map_or(Self::REAL, |clock| {
            // SAFETY: a field of the live singleton, aligned, read without forming a reference
            // that a concurrent write could invalidate.
            Self::new(unsafe { std::ptr::read_volatile(&raw const clock.m_UpdateSpeed) })
        })
    }

    /// Game seconds per real second.
    pub const fn get(self) -> f32 {
        self.0
    }

    /// The real seconds a game step of `game_dt` seconds took; zero for a step that is not finite
    /// and positive.
    pub fn real_dt(self, game_dt: f32) -> f32 {
        if game_dt.is_finite() && game_dt > 0.0 {
            game_dt / self.0
        } else {
            0.0
        }
    }

    /// The game seconds that pass in `real_s` real seconds: a real duration on the game's clock.
    pub fn game_s(self, real_s: f32) -> f32 {
        real_s * self.0
    }

    /// A rate of the player's body, per real second, as a rate per game second: the same motion
    /// through the slowed world, faster in its seconds.
    pub fn to_game_rate(self, per_real_s: Vec3) -> Vec3 {
        per_real_s / self.0
    }
}

/// Game seconds counted from real ones: each reading advances by the real time since the last,
/// scaled by the time scale then.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GameClock {
    /// The last reading: real seconds, and the game seconds counted to then.
    last: Option<(f64, f64)>,
}

impl GameClock {
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// The game seconds counted at `real_s` real seconds, the world running at `time_scale` since
    /// the last reading. A real clock that went back counts nothing.
    pub fn at(&mut self, real_s: f64, time_scale: TimeScale) -> f64 {
        let game_s = match self.last {
            Some((last_real, last_game)) if real_s.is_finite() => {
                last_game + (real_s - last_real).max(0.0) * f64::from(time_scale.get())
            }
            Some((_, last_game)) => last_game,
            None => 0.0,
        };
        if real_s.is_finite() {
            self.last = Some((real_s, game_s));
        }
        game_s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broken_clock_is_real_time_and_the_scale_stays_in_range() {
        for broken in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(TimeScale::new(broken), TimeScale::REAL);
        }
        assert_eq!(TimeScale::new(0.001).get(), MIN_TIME_SCALE);
        assert_eq!(TimeScale::new(100.0).get(), MAX_TIME_SCALE);
        assert_eq!(TimeScale::new(0.25).get(), 0.25);
    }

    #[test]
    fn a_slowed_step_took_longer_in_real_time() {
        let slow = TimeScale::new(0.25);
        assert_eq!(slow.real_dt(0.0083), 0.0083 / 0.25);
        assert_eq!(TimeScale::REAL.real_dt(0.033), 0.033);
        assert_eq!(slow.game_s(0.03), 0.0075);
        for broken in [0.0, -0.01, f32::NAN] {
            assert_eq!(slow.real_dt(broken), 0.0);
        }
    }

    #[test]
    fn a_real_rate_is_faster_per_game_second() {
        let slow = TimeScale::new(0.25);
        let swing = Vec3::new(4.0, 0.0, -2.0);
        assert_eq!(slow.to_game_rate(swing), swing * 4.0);
        assert_eq!(TimeScale::REAL.to_game_rate(swing), swing);
    }

    #[test]
    fn the_game_clock_counts_real_time_at_the_scale_it_ran_at() {
        let mut clock = GameClock::new();
        assert_eq!(clock.at(10.0, TimeScale::REAL), 0.0);
        assert_eq!(clock.at(11.0, TimeScale::REAL), 1.0);
        assert_eq!(clock.at(13.0, TimeScale::new(0.25)), 1.5);
        // A real clock that went back counts nothing, and a broken one keeps the count.
        assert_eq!(clock.at(12.0, TimeScale::REAL), 1.5);
        assert_eq!(clock.at(f64::NAN, TimeScale::REAL), 1.5);
        assert_eq!(clock.at(13.0, TimeScale::REAL), 2.5);
    }
}
