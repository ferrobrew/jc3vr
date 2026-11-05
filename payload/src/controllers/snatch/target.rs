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
    physical_hands::{
        self,
        collider_view::model::Aabb,
        doors::DoorTarget,
        remote::{self, DroppedWeapon, RagdollTarget},
    },
    snatch::{
        OfferInput, Refusal, SNATCH, engine,
        rules::{self, Candidate, PropHandling},
    },
    weapons::{
        grenade,
        hold::{
            rack::{self, remote::RemoteRack},
            refs::Weak,
            steal::{self, HolsteredTarget},
            world::{TargetKind, WorldTarget},
        },
    },
};

/// Revalidate a recent offer without selecting a different object.
///
/// # Safety
///
/// The game thread, before the world update and the physics step.
pub(super) unsafe fn refresh(offer: &mut Offer, ray: &Ray, config: &ControllersConfig) -> bool {
    let point = match &mut offer.kind {
        OfferKind::WorldGrenade { weapon, body, weak } => {
            let Some(_alive) = weak.lock() else {
                return false;
            };
            if !unsafe { grapple_weapons::is_dropped_weapon(*weapon) }
                || unsafe { engine::hand_grenade(*weapon) } != Some(true)
            {
                return false;
            }
            let Ok((point, _)) = (unsafe { physical_hands::body_pose(*body, *weapon) }) else {
                return false;
            };
            if !unsafe { in_sight(ray, point, weapon.get()) } {
                return false;
            }
            point
        }
        OfferKind::Ground {
            weapon,
            body,
            local_point,
            weak,
        } => {
            let Some(_alive) = weak.lock() else {
                return false;
            };
            if !unsafe { grapple_weapons::is_dropped_weapon(*weapon) } {
                return false;
            }
            let Ok((centre, orientation)) = (unsafe { physical_hands::body_pose(*body, *weapon) })
            else {
                return false;
            };
            let point = centre + orientation * *local_point;
            let player = unsafe { Character::GetLocalPlayerCharacter() } as usize;
            let options = CastOptions {
                ignore: [player, weapon.get()],
                see_through: true,
                material: false,
            };
            if !unsafe { raycast::line_of_sight(ray.origin, point, SIGHT_TOLERANCE_M, &options) } {
                return false;
            }
            point
        }
        OfferKind::Held(target) => {
            let Some(current) = steal::targets(|p| p)
                .into_iter()
                .find(|current| current.weapon == target.weapon && current.kind == target.kind)
            else {
                return false;
            };
            let TargetKind::Held { character } = current.kind else {
                return false;
            };
            if !unsafe { in_sight(ray, current.points[0], character) } {
                return false;
            }
            *target = current;
            current.points[0]
        }
        OfferKind::Holstered(target) => {
            let Some(current) = steal::holstered_targets().into_iter().find(|current| {
                current.weapon == target.weapon && current.character == target.character
            }) else {
                return false;
            };
            if !unsafe { in_sight(ray, current.point, current.character) } {
                return false;
            }
            *target = current;
            current.point
        }
        OfferKind::Rack { weapon, weak } => {
            let Some(point) = (unsafe { rack::remote::current(*weapon, weak) }) else {
                return false;
            };
            if !unsafe { in_sight(ray, point, weapon.get()) } {
                return false;
            }
            point
        }
        OfferKind::Prop {
            object,
            body,
            handling,
            weak,
            ..
        } => {
            let Some(_alive) = weak.lock() else {
                return false;
            };
            if unsafe { prop_handling(*object, *body, config) }.map(|(_, h)| h) != Some(*handling) {
                return false;
            }
            let Ok((point, _)) = (unsafe { physical_hands::body_pose(*body, *object) }) else {
                return false;
            };
            if !unsafe { in_sight(ray, point, object.get()) } {
                return false;
            }
            point
        }
        OfferKind::Ragdoll {
            character,
            owner,
            body,
            mass_kg,
            weak,
        } => {
            let Some(_alive) = weak.lock() else {
                return false;
            };
            let Some(current) = (unsafe { ragdoll_offer(*character, *body, offer.point, config) })
            else {
                return false;
            };
            if current.body != *body || current.owner != *owner {
                return false;
            }
            *mass_kg = current.mass_kg;
            if !unsafe { in_sight(ray, current.centre, character.get()) } {
                return false;
            }
            current.centre
        }
        OfferKind::Grenade { object, body, weak } => {
            let Some(_alive) = weak.lock() else {
                return false;
            };
            if !grenade::inert_in_world(*object, *body) {
                return false;
            }
            let Ok((point, _)) = (unsafe { physical_hands::body_pose(*body, *object) }) else {
                return false;
            };
            if !unsafe { in_sight(ray, point, object.get()) } {
                return false;
            }
            point
        }
        OfferKind::Door { door, handle, weak } => {
            let Some(_alive) = weak.lock() else {
                return false;
            };
            if !config.physical_hands.doors.enabled
                || unsafe { remote::door_moving(door, SNATCH.door_max_vehicle_speed_mps) }
            {
                return false;
            }
            let Some(body) = door.vehicle_body else {
                return false;
            };
            let Some((current, point)) = (unsafe {
                remote::closed_door(
                    door.vehicle,
                    body,
                    *handle,
                    door.outward,
                    SNATCH.door_reach_m,
                    &config.physical_hands.doors,
                )
            }) else {
                return false;
            };
            if current.part_index != door.part_index || current.slot != door.slot {
                return false;
            }
            if !unsafe { in_sight(ray, point, door.vehicle.get()) } {
                return false;
            }
            *door = current;
            *handle = point;
            point
        }
    };
    let ground = matches!(
        offer.kind,
        OfferKind::Ground { .. } | OfferKind::WorldGrenade { .. } | OfferKind::Rack { .. }
    );
    let chosen = rules::choose(
        ray,
        [Candidate { kind: (), point }],
        |_, distance| cone(ground, distance) + RETAIN_CONE_DEG.to_radians(),
        SNATCH.range_m,
    );
    let Some(chosen) = chosen else { return false };
    offer.point = point;
    offer.distance = chosen.distance;
    true
}

/// Extra angular tolerance while retaining a recent aimed identity across a grip press.
const RETAIN_CONE_DEG: f32 = 4.0;

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
    Ground {
        weapon: NonZeroUsize,
        body: u32,
        local_point: Vec3,
        weak: Weak,
    },
    /// A gun in a character's hands.
    Held(WorldTarget),
    /// A gun holstered on a character's body.
    Holstered(HolsteredTarget),
    /// A long gun on a rack, or lying where a locker's placement rule set it down.
    Rack { weapon: NonZeroUsize, weak: Weak },
    /// A loose object: one a hand can carry, or one heavier, up to the swing's ceiling, which the
    /// line swings.
    Prop {
        object: NonZeroUsize,
        body: u32,
        mass_kg: f32,
        handling: PropHandling,
        weak: Weak,
    },
    /// A ragdolling character, fallen, knocked down, or dead, pulled by the body of its ragdoll
    /// the aim hit, `body` of `owner`, which drags the whole ragdoll of `mass_kg`. Always swung,
    /// within the swing's ceiling.
    Ragdoll {
        character: NonZeroUsize,
        owner: NonZeroUsize,
        body: u32,
        mass_kg: f32,
        weak: Weak,
    },
    /// A grenade lying inert in the world, its pin in: the loadout's still, and taken into the
    /// hand through it.
    Grenade {
        object: NonZeroUsize,
        body: u32,
        weak: Weak,
    },
    /// A finite native hand grenade lying in the world, which the game would collect by proximity:
    /// the hand takes it on the catch.
    WorldGrenade {
        weapon: NonZeroUsize,
        body: u32,
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
            OfferKind::Ground { .. }
            | OfferKind::Held(_)
            | OfferKind::Holstered(_)
            | OfferKind::Rack { .. } => RemoteKind::Weapon,
            OfferKind::Prop { .. }
            | OfferKind::Ragdoll { .. }
            | OfferKind::Grenade { .. }
            | OfferKind::WorldGrenade { .. } => RemoteKind::Object,
            OfferKind::Door { .. } => RemoteKind::Door,
        }
    }

    pub(super) const fn label(&self) -> &'static str {
        match self {
            OfferKind::Ground { .. } => "WEAPON ON THE GROUND",
            OfferKind::Held(_) => "WEAPON IN HANDS",
            OfferKind::Holstered(_) => "HOLSTERED WEAPON",
            OfferKind::Rack { .. } => "WEAPON ON A RACK",
            OfferKind::Prop {
                handling: PropHandling::Carry,
                ..
            } => "OBJECT",
            OfferKind::Prop {
                handling: PropHandling::Swing,
                ..
            } => "HEAVY OBJECT",
            OfferKind::Ragdoll { .. } => "BODY",
            OfferKind::Grenade { .. } | OfferKind::WorldGrenade { .. } => "GRENADE",
            OfferKind::Door { .. } => "CAR DOOR",
        }
    }
}

/// Why the grapple hand cannot snatch now, if it cannot.
///
/// # Safety
///
/// The game thread, before the world update.
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
/// weapon or an inert grenade lying in the world or a gun in a character's hands or holster near
/// its line, within the range, in sight.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn choose(ray: &Ray, config: &ControllersConfig) -> Option<Offer> {
    #[derive(Copy, Clone)]
    enum Kind {
        Aimed,
        Floor(usize),
        Loose(usize),
        Held(usize),
        Holstered(usize),
        Rack(usize),
    }
    let hold = &config.weapons.hold;
    let takes_held = hold.either_hand;
    let aimed = grapple_weapons::aimed().and_then(|(aimed, alive)| {
        unsafe { aimed_offer(&aimed, config) }.map(|k| (aimed, k, alive))
    });
    let aimed_weapon = aimed
        .as_ref()
        .and_then(|(aimed, _, _)| aimed.weapon.then_some(aimed.object));
    let floor: Vec<DroppedWeapon> = unsafe { remote::dropped_weapons(|aabb| near_aim(ray, aabb)) }
        .into_iter()
        .filter(|weapon| Some(weapon.object) != aimed_weapon)
        .collect();
    // An inert grenade lying in the world is small, so it is offered within the floor's cone as a
    // weapon lying there is, not only under a direct hit of the aim.
    let aimed_object = aimed.as_ref().map(|(aimed, _, _)| aimed.object);
    let loose: Vec<(NonZeroUsize, u32, Vec3)> = grenade::inert_lying()
        .into_iter()
        .filter(|(object, _, _)| Some(*object) != aimed_object)
        .collect();
    // A weapon its rule set down that lies in the world with its body is the floor's already.
    let mut racks: Vec<RemoteRack> = unsafe { rack::remote::near(|point| near_point(ray, point)) }
        .into_iter()
        .filter(|rack| {
            Some(rack.weapon) != aimed_weapon && !floor.iter().any(|w| w.object == rack.weapon)
        })
        .collect();
    let held: Vec<WorldTarget> = if takes_held {
        steal::targets(|p| p)
            .into_iter()
            .filter(|t| matches!(t.kind, TargetKind::Held { .. }))
            .collect()
    } else {
        Vec::new()
    };
    let holstered = if takes_held {
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
        .chain(floor.iter().enumerate().map(|(i, weapon)| Candidate {
            kind: Kind::Floor(i),
            point: weapon.centre,
        }))
        .chain(
            loose
                .iter()
                .enumerate()
                .map(|(i, (_, _, centre))| Candidate {
                    kind: Kind::Loose(i),
                    point: *centre,
                }),
        )
        .chain(held.iter().enumerate().map(|(i, target)| Candidate {
            kind: Kind::Held(i),
            point: target.points[0],
        }))
        .chain(holstered.iter().enumerate().map(|(i, target)| Candidate {
            kind: Kind::Holstered(i),
            point: target.point,
        }))
        .chain(racks.iter().enumerate().map(|(i, rack)| Candidate {
            kind: Kind::Rack(i),
            point: rack.point,
        }));
    let chosen = rules::choose(
        ray,
        candidates,
        |kind, distance| {
            cone(
                matches!(kind, Kind::Floor(_) | Kind::Loose(_) | Kind::Rack(_)),
                distance,
            )
        },
        SNATCH.range_m,
    )?;
    let point = chosen.candidate.point;
    let kind = match chosen.candidate.kind {
        Kind::Aimed => aimed.map(|(_, kind, _alive)| kind)?,
        Kind::Floor(i) => {
            let weapon = floor[i];
            if !unsafe { in_sight(ray, point, weapon.object.get()) } {
                return None;
            }
            // SAFETY: its body was read as its own on this thread this tick.
            let weak = unsafe { weak_of(weapon.object) };
            match unsafe { engine::hand_grenade(weapon.object) } {
                None => OfferKind::Ground {
                    weapon: weapon.object,
                    body: weapon.body,
                    local_point: Vec3::ZERO,
                    weak,
                },
                Some(true) => OfferKind::WorldGrenade {
                    weapon: weapon.object,
                    body: weapon.body,
                    weak,
                },
                Some(false) => return None,
            }
        }
        Kind::Loose(i) => {
            let (object, body, _) = loose[i];
            if !unsafe { in_sight(ray, point, object.get()) } {
                return None;
            }
            OfferKind::Grenade {
                object,
                body,
                // SAFETY: the grenade's pool holds a strong reference to it this tick.
                weak: unsafe { weak_of(object) },
            }
        }
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
        Kind::Rack(i) => {
            let rack = racks.swap_remove(i);
            if !unsafe { in_sight(ray, point, rack.weapon.get()) } {
                return None;
            }
            OfferKind::Rack {
                weapon: rack.weapon,
                weak: rack.weak,
            }
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

/// The cone, radians, of a candidate `distance` metres off: a weapon lying in the world's widens
/// with its distance ([`rules::floor_cone`]), and every other's is the snatch's cone.
fn cone(on_ground: bool, distance: f32) -> f32 {
    let base = SNATCH.cone_deg.to_radians();
    if on_ground {
        rules::floor_cone(
            distance,
            base,
            SNATCH.floor_cone_widen_deg.to_radians(),
            SNATCH.range_m,
        )
    } else {
        base
    }
}

/// Whether a dynamic body's box could hold a weapon lying in the world that [`choose`] would take
/// from `ray`: within the range and the widest floor cone, allowing for the box's own size.
fn near_aim(ray: &Ray, aabb: &Aabb) -> bool {
    let offset = aabb.centre() - ray.origin;
    let distance = offset.length();
    let half_size = (aabb.max - aabb.min).length() * 0.5;
    if !(distance.is_finite() && half_size.is_finite()) || distance - half_size > SNATCH.range_m {
        return false;
    }
    if distance <= half_size {
        return true;
    }
    let slack = (half_size / distance).asin();
    let widest = (SNATCH.cone_deg + SNATCH.floor_cone_widen_deg).to_radians();
    ray.direction.angle_between(offset / distance) <= widest + slack
}

/// Whether `point` could be a candidate [`choose`] would take from `ray`: within the range and the
/// widest floor cone.
fn near_point(ray: &Ray, point: Vec3) -> bool {
    let offset = point - ray.origin;
    let distance = offset.length();
    if !distance.is_finite() || distance > SNATCH.range_m || distance < 1e-3 {
        return false;
    }
    let widest = (SNATCH.cone_deg + SNATCH.floor_cone_widen_deg).to_radians();
    ray.direction.angle_between(offset / distance) <= widest
}

/// What the object under the aim is to the snatch: a weapon lying in the world, a loose object a
/// hand can carry or the line can swing, or a vehicle's closed door near the aim's point.
///
/// # Safety
///
/// The game thread, before the world update; the object must be held alive.
unsafe fn aimed_offer(
    aimed: &grapple_weapons::Aimed,
    config: &ControllersConfig,
) -> Option<OfferKind> {
    // SAFETY: the caller's strong reference keeps the object, and its weak reference, alive.
    let weak = || unsafe { weak_of(aimed.object) };
    if aimed.weapon {
        if !unsafe { grapple_weapons::is_dropped_weapon(aimed.object) } {
            return None;
        }
        let (centre, orientation) =
            unsafe { physical_hands::body_pose(aimed.body, aimed.object) }.ok()?;
        match unsafe { engine::hand_grenade(aimed.object) } {
            None => {}
            Some(true) => {
                return Some(OfferKind::WorldGrenade {
                    weapon: aimed.object,
                    body: aimed.body,
                    weak: weak(),
                });
            }
            Some(false) => return None,
        }
        return Some(OfferKind::Ground {
            weapon: aimed.object,
            body: aimed.body,
            local_point: orientation.inverse() * (aimed.point - centre),
            weak: weak(),
        });
    }
    if grenade::inert_in_world(aimed.object, aimed.body) {
        return Some(OfferKind::Grenade {
            object: aimed.object,
            body: aimed.body,
            weak: weak(),
        });
    }
    if let Some(ragdoll) = unsafe { ragdoll_offer(aimed.object, aimed.body, aimed.point, config) } {
        return Some(OfferKind::Ragdoll {
            character: aimed.object,
            owner: ragdoll.owner,
            body: ragdoll.body,
            mass_kg: ragdoll.mass_kg,
            weak: weak(),
        });
    }
    let hands = &config.physical_hands;
    if let Some((mass_kg, handling)) = unsafe { prop_handling(aimed.object, aimed.body, config) } {
        return Some(OfferKind::Prop {
            object: aimed.object,
            body: aimed.body,
            mass_kg,
            handling,
            weak: weak(),
        });
    }
    if hands.doors.enabled {
        let (door, handle) = unsafe {
            remote::closed_door(
                aimed.object,
                aimed.body,
                aimed.point,
                aimed.normal,
                SNATCH.door_reach_m,
                &hands.doors,
            )
        }?;
        if unsafe { remote::door_moving(&door, SNATCH.door_max_vehicle_speed_mps) } {
            return None;
        }
        // SAFETY: the door's vehicle is the object under the aim, or the vehicle of a part of it,
        // live for this tick.
        let weak = unsafe { weak_of(door.vehicle) };
        return Some(OfferKind::Door { door, handle, weak });
    }
    None
}

/// The mass of the loose object under the grapple's aim, whatever the snatch would do with it, for
/// the readout.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn aimed_mass() -> Option<f32> {
    let (aimed, _alive) = grapple_weapons::aimed()?;
    if aimed.weapon {
        return None;
    }
    unsafe { remote::loose_mass(aimed.object, aimed.body) }.or_else(|| {
        unsafe { remote::ragdoll_target(aimed.object, aimed.body, aimed.point) }.map(|r| r.mass_kg)
    })
}

/// The ragdoll of the character `object` the aim hit on its body `body` at `point`, when the
/// snatch offers it: the character ragdolls, the swing is on, and the whole ragdoll is within the
/// swing's ceiling.
///
/// # Safety
///
/// The game thread, before the world update; the object must be held alive.
unsafe fn ragdoll_offer(
    object: NonZeroUsize,
    body: u32,
    point: Vec3,
    config: &ControllersConfig,
) -> Option<RagdollTarget> {
    let snatch = &config.weapons.snatch;
    if !snatch.sling {
        return None;
    }
    let ragdoll = unsafe { remote::ragdoll_target(object, body, point) }?;
    (ragdoll.mass_kg <= snatch.swing.ceiling_kg).then_some(ragdoll)
}

/// The mass of `object`'s body `body` and what the snatch does with it, when it is a loose object
/// the snatch offers: one a hand can carry, or, while the swing is on, one up to its ceiling.
///
/// # Safety
///
/// The game thread, before the world update; the object must be held alive.
unsafe fn prop_handling(
    object: NonZeroUsize,
    body: u32,
    config: &ControllersConfig,
) -> Option<(f32, PropHandling)> {
    let snatch = &config.weapons.snatch;
    let ceiling = if snatch.sling {
        snatch.swing.ceiling_kg
    } else {
        0.0
    };
    let mass_kg = unsafe { remote::loose_mass(object, body) }?;
    rules::prop_handling(mass_kg, remote::carry_max_kg(), ceiling).map(|h| (mass_kg, h))
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
/// The game thread, before the world update.
unsafe fn in_sight(ray: &Ray, point: Vec3, character: usize) -> bool {
    let player = unsafe { Character::GetLocalPlayerCharacter() } as usize;
    let options = CastOptions {
        ignore: [player, character],
        see_through: true,
        material: false,
    };
    unsafe { raycast::line_of_sight(ray.origin, point, SIGHT_TOLERANCE_M, &options) }
}
