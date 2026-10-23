//! What the grapple's aim offers the snatch: when the grapple hand may snatch at all, and the one
//! target under the aim or near its line.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::{character::character::Character, game::GameObject, physics::PhysicsGameObject};

use crate::controllers::{
    aim::{
        grapple_weapons,
        ray::Ray,
        raycast::{self, CastOptions},
    },
    config::ControllersConfig,
    grab::RemoteKind,
    mode::Mode,
    physical_hands::{self, doors::DoorTarget, remote},
    snatch::{
        OfferInput, Refusal, engine,
        rules::{self, Candidate},
    },
    weapons::hold::{
        refs::Weak,
        steal::{self, HolsteredTarget},
        world::{TargetKind, WorldTarget},
    },
};

/// A remote target offered to the grip.
pub(super) struct Offer {
    pub kind: OfferKind,
    /// Where the hook goes, world space.
    pub point: Vec3,
    /// How far it is from the grapple's ray origin, metres.
    pub distance: f32,
}

pub(super) enum OfferKind {
    /// A weapon lying in the world.
    Ground { weapon: NonZeroUsize, weak: Weak },
    /// A gun in a character's hands.
    Held(WorldTarget),
    /// A gun holstered on a character's body.
    Holstered(HolsteredTarget),
    /// A loose object one hand can carry.
    Prop {
        object: NonZeroUsize,
        body: u32,
        mass_kg: f32,
        weak: Weak,
    },
    /// A vehicle's closed door, pulled at its handle, world space.
    Door {
        door: DoorTarget,
        handle: Vec3,
        weak: Weak,
    },
}

impl OfferKind {
    /// What it is to the grip, for its marker's colour.
    pub(super) const fn remote(&self) -> RemoteKind {
        match self {
            OfferKind::Ground { .. } | OfferKind::Held(_) | OfferKind::Holstered(_) => {
                RemoteKind::Weapon
            }
            OfferKind::Prop { .. } => RemoteKind::Object,
            OfferKind::Door { .. } => RemoteKind::Door,
        }
    }

    pub(super) const fn label(&self) -> &'static str {
        match self {
            OfferKind::Ground { .. } => "WEAPON ON THE GROUND",
            OfferKind::Held(_) => "WEAPON IN HANDS",
            OfferKind::Holstered(_) => "HOLSTERED WEAPON",
            OfferKind::Prop { .. } => "OBJECT",
            OfferKind::Door { .. } => "CAR DOOR",
        }
    }
}

/// Why the grapple hand cannot snatch now, if it cannot.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn offerable(input: &OfferInput) -> Result<(), &'static str> {
    if !matches!(input.detection.mode, Mode::OnFoot(_)) || input.detection.traversal.is_some() {
        return Err("NOT ON FOOT");
    }
    let i = input.hand.index();
    if !physical_hands::free_hands()[i] || physical_hands::held_props()[i].is_some() {
        return Err("HAND NOT EMPTY");
    }
    if input.button_held {
        return Err("GRAPPLE BUTTON HELD");
    }
    let hook = unsafe { engine::local_hook() }.ok_or("NO HOOK")?;
    match engine::busy(hook) {
        Some(Refusal::LineOut) => Err("LINE OUT"),
        Some(_) => Err("HOOK BUSY"),
        None => Ok(()),
    }
}

/// The target the grapple's aim picks along `ray`: the weapon, object, or car door under it, or a
/// gun in a character's hands or holster near its line, within the range, in sight.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn choose(ray: &Ray, config: &ControllersConfig) -> Option<Offer> {
    #[derive(Copy, Clone)]
    enum Kind {
        Aimed,
        Held(usize),
        Holstered(usize),
    }
    let snatch = &config.weapons.snatch;
    let hold = &config.weapons.hold;
    let takes_held = hold.steal && hold.direct_pickup && hold.either_hand;
    let aimed = grapple_weapons::aimed().and_then(|(aimed, alive)| {
        unsafe { aimed_offer(&aimed, config) }.map(|k| (aimed, k, alive))
    });
    // A rack's weapons are the grab's alone.
    let held: Vec<WorldTarget> = if takes_held {
        steal::targets(|p| p)
            .into_iter()
            .filter(|t| matches!(t.kind, TargetKind::Held { .. }))
            .collect()
    } else {
        Vec::new()
    };
    let holstered = if takes_held && snatch.holstered {
        steal::holstered_targets()
    } else {
        Vec::new()
    };
    let candidates = aimed
        .iter()
        .map(|(aimed, kind, _)| Candidate {
            kind: Kind::Aimed,
            point: match kind {
                OfferKind::Door { handle, .. } => *handle,
                _ => aimed.point,
            },
        })
        .chain(held.iter().enumerate().map(|(i, target)| Candidate {
            kind: Kind::Held(i),
            point: target.points[0],
        }))
        .chain(holstered.iter().enumerate().map(|(i, target)| Candidate {
            kind: Kind::Holstered(i),
            point: target.point,
        }));
    let chosen = rules::choose(
        ray,
        candidates,
        snatch.cone_deg.to_radians(),
        snatch.range_m,
    )?;
    let point = chosen.candidate.point;
    let kind = match chosen.candidate.kind {
        Kind::Aimed => aimed.map(|(_, kind, _alive)| kind)?,
        Kind::Held(i) => {
            let target = held[i];
            let TargetKind::Held { character } = target.kind else {
                return None;
            };
            unsafe { in_sight(ray, point, character) }.then_some(OfferKind::Held(target))?
        }
        Kind::Holstered(i) => {
            let target = holstered[i];
            unsafe { in_sight(ray, point, target.character) }
                .then_some(OfferKind::Holstered(target))?
        }
    };
    Some(Offer {
        kind,
        point,
        distance: chosen.distance,
    })
}

/// How far short of a gun on a character its line of sight may be blocked, metres: the line ends
/// inside the character.
const SIGHT_TOLERANCE_M: f32 = 0.3;

/// What the object under the aim is to the snatch: a weapon lying in the world, a loose object a
/// hand can carry, or a vehicle's closed door near the aim's point.
///
/// # Safety
///
/// The main thread, outside the physics step; the object must be held alive.
unsafe fn aimed_offer(
    aimed: &grapple_weapons::Aimed,
    config: &ControllersConfig,
) -> Option<OfferKind> {
    let snatch = &config.weapons.snatch;
    // SAFETY: the caller's strong reference keeps the object, and its weak reference, alive.
    let weak = || unsafe { weak_of(aimed.object) };
    if aimed.weapon {
        return Some(OfferKind::Ground {
            weapon: aimed.object,
            weak: weak(),
        });
    }
    let hands = &config.physical_hands;
    if snatch.props
        && let Some(mass_kg) =
            unsafe { remote::loose_prop(aimed.object, aimed.body, &hands.pickup) }
    {
        return Some(OfferKind::Prop {
            object: aimed.object,
            body: aimed.body,
            mass_kg,
            weak: weak(),
        });
    }
    if snatch.doors && hands.doors.enabled {
        let (door, handle) = unsafe {
            remote::closed_door(
                aimed.object,
                aimed.body,
                aimed.point,
                aimed.normal,
                snatch.door_reach_m,
                &hands.doors,
            )
        }?;
        if unsafe { remote::door_moving(&door, snatch.door_max_vehicle_speed_mps) } {
            return None;
        }
        // SAFETY: the door's vehicle is the object under the aim, or the vehicle of a part of it,
        // live for this tick.
        let weak = unsafe { weak_of(door.vehicle) };
        return Some(OfferKind::Door { door, handle, weak });
    }
    None
}

/// A weak reference to the physics game object at `object`, from its own.
///
/// # Safety
///
/// `object` must be a live physics game object.
pub(super) unsafe fn weak_of(object: NonZeroUsize) -> Weak {
    let game_object =
        (object.get() + PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize) as *const GameObject;
    unsafe { Weak::of(&(*game_object).m_WeakThis) }
}

/// Whether the gun at `point` on `character` is in sight from `ray`'s origin: a ghost hook flies
/// through walls, so the snatch checks the sight line itself.
///
/// # Safety
///
/// The main thread, outside the physics step.
unsafe fn in_sight(ray: &Ray, point: Vec3, character: usize) -> bool {
    let player = unsafe { Character::GetLocalPlayerCharacter() } as usize;
    let options = CastOptions {
        ignore: [player, character],
        see_through: true,
        material: false,
    };
    unsafe { raycast::line_of_sight(ray.origin, point, SIGHT_TOLERANCE_M, &options) }
}
