//! The snatch's catch, as pure rules: which hand the line steers its target to, and when a hand
//! has caught it.
//!
//! The line steers toward the receiver: of the free, empty hands, the one nearest the target's
//! incoming path, the segment from the target to the player's eyes, whatever its grip is doing,
//! kept until the other is nearer by a margin ([`receiver`]). A catch is the hand's grip closed on
//! the target while the target is within the catch zone of its palm, or up to a grace after it was
//! last there ([`CatchWatch`]): a grip that closes a moment late still catches. The grapple hand
//! holds its grip throughout the line's flight, so it catches only as the receiver; the other free
//! hand catches whenever its grip closes on the target.

use body_rig::geometry::segment_distance;
use glam::Vec3;

use crate::controllers::frame::Hand;

/// The hand the line steers toward: of the hands `palms` offers (by [`Hand::index`], the free,
/// empty hands' palms), the one nearest the segment from the target at `target` to the eyes at
/// `eye`, keeping `current` unless the other is nearer by `hysteresis` metres or more. `None` when
/// no hand is offered.
pub fn receiver(
    current: Option<Hand>,
    palms: [Option<Vec3>; 2],
    target: Vec3,
    eye: Vec3,
    hysteresis: f32,
) -> Option<Hand> {
    let miss = |hand: Hand| {
        palms[hand.index()]
            .map(|palm| segment_distance(palm, target, eye))
            .filter(|miss| miss.is_finite())
    };
    let (nearest, nearest_miss) = Hand::BOTH
        .into_iter()
        .filter_map(|hand| miss(hand).map(|m| (hand, m)))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    match current.and_then(|hand| miss(hand).map(|m| (hand, m))) {
        Some((hand, miss)) if miss - nearest_miss < hysteresis.max(0.0) => Some(hand),
        _ => Some(nearest),
    }
}

/// A hand that may catch the target this tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CatchHand {
    /// Where the target must come to for this hand to catch it: the palm, or for a gun the landing
    /// steers, where the hand will hold the gun's grip.
    pub palm: Vec3,
    /// Whether the hand's grip is closed, on nothing or on the snatch.
    pub gripping: bool,
}

/// The catch's limits.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CatchTuning {
    /// How near a hand the target must come for the hand to catch it, metres.
    pub zone_m: f32,
    /// How long after the target was last within a hand's zone the hand's grip may still close on
    /// it, seconds.
    pub grace_s: f32,
}

/// What a tick of the catch found.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct CatchTick {
    /// The hand that caught the target this tick.
    pub caught: Option<Hand>,
    /// Whether a hand whose grip is open has the target within its zone or its grace, so the
    /// target is not yet missed.
    pub pending: bool,
}

/// Watches the target pass the hands: when each was last within a hand's zone.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct CatchWatch {
    time: f32,
    near: [Option<f32>; 2],
}

impl CatchWatch {
    /// Advance by `dt` seconds with the target moving from `from` to `to`, `hands` the hands that
    /// may catch it now (by [`Hand::index`]). The target is within a hand's zone when its path this
    /// tick passed within [`CatchTuning::zone_m`] of the hand, so a fast target is not stepped
    /// over. Of two hands that catch on the same tick, the one the path passed nearer catches. A
    /// hand that stops being offered forgets the target.
    pub fn step(
        &mut self,
        dt: f32,
        from: Vec3,
        to: Vec3,
        hands: [Option<CatchHand>; 2],
        tuning: &CatchTuning,
    ) -> CatchTick {
        self.time += if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let mut caught: Option<(Hand, f32)> = None;
        let mut pending = false;
        for hand in Hand::BOTH {
            let i = hand.index();
            let Some(offered) = hands[i] else {
                self.near[i] = None;
                continue;
            };
            let pass = segment_distance(offered.palm, from, to);
            if pass <= tuning.zone_m {
                self.near[i] = Some(self.time);
            }
            let within = self.near[i].is_some_and(|t| self.time - t <= tuning.grace_s.max(0.0));
            if !within {
                continue;
            }
            if !offered.gripping {
                pending = true;
            } else if caught.is_none_or(|(_, nearest)| pass < nearest) {
                caught = Some((hand, pass));
            }
        }
        CatchTick {
            caught: caught.map(|(hand, _)| hand),
            pending: pending && caught.is_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const DT: f32 = 1.0 / 30.0;
    const TUNING: CatchTuning = CatchTuning {
        zone_m: 0.4,
        grace_s: 0.15,
    };

    fn hand(palm: Vec3, gripping: bool) -> Option<CatchHand> {
        Some(CatchHand { palm, gripping })
    }

    #[test]
    fn the_line_steers_to_the_hand_put_in_the_target_s_way() {
        let eye = Vec3::new(0.0, 1.6, 0.0);
        let target = Vec3::new(0.0, 1.2, -10.0);
        // The grapple hand pulled back by the shoulder, the other reached out ahead of the face.
        let left = Vec3::new(-0.3, 1.3, -0.1);
        let right = Vec3::new(0.05, 1.45, -0.5);
        assert_eq!(
            receiver(None, [Some(left), Some(right)], target, eye, 0.1),
            Some(Hand::Right)
        );
        // Without the other hand, the grapple hand receives.
        assert_eq!(
            receiver(Some(Hand::Right), [Some(left), None], target, eye, 0.1),
            Some(Hand::Left)
        );
        assert_eq!(
            receiver(Some(Hand::Left), [None, None], target, eye, 0.1),
            None
        );
    }

    #[test]
    fn the_receiver_changes_only_for_a_clearly_nearer_hand() {
        let eye = Vec3::ZERO;
        let target = Vec3::new(0.0, 0.0, -10.0);
        let near = Vec3::new(0.05, 0.0, -0.4);
        let close = Vec3::new(-0.1, 0.0, -0.4);
        let far = Vec3::new(-0.3, 0.0, -0.4);
        assert_eq!(
            receiver(
                Some(Hand::Left),
                [Some(close), Some(near)],
                target,
                eye,
                0.1
            ),
            Some(Hand::Left)
        );
        assert_eq!(
            receiver(Some(Hand::Left), [Some(far), Some(near)], target, eye, 0.1),
            Some(Hand::Right)
        );
        assert_eq!(
            receiver(
                Some(Hand::Left),
                [Some(Vec3::NAN), Some(far)],
                target,
                eye,
                0.1
            ),
            Some(Hand::Right)
        );
    }

    #[test]
    fn a_held_grip_catches_where_the_path_crosses_the_zone() {
        let mut watch = CatchWatch::default();
        // Sixty centimetres a tick: never within the zone at a tick, but its path passes the palm.
        let tick = watch.step(
            DT,
            Vec3::new(0.05, 0.0, -0.5),
            Vec3::new(0.05, 0.0, 0.5),
            [hand(Vec3::ZERO, true), None],
            &TUNING,
        );
        assert_eq!(tick.caught, Some(Hand::Left));
    }

    #[test]
    fn a_grip_closing_within_the_grace_still_catches() {
        let mut watch = CatchWatch::default();
        let path = |k: f32| Vec3::new(0.1, 0.0, -0.6 + 0.3 * k);
        let palm = Vec3::ZERO;
        let first = watch.step(DT, path(0.0), path(1.0), [hand(palm, false), None], &TUNING);
        assert_eq!(
            first,
            CatchTick {
                caught: None,
                pending: true
            }
        );
        // Past the palm now, and the grip closes two ticks after the target came by.
        let second = watch.step(DT, path(1.0), path(2.0), [hand(palm, false), None], &TUNING);
        assert!(second.pending);
        let third = watch.step(DT, path(5.0), path(6.0), [hand(palm, true), None], &TUNING);
        assert_eq!(third.caught, Some(Hand::Left));
    }

    #[test]
    fn a_grip_closing_after_the_grace_does_not_catch() {
        let mut watch = CatchWatch::default();
        let palm = Vec3::ZERO;
        watch.step(
            DT,
            Vec3::new(0.1, 0.0, -0.3),
            Vec3::new(0.1, 0.0, 0.3),
            [hand(palm, false), None],
            &TUNING,
        );
        let mut last = None;
        for k in 0..6 {
            let from = Vec3::new(0.1, 0.0, 0.3 + 0.6 * k as f32);
            last = Some(watch.step(
                DT,
                from,
                from + Vec3::Z * 0.6,
                [hand(palm, k == 5), None],
                &TUNING,
            ));
        }
        assert_eq!(
            last,
            Some(CatchTick {
                caught: None,
                pending: false
            })
        );
    }

    #[test]
    fn a_target_outside_the_zone_is_not_caught_and_not_pending() {
        let mut watch = CatchWatch::default();
        let tick = watch.step(
            DT,
            Vec3::new(0.6, 0.0, -0.3),
            Vec3::new(0.6, 0.0, 0.3),
            [hand(Vec3::ZERO, true), hand(Vec3::X * 1.2, false)],
            &TUNING,
        );
        assert_eq!(
            tick,
            CatchTick {
                caught: None,
                pending: false
            }
        );
    }

    #[test]
    fn of_two_gripping_hands_the_nearer_pass_catches() {
        let mut watch = CatchWatch::default();
        let tick = watch.step(
            DT,
            Vec3::new(0.0, 0.0, -0.4),
            Vec3::new(0.0, 0.0, 0.2),
            [
                hand(Vec3::new(0.15, 0.0, 0.0), true),
                hand(Vec3::new(-0.05, 0.0, -0.1), true),
            ],
            &TUNING,
        );
        assert_eq!(tick.caught, Some(Hand::Right));
    }

    #[test]
    fn a_hand_no_longer_offered_forgets_the_target() {
        let mut watch = CatchWatch::default();
        let palm = Vec3::ZERO;
        let from = Vec3::new(0.1, 0.0, -0.3);
        watch.step(
            DT,
            from,
            from + Vec3::Z * 0.6,
            [hand(palm, false), None],
            &TUNING,
        );
        watch.step(DT, from, from, [None, None], &TUNING);
        let far = Vec3::new(2.0, 0.0, 0.0);
        let tick = watch.step(DT, far, far, [hand(palm, true), None], &TUNING);
        assert_eq!(tick.caught, None);
    }
}
