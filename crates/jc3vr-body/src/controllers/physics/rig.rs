//! The rig that carries the hands, as the physics readers see it: the body's facing and the
//! recenter count read together ([`RigSample`]), and one reader's watch on them ([`RigWatch`]),
//! which tells a jump of the rig from the body turning.
//!
//! A snap turn or a recenter moves every palm in the rig at once without the hands moving, so the
//! hand bodies, the climb, and the grapple's pump each start their motion afresh across one. Each
//! reader compares the sample it takes on its own tick with the one it took on its last, so a turn
//! is judged over the reader's own interval: a fast smooth turn that passes the threshold between
//! two of a slow reader's ticks is a jump for that reader.

use glam::Quat;
use jc3vr_view::{headpose, vr::recenter_count};

use crate::controllers::physics::tuning::PHYSICS;

/// The body's facing and the recenter count, read together on one reader's tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RigSample {
    /// The body frame the cockpit pose composes onto ([`headpose::xr::body_rotation`]).
    pub rotation: Quat,
    /// How many recenters have run ([`recenter_count`]).
    pub recenters: u32,
}

impl RigSample {
    /// The rig as it is now.
    pub fn now() -> Self {
        Self {
            rotation: headpose::xr::body_rotation(),
            recenters: recenter_count(),
        }
    }
}

/// Whether the rig jumped between `last` and `now`: the recenter count changed, or the body's
/// facing turned by more than the tuning's `rig.jump_rad` (a snap turn, not the body turning).
/// Never without a `last` sample.
pub fn jumped(last: Option<RigSample>, now: RigSample) -> bool {
    last.is_some_and(|last| {
        last.recenters != now.recenters
            || last.rotation.angle_between(now.rotation) > PHYSICS.get().rig.jump_rad
    })
}

/// One reader's watch on the rig: the sample its last tick took.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct RigWatch {
    last: Option<RigSample>,
}

impl RigWatch {
    /// A watch that has seen nothing, so its first sample is no jump.
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// Take this tick's sample: whether the rig jumped since the last one ([`jumped`]).
    pub fn observe(&mut self, now: RigSample) -> bool {
        let jumped = jumped(self.last, now);
        self.last = Some(now);
        jumped
    }

    /// The sample the last tick took, if there was one.
    pub fn last(&self) -> Option<RigSample> {
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(yaw: f32, recenters: u32) -> RigSample {
        RigSample {
            rotation: Quat::from_rotation_y(yaw),
            recenters,
        }
    }

    #[test]
    fn a_first_sample_is_no_jump() {
        assert!(!jumped(None, sample(0.0, 0)));
        assert!(!RigWatch::new().observe(sample(1.0, 4)));
    }

    #[test]
    fn a_snap_turn_or_a_recenter_is_a_jump_and_a_small_turn_is_not() {
        let jump = PHYSICS.get().rig.jump_rad;
        let last = Some(sample(0.0, 3));
        assert!(!jumped(last, sample(jump * 0.5, 3)));
        assert!(jumped(last, sample(jump * 2.0, 3)));
        assert!(jumped(last, sample(0.0, 4)));
    }

    #[test]
    fn a_watch_judges_each_turn_over_its_own_interval() {
        let jump = PHYSICS.get().rig.jump_rad;
        let mut watch = RigWatch::new();
        watch.observe(sample(0.0, 0));
        assert!(!watch.observe(sample(jump * 0.6, 0)));
        assert!(!watch.observe(sample(jump * 1.2, 0)));
        assert!(watch.observe(sample(jump * 2.6, 0)));
        assert_eq!(watch.last(), Some(sample(jump * 2.6, 0)));
    }
}
