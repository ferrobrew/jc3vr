//! The rail's motion, in the airframe's frame: where the skid's line runs, how the player's walk
//! and the sticky pull move him along it, and when he has stepped off.

use glam::Vec3;

/// The skid's line Rico stands on, in the airframe's body frame: a level segment at his feet's
/// height.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RailLine {
    pub a: Vec3,
    pub b: Vec3,
}

impl RailLine {
    /// The line through `feet`, Rico's feet in the airframe's frame, along whichever of the
    /// frame's level axes his offset from the airframe's centre `centre` runs least along: he
    /// stands beside the cabin, so the skid runs fore and aft across that offset. It reaches
    /// `half_length_m` either way.
    pub(crate) fn through(feet: Vec3, centre: Vec3, half_length_m: f32) -> Self {
        let offset = feet - centre;
        let along = if offset.x.abs() >= offset.z.abs() {
            Vec3::Z
        } else {
            Vec3::X
        };
        let reach = along * half_length_m.max(0.0);
        Self {
            a: feet - reach,
            b: feet + reach,
        }
    }

    /// The point of the line nearest `at`, measured level.
    pub(crate) fn nearest(&self, at: Vec3) -> Vec3 {
        let ab = self.b - self.a;
        let level = Vec3::new(ab.x, 0.0, ab.z);
        let length2 = level.length_squared();
        let t = if length2 > 1e-9 {
            ((at - self.a) * Vec3::new(1.0, 0.0, 1.0)).dot(level) / length2
        } else {
            0.0
        };
        self.a + ab * t.clamp(0.0, 1.0)
    }
}

/// The rail's tuning.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RailMotion {
    /// How far off the line, level, the feet go before Rico has stepped off, metres.
    pub step_off_m: f32,
    /// The sticky pull's time constant: the offset off the line closes at this rate, seconds.
    pub pull_s: f32,
    /// The time constant that holds the feet at the line's height, seconds.
    pub height_s: f32,
    /// The fastest walk the rail passes on, metres per second.
    pub max_walk_mps: f32,
    /// How far below the line the feet go before the rail lets them go, as a hand climb lowering
    /// Rico off the skid takes them, metres.
    pub max_drop_m: f32,
}

/// What a step of the rail does.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum RailStep {
    /// Carry Rico: his velocity relative to the airframe, in its frame.
    Carry(Vec3),
    /// The feet went past the step-off distance: Rico falls.
    SteppedOff,
    /// The feet went below the line: Rico has gone off the skid's edge.
    Dropped,
}

/// Step the rail for feet at `feet` and a walk of `walk`, both in the airframe's frame.
pub(crate) fn step(line: &RailLine, feet: Vec3, walk: Vec3, motion: &RailMotion) -> RailStep {
    let nearest = line.nearest(feet);
    if feet.y < nearest.y - motion.max_drop_m {
        return RailStep::Dropped;
    }
    let off = Vec3::new(feet.x - nearest.x, 0.0, feet.z - nearest.z);
    if off.length() > motion.step_off_m {
        return RailStep::SteppedOff;
    }
    let walk = Vec3::new(walk.x, 0.0, walk.z).clamp_length_max(motion.max_walk_mps.max(0.0));
    let pull = off / motion.pull_s.max(1e-3);
    let rise = (nearest.y - feet.y) / motion.height_s.max(1e-3);
    RailStep::Carry(Vec3::new(walk.x - pull.x, rise, walk.z - pull.z))
}

/// The player's walk relative to the airframe from the velocity the character's own update gave
/// the proxy, `game`, and the airframe's velocity under the feet, `surface`: whichever of the two
/// readings is slower, the update's velocity as it is (an update that did not carry the proxy
/// with its support) or less the surface's (one that did), with whether the update carried it.
pub(crate) fn walk_from(game: Vec3, surface: Vec3) -> (Vec3, bool) {
    let level = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let raw = level(game);
    let less = level(game - surface);
    if less.length() < raw.length() {
        (less, true)
    } else {
        (raw, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOTION: RailMotion = RailMotion {
        step_off_m: 0.35,
        pull_s: 0.25,
        height_s: 0.1,
        max_walk_mps: 3.0,
        max_drop_m: 0.5,
    };

    #[test]
    fn the_line_runs_across_the_offset_from_the_cabin() {
        let beside = RailLine::through(Vec3::new(1.2, -1.0, 0.3), Vec3::ZERO, 1.0);
        assert_eq!(beside.a, Vec3::new(1.2, -1.0, -0.7));
        assert_eq!(beside.b, Vec3::new(1.2, -1.0, 1.3));
        let ahead = RailLine::through(Vec3::new(0.2, -1.0, 2.0), Vec3::ZERO, 1.0);
        assert_eq!(ahead.a.z, 2.0);
        assert!((ahead.b.x - 1.2).abs() < 1e-6);
    }

    #[test]
    fn the_nearest_point_is_measured_level_and_clamped_to_the_ends() {
        let line = RailLine::through(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, 1.0);
        assert_eq!(
            line.nearest(Vec3::new(1.3, 0.4, 0.5)),
            Vec3::new(1.0, 0.0, 0.5)
        );
        assert_eq!(
            line.nearest(Vec3::new(1.0, 0.0, 3.0)),
            Vec3::new(1.0, 0.0, 1.0)
        );
    }

    #[test]
    fn a_walk_along_the_line_passes_and_the_pull_draws_the_feet_back() {
        let line = RailLine::through(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, 1.0);
        let RailStep::Carry(along) = step(&line, Vec3::new(1.0, 0.0, 0.2), Vec3::Z, &MOTION) else {
            panic!("on the line");
        };
        assert!(along.distance(Vec3::Z) < 1e-5, "{along:?}");
        let RailStep::Carry(back) = step(&line, Vec3::new(1.2, 0.0, 0.0), Vec3::ZERO, &MOTION)
        else {
            panic!("within the step-off");
        };
        assert!((back.x + 0.8).abs() < 1e-5 && back.z.abs() < 1e-6);
    }

    #[test]
    fn a_slow_walk_outward_holds_and_a_brisk_one_steps_off() {
        let line = RailLine::through(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, 1.0);
        // The pull balances a walk outward at the offset of the walk times the pull's time.
        let slow = Vec3::X * 1.0;
        let held = 1.0 + slow.x * MOTION.pull_s;
        let RailStep::Carry(v) = step(&line, Vec3::new(held, 0.0, 0.0), slow, &MOTION) else {
            panic!("a slow walk is held");
        };
        assert!(v.x.abs() < 1e-5);
        let brisk = 2.0 * MOTION.step_off_m / MOTION.pull_s;
        let RailStep::Carry(v) = step(&line, Vec3::new(1.3, 0.0, 0.0), Vec3::X * brisk, &MOTION)
        else {
            panic!("inside the step-off");
        };
        assert!(v.x > 0.0);
        assert_eq!(
            step(&line, Vec3::new(1.4, 0.0, 0.0), Vec3::ZERO, &MOTION),
            RailStep::SteppedOff
        );
        assert_eq!(
            step(&line, Vec3::new(1.0, 0.0, 1.5), Vec3::ZERO, &MOTION),
            RailStep::SteppedOff
        );
    }

    #[test]
    fn the_feet_are_held_at_the_line_and_let_go_below_it() {
        let line = RailLine::through(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, 1.0);
        let RailStep::Carry(v) = step(&line, Vec3::new(1.0, -0.05, 0.0), Vec3::ZERO, &MOTION)
        else {
            panic!("on the line");
        };
        assert!((v.y - 0.5).abs() < 1e-5);
        assert_eq!(
            step(&line, Vec3::new(1.0, -0.6, 0.0), Vec3::ZERO, &MOTION),
            RailStep::Dropped
        );
    }

    #[test]
    fn the_walk_is_read_whether_or_not_the_update_carried_the_proxy() {
        let surface = Vec3::new(20.0, 1.0, 0.0);
        let walk = Vec3::new(0.0, 0.0, 1.2);
        assert_eq!(walk_from(walk, surface), (walk, false));
        assert_eq!(walk_from(walk + surface, surface), (walk, true));
        assert_eq!(walk_from(Vec3::ZERO, Vec3::ZERO), (Vec3::ZERO, false));
    }
}
