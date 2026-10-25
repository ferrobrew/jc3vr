//! The one representation of something a hand could take ([`GripTarget`]): what a squeeze takes,
//! where its grab point is and how near a hand must come, how it ranks, and how its marker is
//! drawn. Pure.

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    grab::{Category, GrabVolume},
};

/// Something a hand could take, as its provider places it on the input tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GripTarget {
    /// What a squeeze takes, which names the subsystem that acts on it
    /// ([`GrabVolume::consumer`]).
    pub volume: GrabVolume,
    /// The one hand that may take it, or `None` for either.
    pub hand: Option<Hand>,
    pub tier: Tier,
    /// How near a hand must come to take it.
    pub reach: TargetReach,
    /// The hand that holds it now, if one does: a held handle, a pressed button, a braced foregrip.
    pub held_by: Option<Hand>,
    /// How its marker is drawn.
    pub mark: Mark,
    /// The physical provider's already-observed target, when supplied. Diagnostic only.
    pub(crate) observed: Option<crate::controllers::physical_hands::grasp::Reach>,
    /// A bounded provider-owned offer for the current wrist summary, separate from diagnostics.
    pub(crate) door_eligibility: Option<crate::controllers::physical_hands::DoorEligibility>,
}

impl GripTarget {
    /// A target a hand takes within `radius` of `centre`, a point in the cockpit frame reached by
    /// the hand's grip as it is drawn, marked at `centre` the way `mark` says.
    pub fn sphere(
        volume: GrabVolume,
        hand: Option<Hand>,
        centre: Vec3,
        radius: f32,
        mark: Mark,
    ) -> Self {
        Self {
            volume,
            hand,
            tier: Tier::of(volume),
            reach: TargetReach::Sphere { centre, radius },
            held_by: None,
            mark,
            observed: None,
            door_eligibility: None,
        }
    }

    /// A target `hand` is `distance` metres from, as its provider measured it.
    pub fn measured(volume: GrabVolume, hand: Hand, distance: f32, mark: Mark) -> Self {
        Self {
            volume,
            hand: Some(hand),
            tier: Tier::of(volume),
            reach: TargetReach::Measured { distance },
            held_by: None,
            mark,
            observed: None,
            door_eligibility: None,
        }
    }

    /// This target, held by `hand`: it keeps it whatever else is near.
    pub fn held(self, hand: Hand) -> Self {
        Self {
            hand: Some(hand),
            reach: TargetReach::Measured { distance: 0.0 },
            held_by: Some(hand),
            ..self
        }
    }

    /// This target, ranked as `tier`.
    pub fn ranked(self, tier: Tier) -> Self {
        Self { tier, ..self }
    }

    /// Whether `hand` may take it.
    pub fn for_hand(&self, hand: Hand) -> bool {
        self.hand.is_none_or(|h| h == hand)
    }

    /// Its kind, for the marker's colour.
    pub fn category(&self) -> Category {
        self.volume.category()
    }
}

/// How a target ranks against the others a hand is within reach of.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Taken by the nearest-grab-point rule.
    Target,
    /// Taken only when no target is in reach: a surface to climb, the helicopter to hang from, and
    /// a pouch or a parachute handle the hand misses by a little.
    Fallback,
    /// Taken only when nothing at all is in reach, not even a fallback: a thing out of reach that
    /// the grapple snatches into the hand (see [`crate::controllers::snatch`]).
    Remote,
    /// Worked by the hand without a squeeze, such as a cockpit button or a gun's magazine well:
    /// marked, never taken.
    Touch,
}

impl Tier {
    /// The tier a target of `volume` ranks in by default.
    pub const fn of(volume: GrabVolume) -> Self {
        match volume {
            GrabVolume::Climb | GrabVolume::StuntHold => Tier::Fallback,
            GrabVolume::Snatch(_) => Tier::Remote,
            GrabVolume::CockpitButton(_) | GrabVolume::MagazineWell | GrabVolume::ChargeSurface => {
                Tier::Touch
            }
            _ => Tier::Target,
        }
    }
}

/// How near a hand must come to a target.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TargetReach {
    /// Within `radius` metres of `centre`, a point in the cockpit frame, measured from the hand's
    /// grip as it is drawn.
    Sphere { centre: Vec3, radius: f32 },
    /// The provider's own reach found the target `distance` metres from the hand, at the player's
    /// scale: the physical hands' casts into the world.
    Measured { distance: f32 },
}

/// How a target's marker is drawn.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Mark {
    pub at: MarkAt,
    /// The surface the marker lies on, its normal in the frame of [`at`](Self::at); `None` turns
    /// it toward the eye.
    pub normal: Option<Vec3>,
    /// The marker's diameter, metres at the player's scale.
    pub size: f32,
    pub style: MarkStyle,
    pub shown: Shown,
}

impl Mark {
    /// A ring of `size` at `at`, facing the eye, shown while a hand is within reach.
    pub const fn ring(at: MarkAt, size: f32) -> Self {
        Self {
            at,
            normal: None,
            size,
            style: MarkStyle::Ring,
            shown: Shown::InReach,
        }
    }

    /// The same marker, lying on the surface with `normal`.
    pub const fn on_surface(self, normal: Vec3) -> Self {
        Self {
            normal: Some(normal),
            ..self
        }
    }

    pub const fn styled(self, style: MarkStyle) -> Self {
        Self { style, ..self }
    }

    pub const fn showing(self, shown: Shown) -> Self {
        Self { shown, ..self }
    }
}

/// Where a marker is, anchored at render time
/// (`docs/mod/rendering/render-time-anchoring.md`).
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum MarkAt {
    /// A point in the cockpit frame that rides with the player, anchored to the render-time head.
    Cockpit(Vec3),
    /// A point in the cockpit frame on what `hand` holds, anchored to that hand's render-time grip.
    OnHand { hand: Hand, point: Vec3 },
    /// A point that lies in the world.
    World(Vec3),
}

/// A marker's shape.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MarkStyle {
    /// A ring around the grab point.
    Ring,
    /// A dot at the grab point, ringed while a hand is within reach: a handle or a toggle to reach
    /// for.
    Handle,
    /// A disc lying on a face, ringed while a hand is in front of it: a button.
    Button,
    /// A small disc in the refusal colour: a place on the body that does not take what the hand
    /// holds, shown while the hand is near so that it knows before it opens.
    NoFit,
}

/// When a marker shows.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Shown {
    /// Always, faintly while no hand is within reach: something on the body or in the cockpit, to
    /// be found without looking for it.
    Always,
    /// While a hand is within reach.
    InReach,
    /// Never: a reach margin around a target that has a marker of its own.
    Never,
}
