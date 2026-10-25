//! Grip targets for weapons discovered independently of the physical hands' probes: a gun in a
//! living character's hands, and a weapon on a rack or in a locker. This path does not classify
//! dropped weapon bodies or acquire remote grapple targets.
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
    weapons::{
        engine::{self, HeldWeapon},
        regions::GunRegion,
    },
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
    if !radius.is_finite() || radius <= 0.0 {
        return out;
    }
    for target in targets {
        let [a, b] = target.points;
        if !a.is_finite() || !b.is_finite() {
            continue;
        }
        for hand in Hand::BOTH.into_iter().filter(|h| free[h.index()]) {
            let Some(point) = points[hand.index()].filter(|p| p.is_finite()) else {
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
    if !point.is_finite() || !reach.is_finite() || reach <= 0.0 {
        return None;
    }
    targets
        .iter()
        .filter(|t| t.points.iter().all(|p| p.is_finite()))
        .map(|t| (t, segment_distance(point, t.points[0], t.points[1])))
        .filter(|(_, d)| d.is_finite() && *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(t, _)| *t)
}

/// The nearest target offered to each hand on one input tick.
#[derive(Debug, Default)]
pub(crate) struct Offers {
    hands: [Option<WorldTarget>; 2],
}

impl Offers {
    pub(crate) const fn new() -> Self {
        Self { hands: [None; 2] }
    }

    pub(crate) fn clear(&mut self) {
        self.hands = [None; 2];
    }

    pub(crate) fn gather(
        targets: &[WorldTarget],
        reach: f32,
        free: [bool; 2],
        points: [Option<Vec3>; 2],
    ) -> Self {
        Self {
            hands: Hand::BOTH.map(|hand| {
                points[hand.index()]
                    .filter(|_| free[hand.index()])
                    .and_then(|point| chosen(targets, point, reach))
            }),
        }
    }

    /// Consume only the cached offer, even when another target becomes nearer before completion.
    pub(crate) fn take(
        &mut self,
        hand: Hand,
        point: Option<Vec3>,
        reach: f32,
    ) -> Option<WorldTarget> {
        let offered = self.hands[hand.index()].take()?;
        chosen(std::slice::from_ref(&offered), point?, reach)
    }

    /// A completed finite source cannot also be taken by the other hand from this tick's cache.
    pub(crate) fn acquired(&mut self, target: WorldTarget) {
        for offer in &mut self.hands {
            if offer.is_some_and(|other| other.weapon == target.weapon) {
                *offer = None;
            }
        }
    }
}

/// The consumer revalidates the cached source; an invalid source is never replaced with a new one.
pub(crate) fn complete_offer<T, E>(
    offer: Option<WorldTarget>,
    complete: impl FnOnce(WorldTarget) -> Result<T, E>,
) -> Result<Option<T>, E> {
    offer.map(complete).transpose()
}

/// A free hand's transfer target around the held gun's measured pistol grip, in the cockpit frame.
/// A hand already touching another region keeps that region's action instead of transferring the
/// gun through the wider grip reach. The caller checks ownership and the handover setting.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Handover {
    pub holder: Hand,
    pub grip: Vec3,
    pub radius: f32,
    pub free: [bool; 2],
    pub points: [Option<Vec3>; 2],
    pub contact: Option<GunRegion>,
}

pub(crate) fn handover_candidate(input: Handover) -> Option<Candidate> {
    let hand = input.holder.other();
    let point = input.points[hand.index()]?;
    if !input.free[hand.index()]
        || !point.is_finite()
        || !input.grip.is_finite()
        || !input.radius.is_finite()
        || input.radius <= 0.0
        || input
            .contact
            .is_some_and(|region| region != GunRegion::Grip)
    {
        return None;
    }
    Some(Candidate {
        volume: GrabVolume::Handle,
        centre: input.grip,
        radius: input.radius,
        hand: Some(hand),
    })
}

/// The broad transfer policy, independent of whether region geometry was available this tick.
pub(crate) struct HandoverPolicy {
    pub allowed: bool,
    pub engaged_foregrip: bool,
    pub guns: [Option<HeldWeapon>; 2],
    pub offer: Handover,
}

pub(crate) fn broad_handover_candidate(input: HandoverPolicy) -> Option<Candidate> {
    if !input.allowed || input.engaged_foregrip {
        return None;
    }
    let gun = engine::lone_gun(input.guns, input.offer.free)?;
    if gun.holder != Some(input.offer.holder) {
        return None;
    }
    handover_candidate(input.offer)
}

/// The native grip point read at `placed_wrist`, recomposed on the wrist the gun is drawn on now.
/// Both wrist positions and the point are in their respective world and cockpit frames; the local
/// offset is converted to player-scale metres before recomposition.
pub(crate) fn grip_on_current_wrist(
    point_world: Vec3,
    placed_wrist: Pose,
    current_wrist: Pose,
    world_scale: f32,
) -> Option<Vec3> {
    if !point_world.is_finite()
        || !placed_wrist.position.is_finite()
        || !placed_wrist.orientation.is_finite()
        || !placed_wrist.orientation.is_normalized()
        || !current_wrist.position.is_finite()
        || !current_wrist.orientation.is_finite()
        || !current_wrist.orientation.is_normalized()
        || !world_scale.is_finite()
        || world_scale <= 0.0
    {
        return None;
    }
    let local =
        placed_wrist.orientation.inverse() * (point_world - placed_wrist.position) / world_scale;
    let point = current_wrist.position + current_wrist.orientation * local;
    point.is_finite().then_some(point)
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
    use glam::{Mat4, Quat};

    use super::*;
    use crate::controllers::{
        body::rig::frames::Frames,
        grab::{Mark, MarkAt, PickupKind, RemoteKind, Tier, resolve},
        weapons::{config::HoldConfig, engine::Wielded, heft::HeftClass},
    };

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

    fn resolved_index(candidates: &[Candidate], point: Vec3) -> Option<usize> {
        let targets: Vec<_> = candidates
            .iter()
            .map(|c| c.target(Mark::ring(MarkAt::Cockpit(c.centre), 0.06)))
            .collect();
        resolve::resolve(&targets, [Some(point), None])[Hand::Left.index()]
            .chosen
            .map(|r| r.index)
    }

    #[test]
    fn acquisition_and_completion_share_the_capsule_boundary() {
        let gun = target(TargetKind::Rack { weapon: 3 }, Vec3::ZERO, Vec3::NEG_Z);
        let radius = HoldConfig::new().take_radius_m;
        for (point, inside) in [
            (Vec3::new(radius - 0.001, 0.0, -0.5), true),
            (Vec3::new(radius, 0.0, -0.5), true),
            (Vec3::new(radius + 0.001, 0.0, -0.5), false),
            (Vec3::new(0.0, 0.0, radius), true),
            (Vec3::new(0.0, 0.0, radius + 0.001), false),
            (Vec3::new(0.0, 0.0, -1.0 - radius + 0.001), true),
            (Vec3::new(0.0, 0.0, -1.0 - radius - 0.001), false),
        ] {
            let offered = candidates(&[gun], radius, [true, false], [Some(point), None]);
            assert_eq!(
                resolved_index(&offered, point).is_some(),
                inside,
                "{point:?}"
            );
            assert_eq!(chosen(&[gun], point, radius).is_some(), inside, "{point:?}");
        }
    }

    #[test]
    fn adjacent_guns_keep_nearest_and_equal_distance_ordering() {
        let a = target(TargetKind::Rack { weapon: 3 }, Vec3::ZERO, Vec3::NEG_Z);
        let b = target(
            TargetKind::Held { character: 4 },
            Vec3::X * 0.4,
            Vec3::X * 0.4 + Vec3::NEG_Z,
        );
        for targets in [[a, b], [b, a]] {
            for (x, expected) in [(0.19, a), (0.21, b)] {
                let point = Vec3::new(x, 0.0, -0.5);
                let offered = candidates(&targets, 0.3, [true, false], [Some(point), None]);
                let index = resolved_index(&offered, point).unwrap();
                assert_eq!(targets[index], expected);
                assert_eq!(chosen(&targets, point, 0.3), Some(expected));
            }
            let point = Vec3::new(0.2, 0.0, -0.5);
            let offered = candidates(&targets, 0.3, [true, false], [Some(point), None]);
            assert_eq!(resolved_index(&offered, point), Some(0));
            assert_eq!(chosen(&targets, point, 0.3), Some(targets[0]));
        }
    }

    #[test]
    fn a_direct_world_weapon_keeps_priority_over_a_nearer_remote_offer() {
        let gun = target(TargetKind::Rack { weapon: 3 }, Vec3::ZERO, Vec3::NEG_Z);
        let point = Vec3::new(0.2, 0.0, -0.5);
        let direct = candidates(&[gun], 0.3, [true, false], [Some(point), None])[0];
        let marker = Mark::ring(MarkAt::Cockpit(direct.centre), 0.06);
        let targets = [
            Candidate {
                volume: GrabVolume::Snatch(RemoteKind::Weapon),
                centre: point,
                ..direct
            }
            .target(marker),
            direct.target(marker),
        ];
        let selected = resolve::resolve(&targets, [Some(point), None])[0]
            .chosen
            .unwrap();
        assert_eq!(selected.index, 1);
        assert_eq!(selected.tier, Tier::Target);
    }

    #[test]
    fn the_real_physical_weapon_pickup_keeps_direct_nearest_priority() {
        let point = Vec3::ZERO;
        let marker = Mark::ring(MarkAt::Cockpit(point), 0.06);
        let targets = [
            Candidate {
                volume: GrabVolume::WorldWeapon,
                centre: Vec3::X * 0.2,
                radius: 0.3,
                hand: Some(Hand::Left),
            }
            .target(marker),
            Candidate {
                volume: GrabVolume::Pickup(PickupKind::Weapon),
                centre: Vec3::X * 0.01,
                radius: 0.05,
                hand: Some(Hand::Left),
            }
            .target(marker),
            Candidate {
                volume: GrabVolume::Snatch(RemoteKind::Weapon),
                centre: point,
                radius: 1.0,
                hand: Some(Hand::Left),
            }
            .target(marker),
        ];
        let chosen = resolve::resolve(&targets, [Some(point), None])[0]
            .chosen
            .unwrap();
        assert_eq!(chosen.volume, GrabVolume::Pickup(PickupKind::Weapon));
        assert_eq!(chosen.tier, Tier::Target);
    }

    #[test]
    fn completion_keeps_offered_a_when_b_becomes_nearer() {
        let a = target(TargetKind::Rack { weapon: 3 }, Vec3::ZERO, Vec3::ZERO);
        let b = WorldTarget {
            weapon: 4,
            ..target(TargetKind::Rack { weapon: 4 }, Vec3::X * 0.4, Vec3::X * 0.4)
        };
        let mut offers = Offers::gather(&[a, b], 0.3, [true, false], [Some(Vec3::X * 0.1), None]);
        let moved = Vec3::X * 0.25;
        assert_eq!(chosen(&[a, b], moved, 0.3), Some(b));
        let result = complete_offer(offers.take(Hand::Left, Some(moved), 0.3), |cached| {
            assert_eq!(cached, a);
            Ok::<_, ()>(cached.weapon)
        });
        assert_eq!(result, Ok(Some(a.weapon)));
        assert_eq!(offers.take(Hand::Left, Some(moved), 0.3), None);
    }

    #[test]
    fn completion_rejects_invalid_source_ownership_and_state_without_reselection() {
        #[derive(Debug, Copy, Clone, PartialEq)]
        enum Refusal {
            Gone,
            Ownership,
            State,
        }
        let a = target(TargetKind::Held { character: 3 }, Vec3::ZERO, Vec3::ZERO);
        for refusal in [Refusal::Gone, Refusal::Ownership, Refusal::State] {
            let mut offers = Offers::gather(&[a], 0.3, [true, false], [Some(Vec3::ZERO), None]);
            let result = complete_offer(offers.take(Hand::Left, Some(Vec3::ZERO), 0.3), |cached| {
                assert_eq!(cached, a);
                Err::<(), _>(refusal)
            });
            assert_eq!(result, Err(refusal));
            assert_eq!(offers.take(Hand::Left, Some(Vec3::ZERO), 0.3), None);
        }
    }

    #[test]
    fn cache_clear_and_completion_bounds_do_not_restore_slack_or_old_offers() {
        let a = target(TargetKind::Rack { weapon: 3 }, Vec3::ZERO, Vec3::ZERO);
        for point in [Some(Vec3::X * 0.301), Some(Vec3::NAN), None] {
            let mut offers = Offers::gather(&[a], 0.3, [true, false], [Some(Vec3::ZERO), None]);
            let result = complete_offer(offers.take(Hand::Left, point, 0.3), |_| {
                panic!("an out-of-reach offer must not call the source consumer")
            });
            assert_eq!(result, Ok::<Option<()>, ()>(None));
            assert_eq!(offers.take(Hand::Left, Some(Vec3::ZERO), 0.3), None);
        }
        let mut offers = Offers::gather(&[a], 0.3, [true; 2], [Some(Vec3::ZERO); 2]);
        offers.clear();
        for hand in Hand::BOTH {
            assert_eq!(offers.take(hand, Some(Vec3::ZERO), 0.3), None);
        }
    }

    #[test]
    fn only_one_hand_completes_the_same_finite_rack_offer() {
        let a = target(TargetKind::Rack { weapon: 3 }, Vec3::ZERO, Vec3::ZERO);
        let mut offers = Offers::gather(&[a], 0.3, [true; 2], [Some(Vec3::ZERO); 2]);
        let taken = offers.take(Hand::Left, Some(Vec3::ZERO), 0.3).unwrap();
        assert_eq!(
            complete_offer(Some(taken), |_| Ok::<_, ()>(())),
            Ok(Some(()))
        );
        offers.acquired(taken);
        assert_eq!(offers.take(Hand::Right, Some(Vec3::ZERO), 0.3), None);
    }

    #[test]
    fn unusable_targets_and_radii_offer_nothing() {
        let valid = target(TargetKind::Rack { weapon: 3 }, Vec3::ZERO, Vec3::NEG_Z);
        let invalid = target(TargetKind::Rack { weapon: 4 }, Vec3::NAN, Vec3::ZERO);
        let points = [Some(Vec3::ZERO), None];
        for radius in [0.0, -0.1, f32::NAN, f32::INFINITY] {
            assert!(candidates(&[valid], radius, [true; 2], points).is_empty());
            assert_eq!(chosen(&[valid], Vec3::ZERO, radius), None);
        }
        assert!(candidates(&[invalid], 0.3, [true; 2], points).is_empty());
        assert_eq!(chosen(&[invalid], Vec3::ZERO, 0.3), None);
        assert!(candidates(&[valid], 0.3, [true; 2], [Some(Vec3::NAN), None]).is_empty());
    }

    fn handover(point: Vec3) -> Handover {
        Handover {
            holder: Hand::Right,
            grip: Vec3::ZERO,
            radius: HoldConfig::new().handle_radius_m,
            free: [true, false],
            points: [Some(point), Some(Vec3::ZERO)],
            contact: None,
        }
    }

    #[test]
    fn transfer_reach_includes_its_boundary_but_not_the_foregrip() {
        let radius = HoldConfig::new().handle_radius_m;
        for (distance, inside) in [
            (radius - 0.001, true),
            (radius, true),
            (radius + 0.001, false),
        ] {
            let point = Vec3::X * distance;
            let offer = handover_candidate(handover(point)).unwrap();
            assert_eq!(offer.volume, GrabVolume::Handle);
            assert_eq!(resolved_index(&[offer], point).is_some(), inside);
        }
        let grip = handover_candidate(Handover {
            contact: Some(GunRegion::Grip),
            ..handover(Vec3::ZERO)
        });
        assert!(grip.is_some());
        for region in [
            GunRegion::Stock,
            GunRegion::Magazine,
            GunRegion::Action,
            GunRegion::Handguard,
            GunRegion::Barrel,
        ] {
            assert!(
                handover_candidate(Handover {
                    contact: Some(region),
                    ..handover(Vec3::ZERO)
                })
                .is_none()
            );
        }
    }

    #[test]
    fn a_nearer_foregrip_wins_over_the_transfer_sphere() {
        let point = Vec3::X * 0.1;
        let handle = handover_candidate(handover(point)).unwrap();
        let brace = Candidate {
            volume: GrabVolume::Foregrip,
            centre: point + Vec3::X * 0.01,
            radius: 0.05,
            hand: Some(Hand::Left),
        };
        assert_eq!(resolved_index(&[handle, brace], point), Some(1));
    }

    #[test]
    fn transfer_needs_a_free_tracked_receiver_and_a_usable_radius() {
        assert!(
            handover_candidate(Handover {
                free: [false; 2],
                ..handover(Vec3::ZERO)
            })
            .is_none()
        );
        assert!(
            handover_candidate(Handover {
                points: [None; 2],
                ..handover(Vec3::ZERO)
            })
            .is_none()
        );
        for radius in [0.0, -0.1, f32::NAN, f32::INFINITY] {
            assert!(
                handover_candidate(Handover {
                    radius,
                    ..handover(Vec3::ZERO)
                })
                .is_none()
            );
        }
        let right = handover_candidate(Handover {
            holder: Hand::Left,
            free: [false, true],
            ..handover(Vec3::ZERO)
        })
        .unwrap();
        assert_eq!(right.hand, Some(Hand::Right));
    }

    fn held_pistol(address: usize, holder: Hand, hands: [bool; 2]) -> HeldWeapon {
        HeldWeapon {
            address,
            wielded: Wielded {
                slot: WeaponSlot::E_SLOT_DUAL_WIELD,
                hands,
            },
            holder: Some(holder),
            two_handed: false,
            class: HeftClass::Sidearm,
        }
    }

    #[test]
    fn broad_transfer_is_independent_of_region_geometry_but_respects_contact_and_foregrip() {
        let guns = [None, Some(held_pistol(1, Hand::Right, [false, true]))];
        let policy = |contact, engaged_foregrip| HandoverPolicy {
            allowed: true,
            engaged_foregrip,
            guns,
            offer: Handover {
                contact,
                ..handover(Vec3::ZERO)
            },
        };
        // No classifier result covers missing geometry and disabled region classification.
        assert!(broad_handover_candidate(policy(None, false)).is_some());
        assert!(broad_handover_candidate(policy(Some(GunRegion::Grip), false)).is_some());
        assert!(broad_handover_candidate(policy(None, true)).is_none());
        for region in [
            GunRegion::Stock,
            GunRegion::Magazine,
            GunRegion::Action,
            GunRegion::Handguard,
            GunRegion::Barrel,
        ] {
            assert!(broad_handover_candidate(policy(Some(region), false)).is_none());
        }
        assert!(
            broad_handover_candidate(HandoverPolicy {
                allowed: false,
                ..policy(None, false)
            })
            .is_none()
        );
    }

    #[test]
    fn native_pairs_and_two_separate_pistols_leave_no_transfer_receiver() {
        let pair = held_pistol(1, Hand::Right, [true; 2]);
        let left = held_pistol(2, Hand::Left, [true, false]);
        let right = held_pistol(3, Hand::Right, [false, true]);
        for (guns, gun_hands, armed_hands) in [
            (
                engine::in_hands(Some(pair), [None; 2]),
                pair.wielded.hands,
                [false; 2],
            ),
            (
                engine::in_hands(Some(right), [Some(left), None]),
                right.wielded.hands,
                [true, false],
            ),
        ] {
            let free =
                Hand::BOTH.map(|hand| !gun_hands[hand.index()] && !armed_hands[hand.index()]);
            assert_eq!(engine::lone_gun(guns, free), None);
            assert!(
                broad_handover_candidate(HandoverPolicy {
                    allowed: true,
                    engaged_foregrip: false,
                    guns,
                    offer: Handover {
                        free,
                        ..handover(Vec3::ZERO)
                    },
                })
                .is_none()
            );
        }
        assert!(
            broad_handover_candidate(HandoverPolicy {
                allowed: true,
                engaged_foregrip: false,
                guns: [None, Some(right)],
                offer: Handover {
                    free: [false; 2],
                    ..handover(Vec3::ZERO)
                },
            })
            .is_none()
        );
    }

    #[test]
    fn a_measured_grip_follows_the_current_wrist_at_player_scale() {
        let local = Vec3::new(0.03, -0.08, 0.02);
        let placed = Pose {
            position: Vec3::new(80.0, 12.0, -30.0),
            orientation: Quat::from_rotation_y(0.8),
        };
        let current = Pose {
            position: Vec3::new(0.4, 1.2, -0.6),
            orientation: Quat::from_rotation_x(-0.6),
        };
        for scale in [0.5, 1.0, 2.0] {
            let world = placed.position + placed.orientation * (local * scale);
            let expected = current.position + current.orientation * local;
            let point = grip_on_current_wrist(world, placed, current, scale).unwrap();
            assert!(point.distance(expected) < 1e-5, "{point:?}");
        }
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                grip_on_current_wrist(Vec3::ZERO, placed, current, scale),
                None
            );
        }
        assert_eq!(grip_on_current_wrist(Vec3::NAN, placed, current, 1.0), None);
    }

    #[test]
    fn world_acquisition_is_invariant_under_the_cockpit_transform() {
        let point = Vec3::new(0.29, 0.0, -0.5);
        let radius = HoldConfig::new().take_radius_m;
        for scale in [0.5, 1.0, 2.0] {
            let frames = Frames {
                head_world: Vec3::new(80.0, 12.0, -30.0),
                head_cockpit: Vec3::new(0.1, 1.7, 0.2),
                view: Quat::from_rotation_y(0.8),
                world_scale: scale,
                character: Mat4::IDENTITY,
            };
            let points = [Vec3::ZERO, Vec3::NEG_Z]
                .map(|p| frames.world_to_cockpit(frames.cockpit_to_world(p)));
            let gun = target(TargetKind::Rack { weapon: 3 }, points[0], points[1]);
            let offered = candidates(&[gun], radius, [true, false], [Some(point), None]);
            assert_eq!(resolved_index(&offered, point), Some(0));
            assert_eq!(chosen(&[gun], point, radius), Some(gun));
            let world_distance = segment_distance(
                frames.cockpit_to_world(point),
                frames.cockpit_to_world(Vec3::ZERO),
                frames.cockpit_to_world(Vec3::NEG_Z),
            );
            assert!((world_distance / scale - 0.29).abs() < 1e-5);
        }
    }

    #[test]
    fn saved_reach_is_respected_and_missing_reach_uses_the_new_defaults() {
        let defaults: HoldConfig = toml::from_str("").unwrap();
        let saved: HoldConfig =
            toml::from_str("take_radius_m = 0.10\nhandle_radius_m = 0.07\n").unwrap();
        assert_eq!(defaults.take_radius_m, 0.3);
        assert_eq!(defaults.handle_radius_m, 0.14);
        let gun = target(TargetKind::Rack { weapon: 3 }, Vec3::ZERO, Vec3::NEG_Z);
        let point = Vec3::new(0.2, 0.0, -0.5);
        assert!(chosen(&[gun], point, defaults.take_radius_m).is_some());
        assert_eq!(chosen(&[gun], point, saved.take_radius_m), None);
        let point = Vec3::X * 0.1;
        for (radius, inside) in [
            (defaults.handle_radius_m, true),
            (saved.handle_radius_m, false),
        ] {
            let offer = handover_candidate(Handover {
                radius,
                ..handover(point)
            })
            .unwrap();
            assert_eq!(resolved_index(&[offer], point).is_some(), inside);
        }
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
