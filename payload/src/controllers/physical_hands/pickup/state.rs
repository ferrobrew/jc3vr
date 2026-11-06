//! The pickups' state across ticks: each hand's hold, the grip it keeps on what it cannot simply
//! carry, and what the input tick last asked of it.

use std::{num::NonZeroUsize, time::Instant};

use glam::Vec3;

use crate::controllers::{
    fingers::grip::GripSource,
    frame::Hand,
    grab::PickupKind,
    physical_hands::{
        characters::{Regrip, Seize},
        doors::{Latch, tearing::TearGrip},
        grasp::{Grip, Reach, Strain, TargetKind},
        pickup::diagnostics::{self, ConsumerResult, RequestKey, scan::Scan},
    },
};

/// The pickups' state across ticks.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PickupState {
    pub(crate) held: [Option<Held>; 2],
    /// Each hand's grip on a latched door, before the latch gives.
    pub(crate) latches: [Option<Latch>; 2],
    pub(crate) doors: [Option<TearGrip>; 2],
    /// Each hand's grip on a character.
    pub(crate) seizes: [Option<Seize>; 2],
    /// Each hand waiting to take hold of a character it threw off their feet.
    pub(crate) regrips: [Option<Regrip>; 2],
    /// The hands that yanked a seated character, for the input tick to click the vehicle entry.
    pub(crate) hijack_clicks: [bool; 2],
    pub(crate) requests: [Option<Request>; 2],
    /// The accepted local request whose hold the simulation still observes.
    pub(crate) evidence_holds: [Option<RequestKey>; 2],
    pub(crate) scans: [Scan; 2],
    /// When the input tick last acted on the grips.
    pub(crate) input_at: Option<Instant>,
    pub(crate) joints_enabled: Option<bool>,
    /// The player character's world velocity at the last tick, from which a carried object takes
    /// on the character's change of velocity.
    pub(crate) carrier_velocity: Option<Vec3>,
    /// How far each hand's hold strains from it, for the body it was measured on.
    pub(crate) strains: [Option<HeldStrain>; 2],
}

impl PickupState {
    pub(crate) const fn new() -> Self {
        Self {
            held: [None; 2],
            latches: [None; 2],
            doors: [None; 2],
            seizes: [None; 2],
            regrips: [None; 2],
            hijack_clicks: [false; 2],
            requests: [None; 2],
            evidence_holds: [None; 2],
            scans: [Scan::new(); 2],
            input_at: None,
            joints_enabled: None,
            carrier_velocity: None,
            strains: [None; 2],
        }
    }

    /// The strain of `hand`'s hold of `held`, fresh for a hold it was not measured on.
    pub(crate) fn strain(&self, hand: Hand, held: &Held) -> Strain {
        self.strains[hand.index()]
            .filter(|s| s.object == held.object && s.body == held.body)
            .map_or_else(Strain::default, |s| s.strain)
    }

    pub(crate) fn set_strain(&mut self, hand: Hand, held: &Held, strain: Strain) {
        self.strains[hand.index()] = Some(HeldStrain {
            object: held.object,
            body: held.body,
            strain,
        });
    }

    /// A second holder inherits the existing body's mode and classification.
    pub(crate) fn body_mode(
        &self,
        object: NonZeroUsize,
        body: u32,
        kind: HeldKind,
        joined: bool,
    ) -> (HeldKind, bool) {
        self.held
            .iter()
            .flatten()
            .find(|h| h.object == object && h.body == body)
            .map_or((kind, kind.joined(joined)), |h| (h.kind, h.joined))
    }

    /// A latch handoff retains the pull by carrying every holder of the same body.
    pub(crate) fn carry_body(&mut self, object: NonZeroUsize, body: u32) {
        for holder in self
            .held
            .iter_mut()
            .flatten()
            .filter(|h| h.object == object && h.body == body)
        {
            holder.joined = false;
            holder.kind = HeldKind::Door;
        }
    }

    /// What `hand`'s grip holds, for a recorded release: what it is, and the held object and its
    /// body when the hold names one.
    pub(crate) fn grip_subject(&self, hand: Hand) -> Option<GripSubject> {
        let i = hand.index();
        if let Some(held) = self.held[i] {
            return Some(GripSubject {
                held: held.kind.label(),
                object: Some(held.object.get()),
                body: Some(held.body),
            });
        }
        let held = if self.latches[i].is_some() {
            "a latched door"
        } else if self.doors[i].is_some() {
            "a door being torn off"
        } else if self.seizes[i].is_some() {
            "a character"
        } else if self.regrips[i].is_some() {
            "a character thrown off their feet"
        } else {
            return None;
        };
        Some(GripSubject {
            held,
            object: None,
            body: None,
        })
    }

    pub(crate) fn holding(&self, hand: Hand) -> bool {
        self.held[hand.index()].is_some()
    }

    /// Whether the hand owns a grip or has a grab waiting for the simulation tick.
    pub(crate) fn has_interaction(&self, hand: Hand) -> bool {
        let Self {
            held,
            latches,
            doors,
            seizes,
            regrips,
            requests,
            hijack_clicks: _,
            evidence_holds: _,
            scans: _,
            input_at: _,
            joints_enabled: _,
            carrier_velocity: _,
            strains: _,
        } = self;
        let i = hand.index();
        held[i].is_some()
            || latches[i].is_some()
            || doors[i].is_some()
            || seizes[i].is_some()
            || regrips[i].is_some()
            || match requests[i] {
                Some(Request::Grab(_) | Request::ObservedGrab(_)) => true,
                Some(Request::Release) | None => false,
            }
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
        for hand in Hand::BOTH {
            self.replace_request(hand, None, ConsumerResult::Cleared);
        }
        self.evidence_holds = [None; 2];
        self.held = [None; 2];
        crate::controllers::physical_hands::impacts::publish([None; 2]);
        self.latches = [None; 2];
        self.doors = [None; 2];
        self.seizes = [None; 2];
        self.regrips = [None; 2];
        self.hijack_clicks = [false; 2];
        self.requests = [None; 2];
        self.carrier_velocity = None;
    }

    /// Preserve the queue's existing replacement semantics and record a displaced local request.
    pub(crate) fn replace_request(
        &mut self,
        hand: Hand,
        request: Option<Request>,
        displaced: ConsumerResult,
    ) {
        if let Some(Request::ObservedGrab(key)) = self.requests[hand.index()] {
            diagnostics::consumed(hand, key, displaced);
        }
        self.requests[hand.index()] = request;
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

/// A hold's [`Strain`], with the body it was measured on.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct HeldStrain {
    object: NonZeroUsize,
    body: u32,
    strain: Strain,
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
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Prop => "a prop",
            Self::Articulated => "an articulated object",
            Self::Door => "a door",
            Self::Ragdoll => "a ragdoll",
        }
    }

    /// Loose props use one carry solve for both hands, rather than fixed wrist joints.
    pub(crate) fn joined(self, requested: bool) -> bool {
        requested && self != Self::Prop
    }

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

/// What a grip held, as [`PickupState::grip_subject`] names it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct GripSubject {
    pub(crate) held: &'static str,
    pub(crate) object: Option<usize>,
    pub(crate) body: Option<u32>,
}

/// What the input tick asked of a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum Request {
    Grab(Reach),
    /// A local pickup with the exact input observation that queued it.
    ObservedGrab(RequestKey),
    Release,
}

impl Request {
    pub(crate) fn grab(self) -> Option<(Reach, Option<RequestKey>)> {
        match self {
            Self::Grab(reach) => Some((reach, None)),
            Self::ObservedGrab(key) => Some((key.target, Some(key))),
            Self::Release => None,
        }
    }
}
