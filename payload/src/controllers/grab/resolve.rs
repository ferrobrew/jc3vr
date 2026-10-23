//! The resolver: what each hand is within reach of among the tick's targets, and the one its grip
//! would take. Pure.

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    grab::{
        GrabVolume,
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
/// A target worked by the palm is never taken. Of equally near ones, the first.
pub fn choose(reaches: impl IntoIterator<Item = Reach>) -> Option<Reach> {
    reaches
        .into_iter()
        .filter(|r| r.tier != Tier::Touch && !r.distance.is_nan())
        .min_by(|a, b| a.tier.cmp(&b.tier).then(a.distance.total_cmp(&b.distance)))
}

/// Why a reach lost to the chosen one, for the grip press's log.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Lost {
    /// A target was in reach, and this is a fallback.
    Fallback,
    /// Something was in reach, and this is out of reach, for the grapple to snatch.
    Remote,
    /// The palm works it, not the grip.
    Touch,
    /// The chosen one's grab point is nearer, by this many metres.
    Farther(f32),
}

impl Lost {
    pub fn describe(self) -> String {
        match self {
            Lost::Fallback => "a fallback, and a target is in reach".to_owned(),
            Lost::Remote => "out of reach, and something is in reach".to_owned(),
            Lost::Touch => "worked by the palm, not the grip".to_owned(),
            Lost::Farther(by) => format!("{:.0} mm farther", by * 1000.0),
        }
    }
}

/// Each of `reaches` but `chosen`, with why it lost to it.
pub fn losers(reaches: &[Reach], chosen: Reach) -> Vec<(Reach, Lost)> {
    reaches
        .iter()
        .filter(|r| r.index != chosen.index)
        .map(|r| {
            let lost = if r.tier == Tier::Touch {
                Lost::Touch
            } else if r.tier == Tier::Remote && chosen.tier != Tier::Remote {
                Lost::Remote
            } else if r.tier > chosen.tier {
                Lost::Fallback
            } else {
                Lost::Farther(r.distance - chosen.distance)
            };
            (*r, lost)
        })
        .collect()
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
