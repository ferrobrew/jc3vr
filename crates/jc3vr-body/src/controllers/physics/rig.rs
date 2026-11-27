//! The rig that carries the hands, as the physics readers see it: the body's facing and the counts
//! of the mod's own jumps of it, read together ([`RigSample`]), and one reader's watch on them
//! ([`RigWatch`]), which tells a jump of the rig from the body turning.
//!
//! A snap turn, a hard step of the view toward the body, a recenter, or a follow bank moves every
//! palm in the rig at once without the hands moving. The mod makes each of them, so each is an
//! event: the view counts the one-tick turns of the body frame ([`headpose::xr::rig_turn_count`])
//! and the re-bases of the cockpit frame ([`rebase_count`]), and a reader whose count changed since
//! its last tick starts its motion afresh, on that tick and the next. A turn the game makes in one
//! tick (a respawn, a cutscene, a teleport) has no event; past the tuning's
//! `rig.unannounced_turn_rad` between two of a reader's ticks, it counts as a jump too. That angle
//! is wider than any smooth turn makes in a tick, so a fast stick or mouse turn is the body
//! turning, never a jump.

use glam::Quat;
use jc3vr_view::{headpose, vr::rebase_count};

use crate::controllers::physics::tuning::PHYSICS;

/// The body's facing and the counts of the mod's jumps of the rig, read together on one reader's
/// tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RigSample {
    /// The body frame the cockpit pose composes onto ([`headpose::xr::body_rotation`]).
    pub rotation: Quat,
    /// How many times the cockpit frame has been re-based ([`rebase_count`]).
    pub rebases: u32,
    /// How many times the mod has turned the body frame in one tick
    /// ([`headpose::xr::rig_turn_count`]).
    pub turns: u32,
}

impl RigSample {
    /// The rig as it is now.
    pub fn now() -> Self {
        Self::with_rotation(headpose::xr::body_rotation())
    }

    /// The rig with the body frame `rotation` a reader already took this tick, and the counts as
    /// they are now.
    pub fn with_rotation(rotation: Quat) -> Self {
        Self {
            rotation,
            rebases: rebase_count(),
            turns: headpose::xr::rig_turn_count(),
        }
    }
}

/// What moved the rig at once between two of a reader's ticks.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RigJump {
    /// The cockpit frame was re-based: a recenter or a follow bank. Poses and velocities in the
    /// cockpit frame from before it do not compare with those after.
    Rebase,
    /// The body's facing turned by more than the tuning's `rig.unannounced_turn_rad` with no event:
    /// the game turned or placed the body.
    Unannounced,
    /// The mod turned the body frame in one tick: a snap turn or a hard step of the view. The
    /// cockpit frame and what is measured in it are unchanged; only the body frame turned.
    Turn,
    /// The tick after an event of the mod's. The counts and the facing are separate reads, and a
    /// reader on another thread can take the new count with the old facing, so the facing's jump
    /// can land on this tick with no change of count. Nothing measured in the cockpit frame
    /// changes on it.
    Settling,
}

/// One reader's watch on the rig: the sample its last tick took.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct RigWatch {
    last: Option<RigSample>,
    /// Whether the last tick saw an event of the mod's.
    announced: bool,
}

impl RigWatch {
    /// A watch that has seen nothing, so its first sample is no jump.
    pub const fn new() -> Self {
        Self {
            last: None,
            announced: false,
        }
    }

    /// Take this tick's sample: what jumped the rig since the last one, if anything. A re-base or
    /// an unannounced turn outranks a turn, so a reader that cuts its history on either never takes
    /// one for the milder case, and the tick after an event of the mod's is
    /// [`RigJump::Settling`] when nothing else jumped.
    pub fn observe(&mut self, now: RigSample) -> Option<RigJump> {
        let settling = std::mem::replace(&mut self.announced, false);
        let jump = self.last.and_then(|last| {
            self.announced = last.rebases != now.rebases || last.turns != now.turns;
            if last.rebases != now.rebases {
                Some(RigJump::Rebase)
            } else if last.rotation.angle_between(now.rotation)
                > PHYSICS.get().rig.unannounced_turn_rad
            {
                Some(RigJump::Unannounced)
            } else if last.turns != now.turns {
                Some(RigJump::Turn)
            } else {
                None
            }
        });
        self.last = Some(now);
        jump.or(settling.then_some(RigJump::Settling))
    }

    /// The sample the last tick took, if there was one.
    pub fn last(&self) -> Option<RigSample> {
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(yaw: f32, rebases: u32, turns: u32) -> RigSample {
        RigSample {
            rotation: Quat::from_rotation_y(yaw),
            rebases,
            turns,
        }
    }

    fn unannounced() -> f32 {
        PHYSICS.get().rig.unannounced_turn_rad
    }

    #[test]
    fn a_first_sample_is_no_jump() {
        assert_eq!(RigWatch::new().observe(sample(1.0, 4, 2)), None);
    }

    #[test]
    fn a_turn_or_a_rebase_is_a_jump_however_small_and_the_next_tick_settles_it() {
        let mut watch = RigWatch::new();
        watch.observe(sample(0.0, 3, 0));
        assert_eq!(watch.observe(sample(0.01, 3, 1)), Some(RigJump::Turn));
        assert_eq!(watch.observe(sample(0.5, 3, 1)), Some(RigJump::Settling));
        assert_eq!(watch.observe(sample(0.5, 3, 1)), None);
        assert_eq!(watch.observe(sample(0.5, 4, 1)), Some(RigJump::Rebase));
        assert_eq!(watch.observe(sample(0.5, 4, 2)), Some(RigJump::Turn));
        assert_eq!(watch.observe(sample(0.5, 4, 2)), Some(RigJump::Settling));
        assert_eq!(watch.observe(sample(0.5, 4, 2)), None);
    }

    #[test]
    fn a_smooth_turn_is_never_a_jump_and_a_turn_past_the_fallback_is() {
        let mut watch = RigWatch::new();
        watch.observe(sample(0.0, 0, 0));
        let mut yaw = 0.0;
        for _ in 0..20 {
            yaw += unannounced() * 0.9;
            assert_eq!(watch.observe(sample(yaw, 0, 0)), None);
        }
        yaw += unannounced() * 1.1;
        assert_eq!(watch.observe(sample(yaw, 0, 0)), Some(RigJump::Unannounced));
        assert_eq!(watch.observe(sample(yaw, 0, 0)), None);
        assert_eq!(watch.last(), Some(sample(yaw, 0, 0)));
    }

    #[test]
    fn an_unannounced_turn_outranks_a_turn_on_the_same_tick() {
        let mut watch = RigWatch::new();
        watch.observe(sample(0.0, 0, 0));
        assert_eq!(
            watch.observe(sample(unannounced() * 1.5, 0, 1)),
            Some(RigJump::Unannounced)
        );
        assert_eq!(
            watch.observe(sample(unannounced() * 1.5, 0, 1)),
            Some(RigJump::Settling)
        );
    }

    #[test]
    fn a_rebase_outranks_a_turn_and_an_unannounced_turn_on_the_same_tick() {
        let mut watch = RigWatch::new();
        watch.observe(sample(0.0, 0, 0));
        assert_eq!(
            watch.observe(sample(unannounced() * 1.5, 1, 1)),
            Some(RigJump::Rebase)
        );
        assert_eq!(
            watch.observe(sample(unannounced() * 1.5, 1, 1)),
            Some(RigJump::Settling)
        );
    }
}
