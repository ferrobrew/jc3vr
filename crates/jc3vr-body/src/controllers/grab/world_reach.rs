//! What the physical hands' reach found in the world, as the grip targets carry it: the object and
//! body a hand reached, what kind of thing it is (a loose or hinged object, a weapon, a vehicle's
//! door, or a character), and the offer of a latched door that the controls summary checks. The
//! physical hands find and act on them (the physical crate's `controllers::physical_hands`). Pure.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::vehicle::vehicle::DoorSlot;
use jc3vr_core::published::Stamp;
use jc3vr_input::frame::Hand;

/// An object within a hand's reach.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Reach {
    /// The object's address.
    pub object: NonZeroUsize,
    /// The reached body's Havok id.
    pub body: u32,
    /// Where the hand reached the object, world space.
    pub point: Vec3,
    /// How far the point is from the palm, metres.
    pub distance: f32,
    /// The surface's normal at the point, toward the palm; zero where none is known.
    pub normal: Vec3,
    pub kind: TargetKind,
}

/// What a reachable object is to a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TargetKind {
    /// A loose rigid object, held in the hand, of this mass in kilograms.
    Prop { mass_kg: f32 },
    /// An object joined to the world or another body by a constraint, such as a hinged gate or a
    /// lever, of this mass in kilograms, which its constraint carries.
    Articulated { mass_kg: f32 },
    /// A weapon lying in the world, collected through the game's own pickup.
    Weapon,
    /// A vehicle's door, hood, trunk, or hatch: an open one held as a loose object on its hinge, a
    /// closed one gripped at its handle area until the latch gives.
    Door(DoorTarget),
    /// A character, held on to until a yank.
    Character(CharacterTarget),
}

/// A door a hand can take.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DoorTarget {
    /// The vehicle the door belongs to.
    pub vehicle: NonZeroUsize,
    /// The vehicle's body the reach found, which proves the vehicle alive while the hand grips a
    /// latched door; `None` for a hit on an open door's own body.
    pub vehicle_body: Option<u32>,
    pub part_index: u32,
    pub slot: DoorSlot,
    /// Whether the door is merged into the vehicle's body, latched shut.
    pub latched: bool,
    /// Which way is out of the vehicle at the door, world space.
    pub outward: Vec3,
}

/// A character a hand can take hold of.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CharacterTarget {
    pub character: NonZeroUsize,
    /// The body the reach found, which proves the character alive while the hand holds on.
    pub body: u32,
    /// The vehicle attachment and its access through an open door, a broken window, or an exposed
    /// seat.
    pub seated: Option<Seated>,
    /// Whether the body is one of the character's ragdoll's, which the hand holds as a loose
    /// object, rather than its capsule.
    pub ragdoll: bool,
    /// Whether the player's faction is hostile to the character.
    pub hostile: bool,
    /// The whole ragdoll's mass, kilograms, for a body of its ragdoll: what holding that one body
    /// drags. Zero for the capsule.
    pub mass_kg: f32,
}

/// A character attached to a vehicle, and how the hand reaches them.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Seated {
    pub vehicle: NonZeroUsize,
    pub access: SeatAccess,
}

/// How a hand reaches a seated character.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SeatAccess {
    /// Through an open door beside the hand.
    Door {
        slot: DoorSlot,
        /// How far the door stands open, degrees.
        open_deg: f32,
    },
    /// Through a window of the vehicle the glass brought down (the physical crate's
    /// `controllers::physical_hands::characters::pane`).
    Pane,
    /// An exposed motorcycle occupant, who needs no door.
    Exposed,
}

impl SeatAccess {
    /// The open door the hand reaches through, and how far it stands open, degrees.
    pub fn door(self) -> Option<(DoorSlot, f32)> {
        match self {
            SeatAccess::Door { slot, open_deg } => Some((slot, open_deg)),
            SeatAccess::Pane | SeatAccess::Exposed => None,
        }
    }
}

/// An unheld latched-door offer from one fresh physical provider sample. Only the provider creates
/// it; final summary capture checks that the same source and reach remain unclaimed after finish.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DoorEligibility {
    hand: Hand,
    epoch: u64,
    at: Stamp,
    reach: Reach,
}

impl DoorEligibility {
    /// The offer `hand` takes of the door `reach` found, from the provider's sample `at` of its
    /// source `epoch`.
    pub const fn new(hand: Hand, epoch: u64, at: Stamp, reach: Reach) -> Self {
        Self {
            hand,
            epoch,
            at,
            reach,
        }
    }

    pub const fn hand(self) -> Hand {
        self.hand
    }

    /// The provider's source the offer came from.
    pub const fn epoch(self) -> u64 {
        self.epoch
    }

    /// The provider's sample the offer came from.
    pub const fn at(self) -> Stamp {
        self.at
    }

    pub const fn reach(self) -> Reach {
        self.reach
    }
}
