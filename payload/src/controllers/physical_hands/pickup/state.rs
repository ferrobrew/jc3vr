//! The pickups' state across ticks: each hand's hold, the grip it keeps on what it cannot simply
//! carry, and what the input tick last asked of it.

use std::{num::NonZeroUsize, time::Instant};

use crate::controllers::{
    fingers::grip::GripSource,
    frame::Hand,
    grab::PickupKind,
    physical_hands::{
        characters::{Regrip, Seize},
        doors::Latch,
        grasp::{Grip, Reach, TargetKind},
    },
};

/// The pickups' state across ticks.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PickupState {
    pub(crate) held: [Option<Held>; 2],
    /// Each hand's grip on a latched door, before the latch gives.
    pub(crate) latches: [Option<Latch>; 2],
    /// Each hand's grip on a character.
    pub(crate) seizes: [Option<Seize>; 2],
    /// Each hand waiting to take hold of a character it yanked off its feet.
    pub(crate) regrips: [Option<Regrip>; 2],
    /// The hands that yanked a seated character, for the input tick to click the vehicle entry.
    pub(crate) hijack_clicks: [bool; 2],
    pub(crate) requests: [Option<Request>; 2],
    /// When the input tick last acted on the grips.
    pub(crate) input_at: Option<Instant>,
}

impl PickupState {
    pub(crate) const fn new() -> Self {
        Self {
            held: [None; 2],
            latches: [None; 2],
            seizes: [None; 2],
            regrips: [None; 2],
            hijack_clicks: [false; 2],
            requests: [None; 2],
            input_at: None,
        }
    }

    pub(crate) fn holding(&self, hand: Hand) -> bool {
        self.held[hand.index()].is_some()
    }

    /// What kind of target the object `hand` holds is, when it holds one.
    pub(crate) fn held_kind(&self, hand: Hand) -> Option<PickupKind> {
        self.held[hand.index()].map(|held| match held.kind {
            HeldKind::Prop => PickupKind::Prop,
            HeldKind::Articulated => PickupKind::Hinged,
            HeldKind::Door => PickupKind::Door,
            HeldKind::Ragdoll => PickupKind::Character,
        })
    }

    /// Whether `hand` grips a latched door.
    pub(crate) fn latched(&self, hand: Hand) -> bool {
        self.latches[hand.index()].is_some()
    }

    /// Whether `hand` carries what it holds, rather than holding it through a joint.
    pub(crate) fn carrying(&self, hand: Hand) -> bool {
        self.held[hand.index()].is_some_and(|h| !h.joined)
    }

    /// What `hand` holds, for its finger pose: the grip's source, and the held body when the hand
    /// holds one rather than gripping a latched door or a character.
    pub(crate) fn grip(&self, hand: Hand) -> Option<(GripSource, Option<u32>)> {
        let i = hand.index();
        if let Some(held) = self.held[i] {
            return Some((held.kind.source(), Some(held.body)));
        }
        if self.latches[i].is_some() {
            return Some((GripSource::Door, None));
        }
        (self.seizes[i].is_some() || self.regrips[i].is_some())
            .then_some((GripSource::Character, None))
    }

    /// The loose object each hand carries, by [`Hand::index`]: a hold of a prop, not a grip on
    /// something hinged, a door, or a character.
    pub(crate) fn props(&self) -> [Option<NonZeroUsize>; 2] {
        self.held
            .map(|h| h.filter(|h| h.kind == HeldKind::Prop).map(|h| h.object))
    }

    /// The objects the hands hold, as addresses, zero for none.
    pub(crate) fn held_objects(&self) -> [usize; 2] {
        self.held.map(|h| h.map_or(0, |h| h.object.get()))
    }

    /// Let go of everything without throwing, and forget the requests.
    pub(crate) fn drop_all(&mut self) {
        self.held = [None; 2];
        self.latches = [None; 2];
        self.seizes = [None; 2];
        self.regrips = [None; 2];
        self.requests = [None; 2];
    }
}

/// One hand's hold.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Held {
    pub(crate) object: NonZeroUsize,
    pub(crate) body: u32,
    pub(crate) grip: Grip,
    pub(crate) mass_kg: f32,
    /// Whether the hand holds the object through a joint rather than carrying it.
    pub(crate) joined: bool,
    pub(crate) kind: HeldKind,
}

impl Held {
    pub(crate) fn same(&self, other: &Held) -> bool {
        self.object == other.object && self.body == other.body
    }
}

/// What a held body is to the hand, for its finger pose.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum HeldKind {
    /// A loose object.
    Prop,
    /// An object hinged to the world or another body.
    Articulated,
    /// A vehicle's door, held once open.
    Door,
    /// A body of a character's ragdoll.
    Ragdoll,
}

impl HeldKind {
    pub(crate) fn of(kind: &TargetKind) -> Self {
        match kind {
            TargetKind::Articulated { .. } => HeldKind::Articulated,
            TargetKind::Door(_) => HeldKind::Door,
            TargetKind::Character(_) => HeldKind::Ragdoll,
            TargetKind::Prop { .. } | TargetKind::Weapon => HeldKind::Prop,
        }
    }

    const fn source(self) -> GripSource {
        match self {
            HeldKind::Prop => GripSource::Pickup,
            HeldKind::Articulated => GripSource::Articulated,
            HeldKind::Door => GripSource::Door,
            HeldKind::Ragdoll => GripSource::Character,
        }
    }
}

/// What the input tick asked of a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum Request {
    Grab(Reach),
    Release,
}
