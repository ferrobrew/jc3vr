//! Acquisition and lifetime of a target before its physical flight.

use std::{cell::Cell, num::NonZeroUsize};

use glam::Vec3;
use jc3vr_body::controllers::{
    grab::world_reach::DoorTarget,
    physics::{grapple_hook::BodyHit, refs::Strong, vehicles::vehicle_frame},
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::Hand;
use jc3vr_weapons::controllers::{
    loadout::TakeSource,
    weapons::{
        c4::placed,
        hold::{
            source as hold_source,
            source::{Freed, Ticket, WeaponSource, Yield},
            steal::CharacterGun,
            tuning::HOLD,
        },
    },
};

use crate::controllers::snatch::{
    Refusal, SNATCH, charge, engine,
    pull::fire_hook,
    rules::PropHandling,
    target::{Offer, OfferKind},
};

/// What the snatch fired at, with its lifetime checked throughout the flight.
pub(super) struct Grabbed {
    /// The physics game object the hook goes to.
    pub object: NonZeroUsize,
    pub kind: Pulled,
    pub(super) hit: Cell<Option<BodyHit>>,
    pub(super) object_local: Option<Vec3>,
    pub(super) alive: Lifetime,
}

/// The reference appropriate to the target's storage. Pooled charges must not retain a strong
/// reference across the ammunition manager's forced removal.
pub(super) enum Lifetime {
    Object { _alive: Strong },
    Charge(charge::FlightCharge),
}

impl Grabbed {
    pub(super) fn catch_charge(&self, hand: Hand) -> bool {
        match &self.alive {
            Lifetime::Charge(charge) if charge.live() => placed::take(hand, charge.identity()),
            _ => false,
        }
    }

    pub(super) fn live(&self) -> bool {
        match &self.alive {
            Lifetime::Object { .. } => true,
            Lifetime::Charge(charge) => charge.live(),
        }
    }
}

/// How a grabbed thing comes.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) enum Pulled {
    /// A weapon lying in the world, taken from `source`, into the hand through the table.
    Weapon { source: TakeSource },
    /// A loose object, body `body` of `mass_kg`: one a hand can carry into the hand's ordinary
    /// hold, or one too heavy, swung on the held line.
    Prop {
        body: u32,
        mass_kg: f32,
        handling: PropHandling,
    },
    /// A ragdolling character's ragdoll, pulled by its body `body` of `owner`, which drags the
    /// whole ragdoll of `mass_kg`: always swung on the held line.
    Ragdoll {
        owner: NonZeroUsize,
        body: u32,
        mass_kg: f32,
    },
    /// An inert grenade, body `body`, into the hand through the loadout.
    Grenade { body: u32 },
    /// A finite native hand grenade lying in the world, body `body`: the catch takes it into the
    /// hand as the grenade it is, as a hand closing on it does.
    WorldGrenade { body: u32 },
    /// A placed charge's native body, kept live and counted until the physical catch.
    Charge { body: u32 },
    /// A vehicle's closed door, pulled open at its handle, `handle` in the vehicle's frame.
    Door { door: DoorTarget, handle: Vec3 },
}

impl Pulled {
    /// Whether the target is too heavy to come to the hand, so the line swings it from the bite.
    pub(super) const fn tethered(&self) -> bool {
        matches!(
            self,
            Pulled::Prop {
                handling: PropHandling::Swing,
                ..
            } | Pulled::Ragdoll { .. }
        )
    }
}

/// What a fire at an armed target started.
pub(super) enum Fired {
    /// A source gave its weapon up, or is making it; the hook fires once it lies in the world.
    Freeing(Freeing),
    /// The hook flies.
    HookOut(Grabbed, NonZeroUsize),
}

/// Fire at the armed target: free a gun from a character's hands or holster first, fire the hook at
/// a weapon, an object, a charge, or a door.
pub(super) fn fire_at(game: &GameTick<'_>, hand: Hand, offer: Offer) -> Result<Fired, Refusal> {
    let hit = offer.hit;
    let object_local = offer.object_local;
    let source = match offer.kind {
        OfferKind::Ground { weak, .. } => WeaponSource::Ground { weak },
        OfferKind::Held(target) => {
            WeaponSource::Character(CharacterGun::of_target(&target).ok_or(Refusal::Gone)?)
        }
        OfferKind::Holstered(target) => WeaponSource::Character(CharacterGun::holstered(&target)),
        OfferKind::Rack { weapon, weak, gun } => WeaponSource::Rack { weapon, weak, gun },
        OfferKind::Prop {
            object,
            body,
            mass_kg,
            handling,
            weak,
        } => {
            let grabbed = Grabbed {
                object,
                kind: Pulled::Prop {
                    body,
                    mass_kg,
                    handling,
                },
                hit: Cell::new(hit),
                object_local,
                alive: Lifetime::Object {
                    _alive: weak.lock().ok_or(Refusal::Gone)?,
                },
            };
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::Ragdoll {
            character,
            owner,
            body,
            mass_kg,
            weak,
        } => {
            let grabbed = Grabbed {
                object: character,
                kind: Pulled::Ragdoll {
                    owner,
                    body,
                    mass_kg,
                },
                hit: Cell::new(hit),
                object_local,
                alive: Lifetime::Object {
                    _alive: weak.lock().ok_or(Refusal::Gone)?,
                },
            };
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::WorldGrenade { weapon, body, weak } => {
            let grabbed = Grabbed {
                object: weapon,
                kind: Pulled::WorldGrenade { body },
                hit: Cell::new(hit),
                object_local,
                alive: Lifetime::Object {
                    _alive: weak.lock().ok_or(Refusal::Gone)?,
                },
            };
            unsafe { engine::dropped(weapon) }?;
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::Grenade { object, body, weak } => {
            let grabbed = Grabbed {
                object,
                kind: Pulled::Grenade { body },
                hit: Cell::new(hit),
                object_local,
                alive: Lifetime::Object {
                    _alive: weak.lock().ok_or(Refusal::Gone)?,
                },
            };
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::PlacedCharge { identity } => {
            let mut flight = unsafe { charge::flight(identity) }.ok_or(Refusal::Gone)?;
            let (body, owner) = unsafe { flight.detach() }.ok_or(Refusal::NoBody)?;
            let hit = unsafe { BodyHit::at(body, owner, offer.point) }.ok_or(Refusal::Gone)?;
            let grabbed = Grabbed {
                object: owner,
                kind: Pulled::Charge { body },
                hit: Cell::new(Some(hit)),
                object_local: None,
                alive: Lifetime::Charge(flight),
            };
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::Door { door, handle, weak } => {
            let alive = weak.lock().ok_or(Refusal::Gone)?;
            let local = unsafe { vehicle_frame(door.vehicle) }
                .inverse()
                .transform_point3(handle);
            let grabbed = Grabbed {
                object: door.vehicle,
                kind: Pulled::Door {
                    door,
                    handle: local,
                },
                hit: Cell::new(hit),
                object_local,
                alive: Lifetime::Object { _alive: alive },
            };
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
    };
    let ground = matches!(source, WeaponSource::Ground { .. });
    let freed = match hold_source::free(game, hand, &source).map_err(Refusal::Source)? {
        Yield::Ready(freed) => freed,
        Yield::Pending(ticket) => return Ok(Fired::Freeing(Freeing::Making(ticket, object_local))),
    };
    let grabbed = grabbed(freed, hit, object_local)?;
    if ground {
        // A weapon lying in the world has its bodies already: the hook flies at once.
        let wire = unsafe { fire_hook(&grabbed) }?;
        return Ok(Fired::HookOut(grabbed, wire));
    }
    tracing::info!(target: "controllers", hand = hand.label(), "snatch: the gun is free of its holder");
    Ok(Fired::Freeing(Freeing::Freed(grabbed)))
}

/// A weapon its source is giving up for the snatch.
pub(super) enum Freeing {
    /// Given up: the hook fires once its bodies lie in the world.
    Freed(Grabbed),
    /// Still being made: a rack's copy, or a pair's pistol, collected once it is.
    Making(Ticket, Option<Vec3>),
}

impl Freeing {
    /// How long the source may take, seconds: a weapon given up lies in the world within a tick
    /// or two, and one being made spawns a few frames later.
    pub(super) fn timeout_s(&self) -> f32 {
        match self {
            Freeing::Freed(_) => SNATCH.get().free_timeout_s,
            Freeing::Making(..) => HOLD.get().hold.rack.copy_wait_ms.duration().as_secs_f32(),
        }
    }
}

/// Collect the weapon a source was making into `freeing`, when it has made it.
pub(super) fn collect(
    game: &GameTick<'_>,
    hand: Hand,
    freeing: Freeing,
) -> Result<Freeing, Refusal> {
    let Freeing::Making(ticket, local) = freeing else {
        return Ok(freeing);
    };
    match hold_source::poll(game, hand, &ticket) {
        None => Ok(Freeing::Making(ticket, local)),
        Some(Ok(freed)) => grabbed(freed, None, local).map(Freeing::Freed),
        Some(Err(refusal)) => Err(Refusal::Source(refusal)),
    }
}

/// A weapon its source gave up, as the snatch holds it.
fn grabbed(
    freed: Freed,
    hit: Option<BodyHit>,
    object_local: Option<Vec3>,
) -> Result<Grabbed, Refusal> {
    Ok(Grabbed {
        object: NonZeroUsize::new(freed.weapon as usize).ok_or(Refusal::Gone)?,
        kind: Pulled::Weapon {
            source: freed.source,
        },
        hit: Cell::new(hit),
        object_local,
        alive: Lifetime::Object {
            _alive: freed.alive,
        },
    })
}
