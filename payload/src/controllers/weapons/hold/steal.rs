//! Taking a gun out of a living character's hands. The design is in
//! `docs/mod/input/physical-weapons.md`, the engine side in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`.
//!
//! A gun in a character's hands has no body in the physics world (only a dropped weapon's bodies
//! are there), so the hands' reach cannot find it. Instead the inventory's end-of-pass detour,
//! which runs for every character, notes each character near the player that wields a gun
//! ([`observe`]): where the gun's grips are, and a weak reference to the character, which tells the
//! input tick whether it still lives without keeping it alive. The weapon handling places grab
//! volumes along those guns for a free hand ([`crate::controllers::weapons::hold::world`]).
//!
//! A grab of one takes the gun ([`take`]), with the character locked alive for the whole take:
//!
//! 1. the character's inventory still holds the gun in the slot noted, and the gun is still in its
//!    hands, and the player's animation allows a pickup;
//! 2. the gun is dropped at once while the character's inventory still holds it
//!    ([`WeaponBase::OnDrop`], the call a swimming character's pickup makes on a slot's occupant),
//!    which unequips it from the character;
//! 3. the character's inventory lets the slot go and selects another weapon for it
//!    ([`Inventory::DropWeapon`] with its selection on, the call its death makes), which defers no
//!    second drop since the gun is already dropped;
//! 4. the character staggers ([`Character::HitReaction`], an impulse between its stumble and its
//!    knockdown thresholds, toward the player);
//! 5. the gun goes straight into the grabbing hand as a weapon off the ground does
//!    ([`crate::controllers::weapons::hold::pickup::take`]): through the loadout table, so the other
//!    hand keeps what it holds, sliding there from where it was in the character's hands.
//!
//! Whether the hand can take it is checked before the character lets go, so a refused take leaves
//! the gun in the character's hands.
//!
//! The detour runs on the simulation's worker threads: it reads the published player position from
//! atomics before it touches anything, and takes the registry's lock only for characters within
//! reach.

use std::{
    fmt,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    time::{Duration, Instant},
};

use glam::{Mat4, Vec3};
use jc3gi::{
    animation::symbol_table::RagdollRBGroupId,
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    damage::DamageFlags,
    equipment::weapon::{WeaponBase, WeaponGripHand, WeaponState},
    types::math::{Matrix4, Vector3},
};
use parking_lot::Mutex;

use crate::controllers::{
    aim::gun,
    frame::{Hand, Pose},
    hands::ik,
    loadout::slot_label,
    weapons::{
        engine::{self, slot_from_raw},
        hold::{
            pickup::{self, Refusal, TakeOptions},
            refs::{Strong, Weak},
            world::{self, TargetKind, WorldTarget},
        },
    },
};

/// Why a take from a character's hands did not happen.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StealRefusal {
    /// The character is gone, or no longer holds the gun.
    Gone,
    /// The pickup that puts the gun in the hand refused it.
    Pickup(Refusal),
}

impl fmt::Display for StealRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StealRefusal::Gone => write!(
                f,
                "take from the hands: the character no longer holds the gun"
            ),
            StealRefusal::Pickup(refusal) => write!(f, "take from the hands: {refusal}"),
        }
    }
}

impl std::error::Error for StealRefusal {}

/// Note characters' guns out to `range_m` from the player rather than within reach: the grapple's
/// snatch takes them from range ([`crate::controllers::snatch`]). Zero keeps the notice to reach.
/// Called every input tick.
pub(crate) fn widen_notice(range_m: f32) {
    let range = if range_m.is_finite() { range_m } else { 0.0 };
    NOTICE_RANGE.store(range.max(NOTICE_RANGE_M).to_bits(), Ordering::Relaxed);
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
/// player, with a gun in its hands. From the end of the character's pose-finalization pass.
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
    let at = Mat4::from(character.m_WorldMatrixT1).w_axis.truncate();
    let distance_sq = at.distance_squared(probe);
    let range = f32::from_bits(NOTICE_RANGE.load(Ordering::Relaxed));
    if distance_sq.is_nan() || distance_sq > range * range {
        return;
    }
    let wielded = unsafe { wielded(character) }.and_then(|(weapon, slot)| {
        // SAFETY: the character's wielded weapon is live for its pass.
        let (points, gun) = unsafe { gun_points(&*weapon) }?;
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

/// The guns in characters' hands noted recently, with their points carried into the cockpit frame
/// by `to_cockpit`. Forgets the characters not noted for a while.
pub(crate) fn targets(to_cockpit: impl Fn(Vec3) -> Vec3) -> Vec<WorldTarget> {
    let mut seen = SEEN.lock();
    seen.retain(|s| s.at.elapsed() <= SEEN_FOR);
    seen.iter()
        .filter_map(|s| {
            let w = s.wielded?;
            Some(WorldTarget {
                kind: TargetKind::Held {
                    character: s.character,
                },
                weapon: w.weapon,
                slot: w.slot,
                points: w.points.map(&to_cockpit),
                gun: w.gun,
            })
        })
        .collect()
}

/// A gun holstered on a character's body, as noted.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct HolsteredTarget {
    pub character: usize,
    pub weapon: usize,
    pub slot: WeaponSlot,
    /// The gun's origin, world space.
    pub point: Vec3,
}

/// The guns holstered on characters noted recently, out of their hands. Forgets the characters not
/// noted for a while.
pub(crate) fn holstered_targets() -> Vec<HolsteredTarget> {
    let mut seen = SEEN.lock();
    seen.retain(|s| s.at.elapsed() <= SEEN_FOR);
    seen.iter()
        .flat_map(|s| {
            s.holstered.iter().flatten().map(|h| HolsteredTarget {
                character: s.character,
                weapon: h.weapon,
                slot: h.slot,
                point: h.point,
            })
        })
        .collect()
}

/// Take `target`'s gun out of its character's hands into `hand`.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn take(
    hand: Hand,
    target: &WorldTarget,
    options: TakeOptions,
) -> Result<WeaponSlot, StealRefusal> {
    let freed = unsafe { release(hand, target) }?;
    // SAFETY: the strong reference keeps the dropped gun alive.
    let weapon = unsafe { &mut *freed.weapon };
    let taken = unsafe { pickup::take(hand, weapon, options) }.map_err(StealRefusal::Pickup)?;
    STEALS.fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        slot = slot_label(taken),
        "hold: took the gun out of a character's hands into the {} hand",
        hand.label()
    );
    Ok(taken)
}

/// A gun a character let go of: dropped into the world with its bodies, held alive.
pub(crate) struct Freed {
    pub weapon: *mut WeaponBase,
    /// Keeps the gun alive while held.
    pub alive: Strong,
}

/// Make `target`'s character let go of its gun and stagger, leaving the gun lying in the world, for
/// a take into `hand` that follows: the grab's take, or the grapple's snatch, which pulls it in
/// first. A take the hand could not make is refused before the character lets go.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn release(hand: Hand, target: &WorldTarget) -> Result<Freed, StealRefusal> {
    let TargetKind::Held { character } = target.kind else {
        return Err(StealRefusal::Gone);
    };
    // Hold the character alive through the release.
    let alive = {
        let seen = SEEN.lock();
        seen.iter()
            .find(|s| s.character == character)
            .and_then(|s| s.weak.lock())
    };
    let Some(_alive) = alive else {
        return Err(StealRefusal::Gone);
    };
    if character == ik::local_character() {
        return Err(StealRefusal::Gone);
    }
    // SAFETY: the strong reference keeps the character alive.
    let victim = unsafe { &mut *(character as *mut Character) };
    let Some((weapon, slot)) = (unsafe { wielded(victim) }) else {
        return Err(StealRefusal::Gone);
    };
    if weapon as usize != target.weapon || slot != target.slot {
        return Err(StealRefusal::Gone);
    }
    // SAFETY: the character's inventory holds the weapon, and the character is held alive.
    let weapon = unsafe { &mut *weapon };
    let player =
        unsafe { engine::local_player() }.ok_or(StealRefusal::Pickup(Refusal::NoCharacter))?;
    unsafe { pickup::takeable(player, weapon) }.map_err(StealRefusal::Pickup)?;
    // Refused before the character lets go, so a hand that cannot take it leaves it in its hands.
    unsafe { pickup::route(weapon, Some(hand)) }.map_err(StealRefusal::Pickup)?;
    // SAFETY: the weapon's own weak reference to itself, live while it is.
    let alive = unsafe { Weak::of(&weapon.m_WeakThis) }
        .lock()
        .ok_or(StealRefusal::Gone)?;
    let player_at = Mat4::from(player.m_WorldMatrixT1).w_axis.truncate();
    unsafe { weapon.OnDrop() };
    unsafe { victim.m_Inventory.DropWeapon(slot, true) };
    unsafe { stagger(victim, player_at) };
    SEEN.lock().retain(|s| s.character != character);
    Ok(Freed { weapon, alive })
}

/// Make `target`'s character drop the gun holstered on its body and stagger, leaving the gun lying
/// in the world, for the grapple's snatch into `hand`. The character keeps what it wields, and its
/// selection is left alone. A take the hand could not make is refused before the gun leaves.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn release_holstered(
    hand: Hand,
    target: &HolsteredTarget,
) -> Result<Freed, StealRefusal> {
    let alive = {
        let seen = SEEN.lock();
        seen.iter()
            .find(|s| s.character == target.character)
            .and_then(|s| s.weak.lock())
    };
    let Some(_alive) = alive else {
        return Err(StealRefusal::Gone);
    };
    if target.character == ik::local_character() {
        return Err(StealRefusal::Gone);
    }
    // SAFETY: the strong reference keeps the character alive.
    let victim = unsafe { &mut *(target.character as *mut Character) };
    let held = unsafe { holstered(victim) };
    if !held
        .iter()
        .flatten()
        .any(|h| h.weapon == target.weapon && h.slot == target.slot)
    {
        return Err(StealRefusal::Gone);
    }
    // SAFETY: the character's inventory holds the weapon, and the character is held alive.
    let weapon = unsafe { &mut *(target.weapon as *mut WeaponBase) };
    let player =
        unsafe { engine::local_player() }.ok_or(StealRefusal::Pickup(Refusal::NoCharacter))?;
    unsafe { pickup::takeable(player, weapon) }.map_err(StealRefusal::Pickup)?;
    unsafe { pickup::route(weapon, Some(hand)) }.map_err(StealRefusal::Pickup)?;
    // SAFETY: the weapon's own weak reference to itself, live while it is.
    let alive = unsafe { Weak::of(&weapon.m_WeakThis) }
        .lock()
        .ok_or(StealRefusal::Gone)?;
    let player_at = Mat4::from(player.m_WorldMatrixT1).w_axis.truncate();
    unsafe { weapon.OnDrop() };
    unsafe { victim.m_Inventory.DropWeapon(target.slot, false) };
    unsafe { stagger(victim, player_at) };
    Ok(Freed { weapon, alive })
}

/// The takes from characters' hands since the payload loaded.
pub(crate) fn steals() -> u64 {
    STEALS.load(Ordering::Relaxed)
}

/// How far from the player a character's gun is noted, metres.
const NOTICE_RANGE_M: f32 = 3.0;
/// How long a noted character stays a target without being noted again.
const SEEN_FOR: Duration = Duration::from_millis(500);
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
    point: Vec3,
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
static STEALS: AtomicU64 = AtomicU64::new(0);
/// How far from the player a character's gun is noted, metres, as `f32` bits: at least
/// [`NOTICE_RANGE_M`].
static NOTICE_RANGE: AtomicU32 = AtomicU32::new(NOTICE_RANGE_M.to_bits());

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
    // Read as the raw value: an unset slot is out of the enum's range.
    let slot = slot_from_raw(unsafe { (&raw const weapon.m_EquipSlot).cast::<i32>().read() })?;
    // A mounted gun is held at its mount, and the grenade, the explosive, and the beacon are not
    // guns to take.
    matches!(
        slot,
        WeaponSlot::E_SLOT_DUAL_WIELD
            | WeaponSlot::E_SLOT_TWO_HANDED
            | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
    )
    .then_some((pointer, slot))
}

/// The guns `character` carries holstered and visible on its body, by [`HOLSTER_SLOTS`], each
/// with its first gun's origin.
unsafe fn holstered(character: &Character) -> [Option<Holstered>; HOLSTER_SLOTS.len()] {
    HOLSTER_SLOTS.map(|slot| {
        let weapon = unsafe {
            character
                .m_Inventory
                .m_Weapons
                .get(slot as usize)?
                .px
                .as_ref()
        }?;
        // Read as the raw value, so that a state the enum does not name is not misread.
        let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
        if state != WeaponState::E_WS_HOLSTERED as i32 {
            return None;
        }
        let first = unsafe { gun::guns(weapon) }.next().flatten()?;
        let point = Mat4::from(first.m_WorldMatrixT1).w_axis.truncate();
        point.is_finite().then_some(Holstered {
            weapon: weapon as *const WeaponBase as usize,
            slot,
            point,
        })
    })
}

/// The gun's segment in the world (the holding hand's grip, and the other hand's grip or the
/// gun's origin) and its first gun's pose.
pub(crate) unsafe fn gun_points(weapon: &WeaponBase) -> Option<([Vec3; 2], Pose)> {
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    let matrix = Mat4::from(first.m_WorldMatrixT1);
    let (_, orientation, origin) = matrix.to_scale_rotation_translation();
    let grip = |hand| {
        let mut out = Matrix4::from(Mat4::IDENTITY);
        unsafe { weapon.GetGripPosition(hand, &mut out) }
            .then(|| Mat4::from(out).w_axis.truncate())
            .filter(|p| p.is_finite())
    };
    let right = grip(WeaponGripHand::E_WEAPON_GRIP_HAND_RIGHT).unwrap_or(origin);
    let left = grip(WeaponGripHand::E_WEAPON_GRIP_HAND_LEFT).unwrap_or(origin);
    (origin.is_finite() && orientation.is_finite()).then_some((
        [right, left],
        Pose {
            position: origin,
            orientation: orientation.normalize(),
        },
    ))
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
    let at = Mat4::from(victim.m_WorldMatrixT1).w_axis.truncate();
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
