//! Where the held view's head comes from: over the root, or following Rico's animated head.
//!
//! The held view hangs from the neutral head over the root while the player is on their own feet,
//! so no clip's bob, crouch, or landing moves it. A ragdoll, its get-up, and a death carry Rico's
//! body far from the root for seconds and end with it somewhere else, and a view held over the
//! root then floats above and behind the body. In those the held view follows the animated head
//! instead, through a critically damped follower with a speed cap, so a ragdoll's fall is an ease
//! rather than a drop. The view's frame stays upright either way; only its position follows.
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
    /// Following the animated head: the followed head and its velocity.
    Head { head: Vec3, velocity: Vec3 },
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
        let shown = self.last.filter(|last| last.is_finite());
        match (source, self.state) {
            (HeldSource::Root, FollowState::Root { offset, rate }) => {
                let (offset, rate) = damp(offset, rate, Vec3::ZERO, RETURN_SMOOTHING_S, dt);
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
            (
                HeldSource::Head,
                FollowState::Head {
                    head: from,
                    velocity,
                },
            ) => {
                let (head, velocity) = damp(from, velocity, animated, FOLLOW_SMOOTHING_S, dt);
                let step = head - from;
                let max_step = MAX_FOLLOW_SPEED_MPS * dt;
                let (head, velocity) = if step.length() > max_step {
                    (
                        from + step.normalize() * max_step,
                        velocity.clamp_length_max(MAX_FOLLOW_SPEED_MPS),
                    )
                } else {
                    (head, velocity)
                };
                self.state = FollowState::Head { head, velocity };
                head
            }
            (HeldSource::Head, _) => {
                self.state = FollowState::Head {
                    head: shown.unwrap_or(animated),
                    velocity: Vec3::ZERO,
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

/// The smoothing of the followed head, seconds: it settles in about a third of a second, so a
/// get-up's rise is followed and a ragdoll's flail is eased.
const FOLLOW_SMOOTHING_S: f32 = 0.12;

/// The fastest the followed head moves, metres per second: a ragdoll's first fall eases in rather
/// than dropping the view.
const MAX_FOLLOW_SPEED_MPS: f32 = 4.0;

/// The smoothing of the difference left on the way back to the root, seconds.
const RETURN_SMOOTHING_S: f32 = 0.12;

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
    /// `distance` is how far the animated head is from the neutral head over the root,
    /// horizontally, metres. Returns whether the view follows the animated head.
    pub fn step(&mut self, allowed: bool, distance: f32, dt: f32) -> bool {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if !allowed || !distance.is_finite() {
            *self = Self::new();
            return false;
        }
        if self.escaped {
            if distance < ESCAPE_RETURN_M {
                *self = Self::new();
            }
            return self.escaped;
        }
        if distance > ESCAPE_M {
            self.away_s += dt;
            self.escaped = self.away_s >= ESCAPE_S;
        } else {
            self.away_s = 0.0;
        }
        self.escaped
    }
}

/// How far the animated head must be from the root's, horizontally, to count toward the escape,
/// metres: past a walk's lean and a crouch's forward reach.
const ESCAPE_M: f32 = 0.45;

/// How long the head must stay that far for the view to follow it, seconds: longer than a landing's
/// dip.
const ESCAPE_S: f32 = 0.4;

/// How close the head must come back for the view to return to the root, metres.
const ESCAPE_RETURN_M: f32 = 0.2;

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
            assert!(next.distance(last) <= MAX_FOLLOW_SPEED_MPS * DT + 1e-5);
            last = next;
        }
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
