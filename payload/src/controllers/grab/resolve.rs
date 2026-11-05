//! The resolver: what each hand is within reach of among the tick's targets, and the one its grip
//! would take. Pure.

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    grab::{
        GrabVolume, PickupKind,
        target::{GripTarget, TargetReach, Tier},
    },
};

/// A target within a hand's reach: which of the tick's targets it is, what it is, how far its grab
/// point is from the hand, and how it ranks.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Reach {
    /// Its index in the tick's targets.
    pub index: usize,
    pub volume: GrabVolume,
    /// How far the grab point is from the hand, metres at the player's scale.
    pub distance: f32,
    pub tier: Tier,
}

/// One hand's targets this tick.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Resolved {
    /// Every target the hand may take and is within reach of, in the tick's order.
    pub reaches: Vec<Reach>,
    /// The one its grip would take.
    pub chosen: Option<Reach>,
}

/// Resolve each hand among `targets`, with `grips` each hand's grip as it is drawn (by
/// [`Hand::index`], `None` for an untracked hand).
pub fn resolve(targets: &[GripTarget], grips: [Option<Vec3>; 2]) -> [Resolved; 2] {
    Hand::BOTH.map(|hand| {
        let reaches = reaches(hand, targets, grips[hand.index()]);
        let chosen = choose(reaches.iter().copied());
        Resolved { reaches, chosen }
    })
}

/// Every target of `targets` that `hand`, with its drawn grip at `grip`, may take and is within
/// reach of. A target the provider measured is within reach whenever it is offered; a hand without
/// a grip reaches only those.
pub fn reaches(hand: Hand, targets: &[GripTarget], grip: Option<Vec3>) -> Vec<Reach> {
    targets
        .iter()
        .enumerate()
        .filter(|(_, t)| t.for_hand(hand))
        .filter_map(|(index, t)| {
            let distance = match t.reach {
                TargetReach::Measured { distance } => distance,
                TargetReach::Sphere { centre, radius } => {
                    let point = grip?;
                    if radius.is_nan() || radius <= 0.0 || !point.is_finite() {
                        return None;
                    }
                    let distance = point.distance(centre);
                    if distance > radius {
                        return None;
                    }
                    distance
                }
            };
            (!distance.is_nan()).then_some(Reach {
                index,
                volume: t.volume,
                distance,
                tier: t.tier,
            })
        })
        .collect()
}

/// The one of `reaches` a grip takes: the nearest target, or with no target, the nearest fallback.
/// A character loses to a weapon to take within reach however much nearer the character is, so a
/// hand closing by a gun in a character's hands takes the gun rather than the character holding
/// it. A target worked by the palm is never taken. Of equally near ones, the first.
pub fn choose(reaches: impl IntoIterator<Item = Reach>) -> Option<Reach> {
    let takeable: Vec<Reach> = reaches
        .into_iter()
        .filter(|r| r.tier != Tier::Touch && !r.distance.is_nan())
        .collect();
    let nearest = takeable
        .iter()
        .copied()
        .min_by(|a, b| a.tier.cmp(&b.tier).then(a.distance.total_cmp(&b.distance)))?;
    if nearest.volume != CHARACTER {
        return Some(nearest);
    }
    takeable
        .iter()
        .copied()
        .filter(|r| r.volume == GrabVolume::WorldWeapon && r.tier == nearest.tier)
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
        .or(Some(nearest))
}

/// A character the physical hands' reach found.
const CHARACTER: GrabVolume = GrabVolume::Pickup(PickupKind::Character);

/// Let each hand reach the things lying on the floor among `targets` (a grenade or a magazine,
/// offered to that hand alone) from its controller's grip, `controllers`, where the hand as it is
/// drawn, `drawn`, stays well above it: the character's crouch does not go as low as the player's,
/// so the drawn hand of a player crouched to the floor hangs short of it. Such a target the
/// controller's grip is within reach of, and the drawn grip is not, is offered at the controller's
/// distance. Both grips are in the cockpit frame, whose +Y is up.
pub fn reach_low(
    targets: &mut [GripTarget],
    drawn: [Option<Vec3>; 2],
    controllers: [Option<Vec3>; 2],
) {
    for target in targets.iter_mut() {
        if !matches!(
            target.volume,
            GrabVolume::LiveGrenade | GrabVolume::WorldMagazine
        ) {
            continue;
        }
        let (Some(hand), TargetReach::Sphere { centre, radius }) = (target.hand, target.reach)
        else {
            continue;
        };
        let (Some(drawn), Some(controller)) = (drawn[hand.index()], controllers[hand.index()])
        else {
            continue;
        };
        if drawn.y - controller.y < LOW_REACH_M || drawn.distance(centre) <= radius {
            continue;
        }
        let distance = controller.distance(centre);
        if distance <= radius {
            target.reach = TargetReach::Measured { distance };
        }
    }
}

/// How far below the drawn hand the controller must be for [`reach_low`] to reach from it, metres:
/// more than the drawn hand strays from its controller in ordinary reaches.
const LOW_REACH_M: f32 = 0.1;

/// Why a reach lost to the chosen one, for the grip press's log.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Lost {
    /// A target was in reach, and this is a fallback.
    Fallback,
    /// Something was in reach, and this is out of reach, for the grapple to snatch.
    Remote,
    /// The palm works it, not the grip.
    Touch,
    /// A character, and a weapon to take is in reach.
    Character,
    /// The chosen one's grab point is nearer, by this many metres.
    Farther(f32),
}

impl Lost {
    pub fn describe(self) -> String {
        match self {
            Lost::Fallback => "a fallback, and a target is in reach".to_owned(),
            Lost::Remote => "out of reach, and something is in reach".to_owned(),
            Lost::Touch => "worked by the palm, not the grip".to_owned(),
            Lost::Character => "a character, and a weapon to take is in reach".to_owned(),
            Lost::Farther(by) => format!("{:.0} mm farther", by * 1000.0),
        }
    }
}

/// Each of `reaches` but `chosen`, with why it lost to it.
pub fn losers(reaches: &[Reach], chosen: Reach) -> Vec<(Reach, Lost)> {
    reaches
        .iter()
        .filter(|r| r.index != chosen.index)
        .map(|r| (*r, lost_to(*r, chosen)))
        .collect()
}

/// Why a reached candidate lost to the resolver's actual chosen candidate.
pub(crate) fn lost_to(reach: Reach, chosen: Reach) -> Lost {
    if reach.tier == Tier::Touch {
        Lost::Touch
    } else if reach.tier == Tier::Remote && chosen.tier != Tier::Remote {
        Lost::Remote
    } else if reach.tier > chosen.tier {
        Lost::Fallback
    } else if reach.volume == CHARACTER && chosen.volume == GrabVolume::WorldWeapon {
        Lost::Character
    } else {
        Lost::Farther(reach.distance - chosen.distance)
    }
}

/// The socket something a hand holds docks in when its grip opens: of the targets `hand`, with its
/// drawn grip at `grip`, is within reach of, the nearest that `fits` what it holds. A hand over both
/// a pouch and a socket that fits the gun docks the gun on the socket.
pub fn dock_target(
    hand: Hand,
    targets: &[GripTarget],
    grip: Option<Vec3>,
    fits: impl Fn(GrabVolume) -> bool,
) -> Option<GrabVolume> {
    reaches(hand, targets, grip)
        .into_iter()
        .filter(|r| fits(r.volume))
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
        .map(|r| r.volume)
}

/// The target `hand`, with its drawn grip at `grip`, may take that is nearest to coming within its reach without
/// being there, and how far outside its reach the hand is, metres: for the log of a press that
/// took nothing. Only a target with a reach around a grab point can be missed.
pub fn nearest_miss(
    hand: Hand,
    targets: &[GripTarget],
    grip: Option<Vec3>,
) -> Option<(usize, f32)> {
    let grip = grip?;
    targets
        .iter()
        .enumerate()
        .filter(|(_, t)| t.for_hand(hand) && t.tier != Tier::Touch)
        .filter_map(|(index, t)| match t.reach {
            TargetReach::Sphere { centre, radius } => {
                let outside = grip.distance(centre) - radius;
                (outside > 0.0 && outside.is_finite()).then_some((index, outside))
            }
            TargetReach::Measured { .. } => None,
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The item nearest `grip` whose point (`at`) lies within `radius_m` of it: the one a hand closing
/// there takes among loose things that are not grip targets of their own. A point or a distance
/// that is not finite is never within reach.
pub fn nearest_within<T: Copy>(
    items: &[T],
    at: impl Fn(&T) -> Vec3,
    grip: Vec3,
    radius_m: f32,
) -> Option<T> {
    items
        .iter()
        .map(|item| (at(item).distance(grip), item))
        .filter(|(distance, _)| distance.is_finite() && *distance <= radius_m)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, item)| *item)
}

#[cfg(test)]
mod vehicle_grip_tests {
    use super::*;
    use crate::controllers::grab::{Mark, MarkAt};

    fn on_floor(volume: GrabVolume, hand: Hand) -> GripTarget {
        let centre = Vec3::new(0.2, -1.5, -0.4);
        GripTarget::sphere(
            volume,
            Some(hand),
            centre,
            0.1,
            Mark::ring(MarkAt::Cockpit(centre), 0.06),
        )
    }

    #[test]
    fn a_crouched_reach_to_the_floor_takes_from_the_controller() {
        // The controller at the grenade on the floor; the drawn hand 0.7 m above it.
        let controller = Vec3::new(0.2, -1.47, -0.4);
        let drawn = Vec3::new(0.2, -0.8, -0.35);
        let mut targets = vec![
            on_floor(GrabVolume::LiveGrenade, Hand::Left),
            on_floor(GrabVolume::WorldMagazine, Hand::Left),
            on_floor(GrabVolume::LiveGrenade, Hand::Right),
            on_floor(GrabVolume::Climb, Hand::Left),
        ];
        reach_low(&mut targets, [Some(drawn), None], [Some(controller), None]);
        let left = resolve(&targets, [Some(drawn), None]);
        assert_eq!(
            left[0].chosen.map(|r| r.volume),
            Some(GrabVolume::LiveGrenade)
        );
        assert!((left[0].chosen.unwrap().distance - 0.03).abs() < 1e-4);
        assert!(matches!(targets[1].reach, TargetReach::Measured { .. }));
        // The other hand's target and a target that is not a floor pickup keep their spheres.
        assert!(matches!(targets[2].reach, TargetReach::Sphere { .. }));
        assert!(matches!(targets[3].reach, TargetReach::Sphere { .. }));
    }

    #[test]
    fn a_drawn_hand_level_with_its_controller_reaches_as_drawn() {
        let controller = Vec3::new(0.2, -1.47, -0.4);
        let drawn = controller + Vec3::new(0.0, 0.05, 0.1);
        let mut targets = vec![on_floor(GrabVolume::LiveGrenade, Hand::Left)];
        let before = targets.clone();
        reach_low(&mut targets, [Some(drawn), None], [Some(controller), None]);
        assert_eq!(targets, before);
        // A controller below the drawn hand but out of reach changes nothing either.
        let mut far = before.clone();
        reach_low(
            &mut far,
            [Some(Vec3::new(0.2, -0.8, -0.4)), None],
            [Some(Vec3::new(0.6, -1.4, -0.4)), None],
        );
        assert_eq!(far, before);
    }

    #[test]
    fn only_an_explicit_foregrip_target_outranks_the_climbing_fallback() {
        let climb = Reach {
            index: 0,
            volume: GrabVolume::Climb,
            distance: 0.01,
            tier: Tier::of(GrabVolume::Climb),
        };
        assert_eq!(choose([climb]).map(|r| r.volume), Some(GrabVolume::Climb));
        let foregrip = Reach {
            index: 1,
            volume: GrabVolume::Foregrip,
            distance: 0.1,
            tier: Tier::of(GrabVolume::Foregrip),
        };
        assert_eq!(
            choose([climb, foregrip]).map(|r| r.volume),
            Some(GrabVolume::Foregrip)
        );
    }
}
