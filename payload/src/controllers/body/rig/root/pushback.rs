//! The push-back: holding the view at a wall the body cannot pass. Pure.
//!
//! The view stays with the real head, so a player who walks or leans into a wall sees through it
//! while the root stands at the wall. While nothing stops the root, the chase owns the gap and
//! there is no push-back. Once the ledger says the chase is obstructed, the view may go only a
//! threshold past the root: the part of the gap beyond it is taken out of the cockpit by sliding the
//! cockpit shift back along the gap, on both sides of the pose pair, so the view holds rigidly at
//! the obstruction. This is a translation of the room against the world, never a rotation.
//!
//! The slide is capped per episode (the debt). With the cap spent, the view fades to black over a
//! span past the threshold. The episode ends once the gap has stayed under half the threshold for a
//! moment; the slide it applied stays, as the room's new place against the world.
//!
//! The thresholds depend on the root's owner: tight while the player's chase is obstructed, loose
//! while the animation carries the body (only a runaway gap), and an arm's length while climbing
//! or seated, where a lean is all the head does.
//!
//! Vectors are world `(x, z)`, metres.

use glam::Vec2;

use crate::controllers::body::rig::{config::PushbackConfig, root::owner::RootOwner};

/// One frame's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PushbackInput {
    /// The gap outside the lean, from the root to the target: what the chase has yet to close.
    pub gap: Vec2,
    /// Who owned the root on the last tick.
    pub owner: Option<RootOwner>,
    /// Whether the ledger found the chase obstructed over its last steps.
    pub stopped: bool,
    /// Seconds since the last frame.
    pub dt: f32,
}

/// One frame's outcome.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PushbackOutcome {
    /// How far to move the view, world `(x, z)`: back along the gap toward the root.
    pub push: Vec2,
    /// The threshold in force, metres, while there is one.
    pub threshold: Option<f32>,
    /// Whether the player's chase counts as obstructed.
    pub obstructed: bool,
    /// The push-back fade's target, `0..=1`.
    pub fade: f32,
}

/// The push-back's episode state.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Pushback {
    /// How far the view has been slid back in this episode, metres.
    debt: f32,
    /// How long the gap has stayed under half the threshold, seconds.
    calm_s: f32,
    /// Episodes started since the payload loaded.
    episodes: u64,
}

impl Pushback {
    pub const fn new() -> Self {
        Self {
            debt: 0.0,
            calm_s: 0.0,
            episodes: 0,
        }
    }

    /// The slide applied in the current episode, metres.
    pub fn debt(&self) -> f32 {
        self.debt
    }

    pub fn episodes(&self) -> u64 {
        self.episodes
    }

    /// End the episode at once, after a teleport or a recenter.
    pub fn end_episode(&mut self) {
        self.debt = 0.0;
        self.calm_s = 0.0;
    }

    /// Advance one frame.
    pub fn step(&mut self, input: &PushbackInput, config: &PushbackConfig) -> PushbackOutcome {
        let gap = if input.gap.is_finite() {
            input.gap
        } else {
            Vec2::ZERO
        };
        let r = gap.length();
        let obstructed =
            input.owner == Some(RootOwner::Player) && input.stopped && r > config.min_gap_m;
        let threshold = threshold(input.owner, obstructed, config);

        // The episode ends once the gap has been calm for a moment, measured against the tight
        // threshold when none is in force, so a slide that flickers in and out of obstruction does
        // not start a fresh episode each time.
        let calm_below = threshold.unwrap_or(config.obstructed_m) * 0.5;
        let dt = if input.dt.is_finite() {
            input.dt.max(0.0)
        } else {
            0.0
        };
        if r < calm_below {
            self.calm_s += dt;
            if self.calm_s >= config.reset_s {
                self.debt = 0.0;
            }
        } else {
            self.calm_s = 0.0;
        }

        let Some(t) = threshold else {
            return PushbackOutcome {
                obstructed,
                ..PushbackOutcome::default()
            };
        };
        let cap = if config.enabled {
            config.debt_cap_m.max(0.0)
        } else {
            0.0
        };
        let mut push = Vec2::ZERO;
        let mut left = r;
        if r > t && self.debt < cap {
            let p = (r - t).min(cap - self.debt);
            if self.debt == 0.0 {
                self.episodes += 1;
            }
            self.debt += p;
            push = -gap / r * p;
            left = r - p;
        }
        let fade = if self.debt >= cap - CAP_EPSILON_M {
            smoothstep((left - t) / config.fade_span_m.max(1e-3))
        } else {
            0.0
        };
        PushbackOutcome {
            push,
            threshold: Some(t),
            obstructed,
            fade,
        }
    }
}

/// The threshold for the root's owner, metres, or `None` when the chase owns the gap or the game
/// owns the root.
pub fn threshold(
    owner: Option<RootOwner>,
    obstructed: bool,
    config: &PushbackConfig,
) -> Option<f32> {
    match owner? {
        RootOwner::Player => obstructed.then_some(config.obstructed_m),
        RootOwner::Authored => Some(config.authored_m),
        RootOwner::Climb | RootOwner::Seated => Some(config.reach_m),
        RootOwner::Game => None,
    }
}

/// How close to the cap the debt counts as spent, metres.
const CAP_EPSILON_M: f32 = 1e-5;

fn smoothstep(t: f32) -> f32 {
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: PushbackConfig = PushbackConfig::new();
    const DT: f32 = 1.0 / 90.0;

    fn into_wall(gap_m: f32) -> PushbackInput {
        PushbackInput {
            gap: Vec2::new(0.0, -gap_m),
            owner: Some(RootOwner::Player),
            stopped: true,
            dt: DT,
        }
    }

    #[test]
    fn an_unobstructed_chase_owns_the_gap() {
        let mut pushback = Pushback::new();
        let free = PushbackInput {
            stopped: false,
            ..into_wall(0.5)
        };
        assert_eq!(pushback.step(&free, &CONFIG), PushbackOutcome::default());
        let game = PushbackInput {
            owner: Some(RootOwner::Game),
            ..into_wall(0.5)
        };
        assert_eq!(pushback.step(&game, &CONFIG).push, Vec2::ZERO);
    }

    #[test]
    fn an_obstruction_holds_the_view_at_the_threshold_up_to_the_cap() {
        let mut pushback = Pushback::new();
        let out = pushback.step(&into_wall(0.25), &CONFIG);
        assert!(out.obstructed);
        assert!((out.push - Vec2::new(0.0, 0.15)).length() < 1e-6, "{out:?}");
        assert_eq!(out.fade, 0.0);
        assert_eq!(pushback.episodes(), 1);
        // A further 0.2 m: only 0.15 of the cap is left, and the fade starts past it.
        let out = pushback.step(&into_wall(0.30), &CONFIG);
        assert!((out.push.length() - 0.15).abs() < 1e-6, "{out:?}");
        assert!((pushback.debt() - 0.30).abs() < 1e-6);
        assert!(out.fade > 0.0 && out.fade < 1.0, "{out:?}");
        let out = pushback.step(&into_wall(0.10 + 0.15), &CONFIG);
        assert_eq!(out.push, Vec2::ZERO);
        assert_eq!(out.fade, 1.0);
        assert_eq!(pushback.episodes(), 1);
    }

    #[test]
    fn a_calm_gap_ends_the_episode() {
        let mut pushback = Pushback::new();
        pushback.step(&into_wall(0.5), &CONFIG);
        assert!(pushback.debt() > 0.0);
        let calm = PushbackInput {
            stopped: false,
            ..into_wall(0.02)
        };
        for _ in 0..10 {
            pushback.step(&calm, &CONFIG);
        }
        assert!(pushback.debt() > 0.0, "ended before the reset time");
        for _ in 0..20 {
            pushback.step(&calm, &CONFIG);
        }
        assert_eq!(pushback.debt(), 0.0);
    }

    #[test]
    fn off_it_only_fades() {
        let off = PushbackConfig {
            enabled: false,
            ..CONFIG
        };
        let mut pushback = Pushback::new();
        let out = pushback.step(&into_wall(0.10 + 0.075), &off);
        assert_eq!(out.push, Vec2::ZERO);
        assert!((out.fade - 0.5).abs() < 1e-5, "{out:?}");
    }

    #[test]
    fn the_thresholds_follow_the_owner() {
        assert_eq!(
            threshold(Some(RootOwner::Player), true, &CONFIG),
            Some(0.10)
        );
        assert_eq!(threshold(Some(RootOwner::Player), false, &CONFIG), None);
        assert_eq!(
            threshold(Some(RootOwner::Authored), false, &CONFIG),
            Some(0.30)
        );
        assert_eq!(
            threshold(Some(RootOwner::Climb), false, &CONFIG),
            Some(0.45)
        );
        assert_eq!(
            threshold(Some(RootOwner::Seated), false, &CONFIG),
            Some(0.45)
        );
        assert_eq!(threshold(Some(RootOwner::Game), true, &CONFIG), None);
        assert_eq!(threshold(None, true, &CONFIG), None);
    }
}
