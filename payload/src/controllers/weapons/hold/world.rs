//! Weapons in the world that the hands' reach cannot find, because they have no body in the
//! physics world: a gun in a living character's hands, and a weapon on a rack or in a locker.
//! Pure: the grip targets placed on them and which one a grab took.
//!
//! Each target is a short segment along the gun, from the grip of the hand that holds it to the
//! grip of the other hand (or the gun's origin without one), in the cockpit frame. Each hand
//! without a gun of its own is offered the point of the segment nearest it, so that a hand closing
//! anywhere on the gun takes it, and the marker shows where. A grab of one
//! ([`GrabVolume::WorldWeapon`]) takes the target whose segment is nearest the grabbing hand.

use glam::Vec3;
use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::{
    frame::{Hand, Pose},
    grab::{Candidate, GrabVolume},
};

/// Where a target weapon is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TargetKind {
    /// In the hands of the living character at this address.
    Held { character: usize },
    /// On a rack, at this weapon's address.
    Rack { weapon: usize },
}

/// A weapon a hand can take.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WorldTarget {
    pub kind: TargetKind,
    pub weapon: usize,
    pub slot: WeaponSlot,
    /// The ends of the gun's segment, in the cockpit frame.
    pub points: [Vec3; 2],
    /// The first gun's pose in the world, which a take slides the gun into the hand from.
    pub gun: Pose,
}

/// The grip targets on `targets`, of `radius`, for each hand in `free` with its drawn point in
/// `points` (by [`Hand::index`]): each at the point of the gun's segment nearest the hand.
pub fn candidates(
    targets: &[WorldTarget],
    radius: f32,
    free: [bool; 2],
    points: [Option<Vec3>; 2],
) -> Vec<Candidate> {
    let radius = radius.max(0.0);
    let mut out = Vec::new();
    for target in targets {
        let [a, b] = target.points;
        for hand in Hand::BOTH.into_iter().filter(|h| free[h.index()]) {
            let Some(point) = points[hand.index()] else {
                continue;
            };
            out.push(Candidate {
                volume: GrabVolume::WorldWeapon,
                centre: nearest_on_segment(point, a, b),
                radius,
                hand: Some(hand),
            });
        }
    }
    out
}

/// The target a grab at `point` took: the one whose segment is nearest it, within `reach` of the
/// segment.
pub fn chosen(targets: &[WorldTarget], point: Vec3, reach: f32) -> Option<WorldTarget> {
    targets
        .iter()
        .map(|t| (t, segment_distance(point, t.points[0], t.points[1])))
        .filter(|(_, d)| d.is_finite() && *d <= reach.max(0.0))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(t, _)| *t)
}

/// The distance from `point` to the segment from `a` to `b`.
pub fn segment_distance(point: Vec3, a: Vec3, b: Vec3) -> f32 {
    point.distance(nearest_on_segment(point, a, b))
}

/// The point of the segment from `a` to `b` nearest `point`.
pub fn nearest_on_segment(point: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let along = b - a;
    let length_sq = along.length_squared();
    let t = if length_sq > f32::EPSILON {
        ((point - a).dot(along) / length_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    a + along * t
}

/// The impulse that staggers a character without knocking it down, from its hit-reaction tuning:
/// `type_scale` scales an impulse of the reaction's damage type, a scaled impulse above
/// `stumble_threshold` staggers it, and one at `ragdoll_threshold` knocks it down. `None` without
/// a usable tuning, or when no impulse staggers without knocking down.
pub fn stagger_impulse(
    type_scale: f32,
    stumble_threshold: f32,
    ragdoll_threshold: f32,
) -> Option<f32> {
    const OVER_STUMBLE: f32 = 1.1;
    const UNDER_RAGDOLL: f32 = 0.9;
    if !(type_scale.is_finite() && stumble_threshold.is_finite() && type_scale > 0.0) {
        return None;
    }
    let scaled = stumble_threshold.max(0.0) * OVER_STUMBLE;
    let scaled = if ragdoll_threshold.is_finite() && ragdoll_threshold > 0.0 {
        if stumble_threshold >= ragdoll_threshold {
            return None;
        }
        scaled.min(ragdoll_threshold * UNDER_RAGDOLL)
    } else {
        scaled
    };
    (scaled > 0.0).then_some(scaled / type_scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(kind: TargetKind, a: Vec3, b: Vec3) -> WorldTarget {
        WorldTarget {
            kind,
            weapon: 0x1000,
            slot: WeaponSlot::E_SLOT_TWO_HANDED,
            points: [a, b],
            gun: Pose::IDENTITY,
        }
    }

    #[test]
    fn a_segment_measures_to_its_nearest_point() {
        let a = Vec3::ZERO;
        let b = Vec3::new(1.0, 0.0, 0.0);
        assert!((segment_distance(Vec3::new(0.5, 0.2, 0.0), a, b) - 0.2).abs() < 1e-6);
        assert!((segment_distance(Vec3::new(-0.3, 0.0, 0.0), a, b) - 0.3).abs() < 1e-6);
        assert!((segment_distance(Vec3::new(1.4, 0.3, 0.0), a, b) - 0.5).abs() < 1e-6);
        // A gun without a second grip is a point.
        assert!((segment_distance(Vec3::Y, a, a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_grab_takes_the_nearest_gun_within_reach() {
        let rifle = target(
            TargetKind::Held { character: 1 },
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -0.6),
        );
        let pistol = target(
            TargetKind::Held { character: 2 },
            Vec3::new(0.5, 0.0, 0.0),
            Vec3::new(0.55, 0.0, 0.0),
        );
        let targets = [rifle, pistol];
        let at_barrel = Vec3::new(0.05, 0.0, -0.5);
        assert_eq!(chosen(&targets, at_barrel, 0.12), Some(rifle));
        assert_eq!(
            chosen(&targets, Vec3::new(0.45, 0.0, 0.0), 0.12),
            Some(pistol)
        );
        assert_eq!(chosen(&targets, Vec3::new(0.25, 0.5, 0.0), 0.12), None);
        assert_eq!(chosen(&[], at_barrel, 0.12), None);
    }

    #[test]
    fn each_free_hand_is_offered_the_point_of_the_gun_nearest_it() {
        let rifle = target(
            TargetKind::Rack { weapon: 3 },
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -0.6),
        );
        let points = [
            Some(Vec3::new(0.1, 0.0, -0.4)),
            Some(Vec3::new(-0.05, 0.0, 0.3)),
        ];
        let both = candidates(&[rifle], 0.1, [true; 2], points);
        assert_eq!(both.len(), 2);
        assert!(both.iter().all(|c| c.volume == GrabVolume::WorldWeapon));
        // The left hand beside the barrel is offered the barrel beside it, the right hand behind the
        // stock the stock's end.
        assert!(both[0].centre.distance(Vec3::new(0.0, 0.0, -0.4)) < 1e-6);
        assert_eq!(both[0].hand, Some(Hand::Left));
        assert!(both[1].centre.distance(Vec3::ZERO) < 1e-6);
        let left = candidates(&[rifle], 0.1, [true, false], points);
        assert_eq!(left.len(), 1);
        assert!(candidates(&[rifle], 0.1, [false; 2], points).is_empty());
        assert!(candidates(&[rifle], 0.1, [true; 2], [None; 2]).is_empty());
    }

    #[test]
    fn the_stagger_lands_between_the_stumble_and_the_knockdown() {
        let impulse = stagger_impulse(2.0, 100.0, 300.0).unwrap();
        let scaled = impulse * 2.0;
        assert!(scaled > 100.0 && scaled < 300.0, "{scaled}");
        // A knockdown just above the stumble caps the stagger below it.
        let close = stagger_impulse(1.0, 100.0, 105.0).unwrap();
        assert!(close > 0.0 && close < 105.0, "{close}");
        // Without a usable tuning there is no stagger.
        assert_eq!(stagger_impulse(0.0, 100.0, 300.0), None);
        assert_eq!(stagger_impulse(1.0, f32::NAN, 300.0), None);
        assert_eq!(stagger_impulse(1.0, 300.0, 100.0), None);
    }
}
