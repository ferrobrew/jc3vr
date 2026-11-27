//! Taking a gun out of a living character's hands or off its body. The design is in
//! `docs/mod/input/physical-weapons.md`, the engine side in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`.
//!
//! A gun in a character's hands has no body in the physics world (only a dropped weapon's bodies
//! are there), so the hands' reach cannot find it. Instead the inventory's end-of-pass detour,
//! which runs for every character, notes each character near the player with a gun in its hands
//! or holstered on its body ([`observe`]): where the grips are of the gun it wields and of the guns holstered on its body,
//! and a weak reference to the character, which tells the input tick whether it still lives without
//! keeping it alive. The weapon handling places grab volumes along those guns for a free hand
//! ([`crate::controllers::weapons::hold::world`]).
//!
//! A grab of one takes the gun through the hands' take from a source
//! ([`crate::controllers::weapons::hold::source`]), which frees it here ([`release`]), with the
//! character locked alive for the whole release:
//!
//! 1. the character's inventory still holds the gun in the slot noted, and the gun is still in its
//!    hands (or still holstered), and the player's animation allows a pickup;
//! 2. the gun is dropped at once while the character's inventory still holds it
//!    ([`WeaponBase::OnDrop`], the call a swimming character's pickup makes on a slot's occupant),
//!    which unequips it from the character;
//! 3. the character's inventory lets the slot go ([`Inventory::DropWeapon`]), which defers no second
//!    drop since the gun is already dropped: for a gun in its hands with its selection on, the call
//!    its death makes, so it selects another weapon, and for a holstered gun with it off, so it
//!    keeps what it wields;
//! 4. the character staggers ([`Character::HitReaction`], an impulse between its stumble and its
//!    knockdown thresholds, toward the player);
//! 5. the gun goes into the grabbing hand as a weapon off the ground does: through the loadout
//!    table, so the other hand keeps what it holds, sliding there from where it was on the
//!    character, or the grapple's snatch pulls it in on the line.
//!
//! Whether the hand can take it is checked before the character lets go, so a refused take leaves
//! the gun in the character's hands.
//!
//! The detour runs on the simulation's worker threads: it reads the published player position from
//! atomics before it touches anything, and takes the registry's lock only for characters within
//! reach.

use std::{
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Instant,
};

use glam::Vec3;
use jc3gi::{
    animation::symbol_table::RagdollRBGroupId,
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    damage::DamageFlags,
    equipment::weapon::{WeaponBase, WeaponState},
    types::math::Vector3,
};
use jc3vr_body::controllers::{
    hands::ik,
    held::pickup::Refusal,
    physics::{characters::character_position, local_player::LocalPlayer, refs::Weak},
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::Pose;
use parking_lot::Mutex;

use crate::controllers::{
    loadout::TakeSource,
    weapons::{
        engine::{grip_points, slot_of, takeable_gun_slot},
        hold::{
            source::{Freed, SourceRefusal},
            tuning::HOLD,
            world,
            world::{TargetKind, WorldTarget},
        },
        inventory::slots,
    },
};

/// Note characters' guns out to `range_m` from the player rather than within reach: the grapple's
/// snatch takes them from range (`jc3vr_grapple::controllers::snatch`). Zero keeps the notice to
/// reach.
/// Called every input tick.
#[inline]
pub fn widen_notice(range_m: f32) {
    let range = if range_m.is_finite() { range_m } else { 0.0 };
    NOTICE_RANGE.store(range.to_bits(), Ordering::Relaxed);
}

/// Publish where the local player is, for the detour's first test, or `None` to stop noting
/// characters. Called every input tick.
pub(crate) fn publish_probe(position: Option<Vec3>) {
    match position.filter(|p| p.is_finite()) {
        Some(p) => {
            PROBE[0].store(p.x.to_bits(), Ordering::Relaxed);
            PROBE[1].store(p.y.to_bits(), Ordering::Relaxed);
            PROBE[2].store(p.z.to_bits(), Ordering::Relaxed);
            ARMED.store(true, Ordering::Relaxed);
        }
        None => {
            ARMED.store(false, Ordering::Relaxed);
            SEEN.lock().clear();
        }
    }
}

/// Note `inventory`'s character when it is a character other than the local player, near the
/// player, with a gun in its hands or holstered on its body. From the end of the character's
/// pose-finalization pass.
///
/// # Safety
///
/// `inventory` must be null or live, on the thread running its character's pass.
pub(crate) unsafe fn observe(inventory: *mut Inventory) {
    if !ARMED.load(Ordering::Relaxed) {
        return;
    }
    let Some(inventory) = (unsafe { inventory.as_ref() }) else {
        return;
    };
    let character = inventory.m_Character;
    // Without the local player's address, any character could be the player.
    let local = ik::local_character();
    if character.is_null() || local == 0 || character as usize == local {
        return;
    }
    // SAFETY: a live inventory's character is live for its pass.
    let character = unsafe { &*character };
    let probe = Vec3::new(
        f32::from_bits(PROBE[0].load(Ordering::Relaxed)),
        f32::from_bits(PROBE[1].load(Ordering::Relaxed)),
        f32::from_bits(PROBE[2].load(Ordering::Relaxed)),
    );
    let at = character_position(character);
    let distance_sq = at.distance_squared(probe);
    let range = f32::from_bits(NOTICE_RANGE.load(Ordering::Relaxed))
        .max(HOLD.get().hold.steal.notice_range_m);
    if distance_sq.is_nan() || distance_sq > range * range {
        return;
    }
    let wielded = unsafe { wielded(character) }.and_then(|(weapon, slot)| {
        // SAFETY: the character's wielded weapon is live for its pass.
        let (points, gun) = unsafe { grip_points(&*weapon) }?;
        Some(Wielded {
            weapon: weapon as usize,
            slot,
            points,
            gun,
        })
    });
    let holstered = unsafe { holstered(character) };
    if wielded.is_none() && holstered.iter().all(Option::is_none) {
        return;
    }
    let address = character as *const Character as usize;
    let now = Instant::now();
    let mut seen = SEEN.lock();
    if let Some(entry) = seen.iter_mut().find(|s| s.character == address) {
        entry.wielded = wielded;
        entry.holstered = holstered;
        entry.at = now;
        return;
    }
    if seen.len() >= MAX_SEEN {
        return;
    }
    seen.push(Seen {
        character: address,
        // SAFETY: the character's own weak reference to itself is live while it is.
        weak: unsafe { Weak::of(&character.m_WeakThis) },
        wielded,
        holstered,
        at: now,
    });
}

/// The guns on characters noted recently, in their hands ([`TargetKind::Held`]) and holstered on
/// their bodies ([`TargetKind::Holstered`]), with their points carried into the cockpit frame by
/// `to_cockpit`. Forgets the characters not noted for a while.
pub fn targets(to_cockpit: impl Fn(Vec3) -> Vec3) -> Vec<WorldTarget> {
    let to_cockpit = &to_cockpit;
    let mut seen = SEEN.lock();
    let seen_for = HOLD.get().hold.steal.seen_for_ms.duration();
    seen.retain(|s| s.at.elapsed() <= seen_for);
    seen.iter()
        .flat_map(|s| {
            let character = s.character;
            let held = s.wielded.map(|w| WorldTarget {
                kind: TargetKind::Held { character },
                weapon: w.weapon,
                slot: w.slot,
                points: w.points.map(to_cockpit),
                gun: w.gun,
            });
            let holstered = s.holstered.iter().flatten().map(move |h| WorldTarget {
                kind: TargetKind::Holstered { character },
                weapon: h.weapon,
                slot: h.slot,
                points: h.points.map(to_cockpit),
                gun: h.gun,
            });
            held.into_iter().chain(holstered)
        })
        .collect()
}

/// A gun holstered on a character's body, as noted.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HolsteredTarget {
    pub character: usize,
    pub weapon: usize,
    pub slot: WeaponSlot,
    /// The gun's origin, world space.
    pub point: Vec3,
}

/// The guns holstered on characters noted recently, out of their hands. Forgets the characters not
/// noted for a while.
pub fn holstered_targets() -> Vec<HolsteredTarget> {
    let mut seen = SEEN.lock();
    let seen_for = HOLD.get().hold.steal.seen_for_ms.duration();
    seen.retain(|s| s.at.elapsed() <= seen_for);
    seen.iter()
        .flat_map(|s| {
            s.holstered.iter().flatten().map(|h| HolsteredTarget {
                character: s.character,
                weapon: h.weapon,
                slot: h.slot,
                point: h.gun.position,
            })
        })
        .collect()
}

/// A gun on a character: in its hands, or holstered on its body.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct CharacterGun {
    pub character: usize,
    pub weapon: usize,
    pub slot: WeaponSlot,
    /// Whether the gun is holstered on the character's body rather than in its hands.
    pub holstered: bool,
}

impl CharacterGun {
    /// The gun that `target` stands for: in the hands for a [`TargetKind::Held`] target, holstered
    /// for a [`TargetKind::Holstered`] one, and `None` for a target not on a character.
    pub fn of_target(target: &WorldTarget) -> Option<CharacterGun> {
        let (character, holstered) = match target.kind {
            TargetKind::Held { character } => (character, false),
            TargetKind::Holstered { character } => (character, true),
            TargetKind::Rack { .. } | TargetKind::RackMember { .. } => return None,
        };
        Some(CharacterGun {
            character,
            weapon: target.weapon,
            slot: target.slot,
            holstered,
        })
    }

    /// The holstered gun the snatch's `target` stands for.
    pub fn holstered(target: &HolsteredTarget) -> CharacterGun {
        CharacterGun {
            character: target.character,
            weapon: target.weapon,
            slot: target.slot,
            holstered: true,
        }
    }
}

/// Make `gun`'s character let go of it and stagger, leaving the gun lying in the world. A gun in
/// its hands goes through its inventory's selection, as its death's drop does; a holstered gun is
/// dropped without a selection change, so the character keeps what it wields. `admit` runs once
/// the gun is known still to be the character's, and a refusal there leaves the gun where it is.
pub(crate) fn release(
    game: &GameTick<'_>,
    gun: &CharacterGun,
    admit: impl FnOnce(&Character, &WeaponBase) -> Result<(), SourceRefusal>,
) -> Result<Freed, SourceRefusal> {
    // Hold the character alive through the release.
    let alive = {
        let seen = SEEN.lock();
        seen.iter()
            .find(|s| s.character == gun.character)
            .and_then(|s| s.weak.lock())
    };
    let Some(_alive) = alive else {
        return Err(SourceRefusal::Gone);
    };
    if gun.character == ik::local_character() {
        return Err(SourceRefusal::Gone);
    }
    // SAFETY: the strong reference keeps the character alive.
    let victim = unsafe { &mut *(gun.character as *mut Character) };
    let still = if gun.holstered {
        unsafe { holstered(victim) }
            .iter()
            .flatten()
            .any(|h| h.weapon == gun.weapon && h.slot == gun.slot)
    } else {
        unsafe { wielded(victim) }
            .is_some_and(|(weapon, slot)| weapon as usize == gun.weapon && slot == gun.slot)
    };
    if !still {
        return Err(SourceRefusal::Gone);
    }
    // SAFETY: the character's inventory holds the weapon, and the character is held alive.
    let weapon = unsafe { &mut *(gun.weapon as *mut WeaponBase) };
    let mut local_player =
        LocalPlayer::get(game).ok_or(SourceRefusal::Take(Refusal::NoCharacter))?;
    let player = local_player.character_mut();
    admit(player, weapon)?;
    // SAFETY: the weapon's own weak reference to itself, live while it is.
    let alive = unsafe { Weak::of(&weapon.m_WeakThis) }
        .lock()
        .ok_or(SourceRefusal::Gone)?;
    let player_at = character_position(player);
    unsafe { weapon.OnDrop() };
    unsafe { victim.m_Inventory.DropWeapon(gun.slot, !gun.holstered) };
    unsafe { stagger(victim, player_at) };
    forget(gun);
    Ok(Freed {
        weapon,
        alive,
        source: TakeSource::Character,
    })
}

/// The most characters noted at once.
const MAX_SEEN: usize = 16;

/// A character noted with a gun in its hands or on its body.
struct Seen {
    character: usize,
    weak: Weak,
    wielded: Option<Wielded>,
    holstered: [Option<Holstered>; HOLSTER_SLOTS.len()],
    at: Instant,
}

/// The gun a noted character wields.
#[derive(Copy, Clone)]
struct Wielded {
    weapon: usize,
    slot: WeaponSlot,
    /// The gun's segment in the world: the holding hand's grip, and the other hand's grip or the
    /// gun's origin.
    points: [Vec3; 2],
    gun: Pose,
}

/// A gun a noted character carries holstered on its body, seen on it.
#[derive(Copy, Clone)]
struct Holstered {
    weapon: usize,
    slot: WeaponSlot,
    /// The gun's segment in the world, between its two grips, or a grip and the gun's origin.
    points: [Vec3; 2],
    gun: Pose,
}

/// The slots of the guns a character holsters where they are seen.
const HOLSTER_SLOTS: [WeaponSlot; 3] = [
    WeaponSlot::E_SLOT_DUAL_WIELD,
    WeaponSlot::E_SLOT_TWO_HANDED,
    WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL,
];

static SEEN: Mutex<Vec<Seen>> = Mutex::new(Vec::new());
static PROBE: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
static ARMED: AtomicBool = AtomicBool::new(false);
/// How far from the player the grapple's snatch notes characters' guns, metres, as `f32` bits:
/// never nearer than the steal's
/// [`notice_range_m`](crate::controllers::weapons::hold::tuning::StealTuning::notice_range_m).
static NOTICE_RANGE: AtomicU32 = AtomicU32::new(0);

/// The gun `character` wields and its slot.
unsafe fn wielded(character: &Character) -> Option<(*mut WeaponBase, WeaponSlot)> {
    let held = unsafe { character.GetWieldingWeapon(false).as_ref() }?;
    let pointer = held.px;
    let weapon = unsafe { pointer.as_ref() }?;
    if !matches!(
        weapon.m_State,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    ) {
        return None;
    }
    let slot = unsafe { slot_of(weapon) }?;
    takeable_gun_slot(slot).then_some((pointer, slot))
}

/// The guns `character` carries holstered and visible on its body, by [`HOLSTER_SLOTS`], each
/// with its first gun's grips and pose.
unsafe fn holstered(character: &Character) -> [Option<Holstered>; HOLSTER_SLOTS.len()] {
    HOLSTER_SLOTS.map(|slot| {
        // SAFETY: the slot's strong reference keeps its weapon alive; forwarded from the caller.
        let weapon = unsafe { slots::weapon(&character.m_Inventory, slot)?.as_ref() };
        // Read as the raw value, so that a state the enum does not name is not misread.
        let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
        if state != WeaponState::E_WS_HOLSTERED as i32 {
            return None;
        }
        let (points, gun) = unsafe { grip_points(weapon) }?;
        Some(Holstered {
            weapon: weapon as *const WeaponBase as usize,
            slot,
            points,
            gun,
        })
    })
}

/// Forget `gun` once its character let go of it, so that no target offers it until the character
/// is noted again: a character whose gun left its hands is forgotten whole, and only the holster is
/// forgotten of one that keeps what it wields.
fn forget(gun: &CharacterGun) {
    let mut seen = SEEN.lock();
    if !gun.holstered {
        seen.retain(|s| s.character != gun.character);
        return;
    }
    for s in seen.iter_mut().filter(|s| s.character == gun.character) {
        for holstered in &mut s.holstered {
            if holstered.is_some_and(|h| h.weapon == gun.weapon) {
                *holstered = None;
            }
        }
    }
}

/// Stagger `victim` toward `toward`, as a melee blow just over its stumble threshold would.
unsafe fn stagger(victim: &mut Character, toward: Vec3) {
    let melee = DamageFlags::DMG_TYPE_MELEE.bits().trailing_zeros() as usize;
    let Some(impulse) = world::stagger_impulse(
        victim
            .m_HitReaction_DamageImpulseScale
            .get(melee)
            .copied()
            .unwrap_or(0.0),
        victim.m_HitReaction_ImpulseStumbleThreshold,
        victim.m_HitReaction_ImpulseRagdollThreshold,
    ) else {
        return;
    };
    let at = character_position(victim);
    let level = Vec3::new(toward.x - at.x, 0.0, toward.z - at.z);
    let Some(direction) = level.try_normalize() else {
        return;
    };
    let mut zone = RagdollRBGroupId {
        m_id: RagdollRBGroupId::NONE,
    };
    // The reaction destroys the zone it is given, so the copy takes its own use first.
    unsafe {
        zone.init_ptr();
        victim.HitReaction(
            &Vector3 {
                data: at.to_array(),
            },
            &Vector3 {
                data: direction.to_array(),
            },
            impulse,
            false,
            &mut zone.m_id,
            DamageFlags::DMG_TYPE_MELEE.bits(),
            false,
        );
    }
}
