//! Where the held view's head comes from: over the root, or following Rico's animated head.
//!
//! The held view hangs from the neutral head over the root while the player is on their own feet,
//! so no clip's bob, crouch, or landing moves it. A ragdoll, its get-up, and a death carry Rico's
//! body far from the root for seconds and end with it somewhere else, and a view held over the
//! root then floats above and behind the body. In those the held view follows the animated head
//! instead, through a critically damped follower of the head's offset from the root with a speed
//! cap, so the head's flail and fall about the body are an ease rather than a drop while the
//! body's own motion (a ragdoll thrown off a car, a long fall) carries the view uncapped. The
//! view's frame stays upright either way; only its position follows.
//!
//! A landing or an authored one-off that carries the head well away from the root for longer than
//! a dip (a fast fall's landing that pitches Rico forward, a crash that turns into a roll) escapes
//! to the follower too ([`Escape`]), and comes back once the head is over the root again. A short
//! landing crouch never reaches the escape, which is what keeps a jump off a roof from moving the
//! view.
//!
//! Switching between the two sources never jumps the view: the follower starts where the view is,
//! and on the way back to the root the difference winds down to nothing ([`HeadFollower`]).

use body_rig::smooth::critically_damped;
use glam::Vec3;

use crate::controllers::locomotion::tuning::LOCOMOTION;

/// Which head the held view hangs from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HeldSource {
    /// The neutral head over the root.
    Root,
    /// Rico's animated head, followed.
    Head,
}

/// The held view's head at the previous and the current tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeadPair {
    pub previous: Vec3,
    pub current: Vec3,
}

/// Follows the held view's head from one source to the other without a jump.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HeadFollower {
    state: FollowState,
    last: Option<Vec3>,
}

#[derive(Debug, Copy, Clone, PartialEq, Default)]
enum FollowState {
    /// Not running: the head is the source's.
    #[default]
    Idle,
    /// Over the root, with what is left of the difference from the previous source and its rate.
    Root { offset: Vec3, rate: Vec3 },
    /// Following the animated head: the followed head's offset from the root and its rate.
    Head { offset: Vec3, rate: Vec3 },
}

impl HeadFollower {
    pub const fn new() -> Self {
        Self {
            state: FollowState::Idle,
            last: None,
        }
    }

    /// Advance by one tick of `dt` seconds with the view hung from `source`: `root` is the neutral
    /// head over the root and `animated` Rico's animated head, both world space.
    pub fn step(&mut self, source: HeldSource, root: Vec3, animated: Vec3, dt: f32) -> HeadPair {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let current = if root.is_finite() && animated.is_finite() {
            self.advance(source, root, animated, dt)
        } else {
            self.state = FollowState::Idle;
            match source {
                HeldSource::Root => root,
                HeldSource::Head => animated,
            }
        };
        let previous = self.last.unwrap_or(current);
        self.last = Some(current);
        HeadPair { previous, current }
    }

    /// Forget the follow: the next step starts at its source.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    fn advance(&mut self, source: HeldSource, root: Vec3, animated: Vec3, dt: f32) -> Vec3 {
        let tuning = &LOCOMOTION.get().follow;
        let shown = self.last.filter(|last| last.is_finite());
        match (source, self.state) {
            (HeldSource::Root, FollowState::Root { offset, rate }) => {
                let (offset, rate) = damp(offset, rate, Vec3::ZERO, tuning.return_smoothing_s, dt);
                self.state = FollowState::Root { offset, rate };
                root + offset
            }
            (HeldSource::Root, _) => {
                let offset = shown.map_or(Vec3::ZERO, |shown| shown - root);
                self.state = FollowState::Root {
                    offset,
                    rate: Vec3::ZERO,
                };
                self.advance(source, root, animated, dt)
            }
            (HeldSource::Head, FollowState::Head { offset: from, rate }) => {
                let (offset, rate) = damp(from, rate, animated - root, tuning.smoothing_s, dt);
                let step = offset - from;
                let max_step = tuning.max_speed_mps * dt;
                let (offset, rate) = if step.length() > max_step {
                    (
                        from + step.normalize() * max_step,
                        rate.clamp_length_max(tuning.max_speed_mps),
                    )
                } else {
                    (offset, rate)
                };
                self.state = FollowState::Head { offset, rate };
                root + offset
            }
            (HeldSource::Head, _) => {
                self.state = FollowState::Head {
                    offset: shown.unwrap_or(animated) - root,
                    rate: Vec3::ZERO,
                };
                self.advance(source, root, animated, dt)
            }
        }
    }
}

/// The critically damped step of each axis of `value` toward `target`.
fn damp(value: Vec3, rate: Vec3, target: Vec3, smooth_s: f32, dt: f32) -> (Vec3, Vec3) {
    let axis = |i: usize| critically_damped(value[i], rate[i], target[i], smooth_s, dt);
    let (x, y, z) = (axis(0), axis(1), axis(2));
    (Vec3::new(x.0, y.0, z.0), Vec3::new(x.1, y.1, z.1))
}

/// Whether a landing or a one-off has carried Rico's head far enough from the root, for long
/// enough, that the held view follows it.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Escape {
    away_s: f32,
    escaped: bool,
}

impl Escape {
    pub const fn new() -> Self {
        Self {
            away_s: 0.0,
            escaped: false,
        }
    }

    /// Fold in one tick of `dt` seconds. `allowed` says the case may escape at all, and
    /// `distance` is how far the animated head is from the neutral head over the root
    /// ([`escape_distance`]). Returns whether the view follows the animated head.
    pub fn step(&mut self, allowed: bool, distance: f32, dt: f32) -> bool {
        let tuning = &LOCOMOTION.get().follow;
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if !allowed || !distance.is_finite() {
            *self = Self::new();
            return false;
        }
        if self.escaped {
            if distance < tuning.escape_return_m {
                *self = Self::new();
            }
            return self.escaped;
        }
        if distance > tuning.escape_m {
            self.away_s += dt;
            self.escaped = self.away_s >= tuning.escape_s;
        } else {
            self.away_s = 0.0;
        }
        self.escaped
    }

    /// Start escaped: the view takes over from a source that left the head `distance`
    /// ([`escape_distance`]) from the root's, and follows the animated head until it is over the
    /// root again rather than cutting through the body to the root.
    pub fn seed(&mut self, distance: f32) {
        if distance.is_finite() && distance > LOCOMOTION.get().follow.escape_m {
            self.escaped = true;
        }
    }
}

/// How far the animated head is from the neutral head over the root for the escape, given the
/// difference `away` between them, metres: the horizontal distance, or the head's drop scaled so a
/// drop of the locomotion tuning's `follow.escape_drop_m` counts as its `follow.escape_m`,
/// whichever is further. A landing's crouch drops
/// the head well short of the drop's threshold; a body lying on the ground does not.
pub fn escape_distance(away: Vec3) -> f32 {
    let tuning = &LOCOMOTION.get().follow;
    let horizontal = Vec3::new(away.x, 0.0, away.z).length();
    let drop = (-away.y).max(0.0) * tuning.escape_m / tuning.escape_drop_m;
    horizontal.max(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn over_the_root_the_head_is_the_roots() {
        let mut follow = HeadFollower::new();
        let root = Vec3::new(1.0, 1.7, 2.0);
        let pair = follow.step(HeldSource::Root, root, Vec3::ZERO, DT);
        assert_eq!(pair.current, root);
        let moved = root + Vec3::X;
        assert_eq!(
            follow.step(HeldSource::Root, moved, Vec3::ZERO, DT).current,
            moved
        );
    }

    #[test]
    fn following_starts_where_the_view_is_and_reaches_the_head() {
        let mut follow = HeadFollower::new();
        let root = Vec3::new(0.0, 1.7, 0.0);
        let head = Vec3::new(0.0, 0.3, 0.6);
        follow.step(HeldSource::Root, root, head, DT);
        let first = follow.step(HeldSource::Head, root, head, DT).current;
        assert!(first.distance(root) < 0.1, "{first}");
        let mut last = first;
        for _ in 0..120 {
            last = follow.step(HeldSource::Head, root, head, DT).current;
        }
        assert!(last.distance(head) < 0.01, "{last}");
    }

    #[test]
    fn the_follow_never_moves_faster_than_its_cap() {
        let mut follow = HeadFollower::new();
        let root = Vec3::new(0.0, 1.7, 0.0);
        let head = Vec3::new(0.0, -5.0, 0.0);
        let mut last = follow.step(HeldSource::Root, root, head, DT).current;
        for _ in 0..60 {
            let next = follow.step(HeldSource::Head, root, head, DT).current;
            assert!(next.distance(last) <= LOCOMOTION.get().follow.max_speed_mps * DT + 1e-5);
            last = next;
        }
    }

    #[test]
    fn a_body_moving_fast_carries_the_followed_view_with_it() {
        // A ragdoll thrown off a car at 20 m/s, its head a steady offset from the root.
        let mut follow = HeadFollower::new();
        let offset = Vec3::new(0.0, -1.2, 0.4);
        let velocity = Vec3::new(20.0, -3.0, 0.0);
        let mut root = Vec3::new(0.0, 1.7, 0.0);
        follow.step(HeldSource::Root, root, root + offset, DT);
        let mut last = Vec3::ZERO;
        for _ in 0..120 {
            root += velocity * DT;
            last = follow
                .step(HeldSource::Head, root, root + offset, DT)
                .current;
        }
        assert!(
            last.distance(root + offset) < 0.01,
            "the view keeps up with the body: {last} against {}",
            root + offset
        );
    }

    #[test]
    fn the_escape_counts_a_long_drop_and_not_a_crouch() {
        assert!(escape_distance(Vec3::new(0.0, -0.5, 0.0)) < LOCOMOTION.get().follow.escape_m);
        assert!(escape_distance(Vec3::new(0.0, -1.3, 0.0)) > LOCOMOTION.get().follow.escape_m);
        assert!(escape_distance(Vec3::new(0.0, 0.5, 0.5)) > LOCOMOTION.get().follow.escape_m);
        assert!(escape_distance(Vec3::new(0.3, 0.0, 0.0)) < LOCOMOTION.get().follow.escape_m);
    }

    #[test]
    fn a_seeded_escape_follows_until_the_head_is_back() {
        let mut escape = Escape::new();
        escape.seed(0.2);
        assert!(!escape.step(true, 0.3, DT), "a near head seeds nothing");
        escape.seed(1.0);
        assert!(escape.step(true, 1.0, DT));
        assert!(escape.step(true, 0.3, DT));
        assert!(!escape.step(true, 0.1, DT));
    }

    #[test]
    fn returning_to_the_root_winds_the_difference_down() {
        let mut follow = HeadFollower::new();
        let root = Vec3::new(0.0, 1.7, 0.0);
        let head = Vec3::new(0.0, 0.5, 0.5);
        for _ in 0..120 {
            follow.step(HeldSource::Head, root, head, DT);
        }
        let first = follow.step(HeldSource::Root, root, head, DT).current;
        assert!(first.distance(head) < 0.1, "{first}");
        let mut last = first;
        for _ in 0..120 {
            last = follow.step(HeldSource::Root, root, head, DT).current;
        }
        assert!(last.distance(root) < 0.01, "{last}");
    }

    #[test]
    fn the_previous_head_is_the_last_ticks() {
        let mut follow = HeadFollower::new();
        let a = follow.step(HeldSource::Root, Vec3::Y, Vec3::ZERO, DT);
        assert_eq!(a.previous, a.current);
        let b = follow.step(HeldSource::Root, Vec3::Y * 2.0, Vec3::ZERO, DT);
        assert_eq!(b.previous, a.current);
    }

    #[test]
    fn a_landing_dip_does_not_escape_and_a_long_excursion_does() {
        let mut escape = Escape::new();
        for _ in 0..18 {
            assert!(!escape.step(true, 0.9, DT));
        }
        assert!(!escape.step(true, 0.1, DT));
        let mut escaped = false;
        for _ in 0..30 {
            escaped = escape.step(true, 0.9, DT);
        }
        assert!(escaped);
        assert!(
            escape.step(true, 0.3, DT),
            "it holds until the head is back"
        );
        assert!(!escape.step(true, 0.1, DT));
    }

    #[test]
    fn a_case_that_may_not_escape_never_does() {
        let mut escape = Escape::new();
        for _ in 0..120 {
            assert!(!escape.step(false, 2.0, DT));
        }
    }
}
