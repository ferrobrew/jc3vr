//! Rack and locker weapons beyond the hands' reach, for the grapple's snatch. The design is in
//! `docs/mod/input/grapple-snatch.md`, "Phase 2: racks and lockers".
//!
//! The hands find a rack's weapon through the player's valid interaction entries
//! ([`crate::controllers::weapons::hold::rack`]), which hold only what is within reach, and the
//! weapon has no body for the aim to hit. So the weapons a placement rule set out are noted as the
//! game renders them ([`observe`]), each by a weak reference, and the snatch looks among those
//! near its ray ([`near`]). A weapon is noted while it waits on its rule: in the rack state, or
//! lying where its rule set it down (a locker's). A rack whose weapon hands out copies
//! ([`WeaponBase::m_InfinitePickup`] in the rack state) is left to the hands, whose take asks it for
//! a copy; a weapon of the dual-wield slot is left to them too, since a rack's pistols are a pair
//! the hands take one at a time ([`crate::controllers::weapons::hold::rack::paired`]).
//!
//! The snatch frees the weapon as the hand's take starts: the rack hears that its weapon was picked
//! up ([`WeaponBase::PickupStart`]), so a finite rack is left as empty as a hand would leave it.
//! The weapon then drops into the world where it hangs ([`WeaponBase::OnDrop`]), and the line pulls
//! it in as any weapon lying in the world ([`release`]).

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::{
    equipment::weapon::{WeaponBase, WeaponState},
    graphics_engine::model_instance::ModelInstanceFlags,
};
use parking_lot::Mutex;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    loadout::TakeSource,
    weapons::{
        engine::{self, character_object, long_slot, object_of, slot_of, takeable_gun_slot},
        hold::{
            pickup::{self, Refusal},
            refs::Weak,
            source::{Freed, SourceRefusal},
        },
    },
};

/// A rack or locker weapon the snatch can be offered.
pub(crate) struct RemoteRack {
    pub weapon: NonZeroUsize,
    /// Its first gun's position, world space.
    pub point: Vec3,
    pub weak: Weak,
}

/// Note `weapon` when its placement rule holds it out for the taking.
///
/// # Safety
///
/// The caller runs the weapon's native update or render pass and keeps it live for this call.
pub(crate) unsafe fn observe(weapon: &WeaponBase) {
    if !unsafe { waiting(weapon) } || weapon.m_WeakThis.pn.is_null() {
        return;
    }
    let address = weapon as *const WeaponBase as usize;
    let mut registry = REGISTRY.lock();
    if registry.iter().any(|seen| seen.weapon == address) {
        return;
    }
    if registry.len() >= MAX_SEEN {
        registry.remove(0);
    }
    // SAFETY: the weapon's own weak reference to itself, live with it; the count is atomic, so the
    // reference may be taken wherever the render pass runs.
    let weak = unsafe { Weak::of(&weapon.m_WeakThis) };
    registry.push(Seen {
        weapon: address,
        weak,
    });
}

/// The noted weapons still waiting on their rules whose first gun shows and passes `near`, held
/// alive only by their weak references. Forgets the weapons that are gone or taken.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn near(near: impl Fn(Vec3) -> bool) -> Vec<RemoteRack> {
    let mut registry = REGISTRY.lock();
    let mut out = Vec::new();
    registry.retain(|seen| {
        let Some(alive) = seen.weak.lock() else {
            return false;
        };
        // SAFETY: the strong reference keeps the weapon alive.
        let weapon = unsafe { &*(seen.weapon as *const WeaponBase) };
        if alive.object() != object_of(weapon) || !unsafe { waiting(weapon) } {
            return false;
        }
        if let Some(point) = unsafe { shown_point(weapon) }.filter(|&p| near(p))
            && let Some(weapon) = NonZeroUsize::new(seen.weapon)
        {
            out.push(RemoteRack {
                weapon,
                point,
                weak: seen.weak.clone(),
            });
        }
        true
    });
    out
}

/// Where `target`'s first gun is now, world space, while it still waits on its rule and shows.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn current(weapon: NonZeroUsize, weak: &Weak) -> Option<Vec3> {
    let _alive = weak.lock()?;
    // SAFETY: the strong reference keeps the weapon alive.
    let weapon = unsafe { &*(weapon.get() as *const WeaponBase) };
    if !unsafe { waiting(weapon) } {
        return None;
    }
    unsafe { shown_point(weapon) }
}

/// Free `weapon` from its rack or locker for the snatch into `hand`: tell the rack it was picked
/// up, as the hand's take does, and drop it into the world where it hangs, for the line to pull
/// in. A take the hand could not make is refused before the weapon leaves.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn release(
    hand: Hand,
    weapon: NonZeroUsize,
    weak: &Weak,
) -> Result<Freed, SourceRefusal> {
    let alive = weak.lock().ok_or(SourceRefusal::Gone)?;
    // SAFETY: the strong reference keeps the weapon alive.
    let weapon = unsafe { &mut *(weapon.get() as *mut WeaponBase) };
    if !unsafe { waiting(weapon) } {
        return Err(SourceRefusal::Gone);
    }
    let owner =
        unsafe { engine::local_player() }.ok_or(SourceRefusal::Take(Refusal::NoCharacter))?;
    unsafe { pickup::preflight(owner, weapon, hand, false) }.map_err(SourceRefusal::Take)?;
    let from_rack = weapon.m_State == WeaponState::E_WS_WEAPON_RACK;
    if from_rack {
        unsafe { weapon.PickupStart(character_object(owner)) };
    }
    unsafe { weapon.OnDrop() };
    let address = weapon as *mut WeaponBase as usize;
    REGISTRY.lock().retain(|seen| seen.weapon != address);
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        from_rack,
        "snatch: freed the weapon from its {} for the {} hand",
        if from_rack { "rack" } else { "locker" },
        hand.label()
    );
    Ok(Freed {
        weapon,
        alive,
        source: if from_rack {
            TakeSource::Rack
        } else {
            TakeSource::Locker
        },
    })
}

/// Forget every noted weapon. Called when the weapon handling stands down.
pub(crate) fn clear() {
    let forgotten = std::mem::take(&mut *REGISTRY.lock());
    drop(forgotten);
}

/// The most weapons noted at once; the oldest is forgotten for a new one.
const MAX_SEEN: usize = 64;

struct Seen {
    weapon: usize,
    weak: Weak,
}

static REGISTRY: Mutex<Vec<Seen>> = Mutex::new(Vec::new());

/// Whether `weapon` waits on its placement rule for the snatch to take it: a long gun in a rack
/// that hands over its own weapon, or one lying where a rule set it down.
///
/// # Safety
///
/// `weapon` must be live.
unsafe fn waiting(weapon: &WeaponBase) -> bool {
    // Read as the raw value: a state the enum does not name is out of its range.
    let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
    // The two-handed and special slots: the long guns a hand takes, not the heavy slot's.
    let long_gun =
        unsafe { slot_of(weapon) }.is_some_and(|slot| long_slot(slot) && takeable_gun_slot(slot));
    let racked = state == WeaponState::E_WS_WEAPON_RACK as i32 && !weapon.m_InfinitePickup;
    let set_down = state == WeaponState::E_WS_DROPPED as i32
        && !weapon.m_SpawnedFromRule.pn.is_null()
        && weapon.m_Holder.pn.is_null();
    long_gun && (racked || set_down)
}

/// `weapon`'s first gun's position, world space, while its model shows.
///
/// # Safety
///
/// `weapon` must be live, on the game thread.
unsafe fn shown_point(weapon: &WeaponBase) -> Option<Vec3> {
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    let model = unsafe { first.m_BaseModelInstance.as_ref() }?;
    if !model.m_Flags.contains(ModelInstanceFlags::VISIBLE) {
        return None;
    }
    unsafe { engine::gun_pose(weapon) }.map(|pose| pose.position)
}
