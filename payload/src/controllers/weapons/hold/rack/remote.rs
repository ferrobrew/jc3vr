//! Rack and locker weapons beyond the hands' reach, for the grapple's snatch. The design is in
//! `docs/mod/input/grapple-snatch.md`, "Phase 2: racks and lockers".
//!
//! The hands find a rack's weapon through the player's valid interaction entries
//! ([`crate::controllers::weapons::hold::rack`]), which hold only what is within reach, and the
//! weapon has no body for the aim to hit. So the weapons a placement rule set out are noted as the
//! game renders them ([`observe`]), each by a weak reference, and the snatch looks among those
//! near its ray ([`near`]), each gun of a rack's pistol pair on its own. A weapon is noted while it
//! waits on its rule: in the rack state, or lying where its rule set it down (a locker's). Noting
//! is all the render pass does; everything that changes a weapon runs in the input tick.
//!
//! The snatch frees the weapon as the hand's take does, through the take from a source
//! ([`crate::controllers::weapons::hold::source`]). A rack that hands its own weapon over hears
//! that its weapon was picked up ([`WeaponBase::PickupStart`]), so a finite rack is left as empty
//! as a hand would leave it, and the weapon drops into the world where it hangs
//! ([`WeaponBase::OnDrop`]) for the line to pull in ([`release`]). A rack with unlimited stock
//! hands out a copy ([`crate::controllers::weapons::hold::rack::copy`]), and a rack's pistol pair
//! gives up the one pistol aimed at
//! ([`crate::controllers::weapons::hold::rack::paired`]), each placed where it hung and dropped
//! into the world the same way.

use std::num::NonZeroUsize;

use glam::{Mat4, Vec3};
use jc3gi::{
    character::inventory::WeaponSlot,
    equipment::weapon::{WeaponBase, WeaponState},
    graphics_engine::model_instance::ModelInstanceFlags,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    weapons::{
        dual_wield::single,
        engine::{self, character_object, object_of, slot_of, takeable_gun_slot},
        hold::{
            pickup::{self, Refusal},
            rack::{offered_source, paired},
            refs::Weak,
            source::{Freed, SourceRefusal},
        },
    },
};

/// A rack or locker weapon the snatch can be offered, at one of its guns.
pub(crate) struct RemoteRack {
    pub weapon: NonZeroUsize,
    /// Which of its guns: the first, or the second of a rack's pistol pair.
    pub gun: u8,
    /// That gun's position, world space.
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
        let Some(address) = NonZeroUsize::new(seen.weapon) else {
            return true;
        };
        for gun in 0..unsafe { shown_guns(weapon) } {
            if let Some(point) = unsafe { shown_point(weapon, gun) }.filter(|&p| near(p)) {
                out.push(RemoteRack {
                    weapon: address,
                    gun,
                    point,
                    weak: seen.weak.clone(),
                });
            }
        }
        true
    });
    out
}

/// Where `weapon`'s gun `gun` is now, world space, while it still waits on its rule and shows.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn current(weapon: NonZeroUsize, gun: u8, weak: &Weak) -> Option<Vec3> {
    let _alive = weak.lock()?;
    // SAFETY: the strong reference keeps the weapon alive.
    let weapon = unsafe { &*(weapon.get() as *const WeaponBase) };
    if !unsafe { waiting(weapon) } {
        return None;
    }
    unsafe { shown_point(weapon, gun) }
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
    let source = offered_source(weapon);
    if from_rack {
        unsafe { weapon.PickupStart(character_object(owner)) };
    }
    unsafe { weapon.OnDrop() };
    let address = weapon as *mut WeaponBase as usize;
    REGISTRY.lock().retain(|seen| seen.weapon != address);
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        source = source.label(),
        "snatch: freed the weapon for the {} hand",
        hand.label()
    );
    Ok(Freed {
        weapon,
        alive,
        source,
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

/// Whether `weapon` waits on its placement rule for the snatch to take it: a gun of a slot a hand
/// holds on a rack, or one lying where a rule set it down that hands over itself rather than a
/// copy (a copy placed at a weapon still lying there would overlap its bodies).
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn waiting(weapon: &WeaponBase) -> bool {
    // Read as the raw value: a state the enum does not name is out of its range.
    let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
    let gun = unsafe { slot_of(weapon) }.is_some_and(takeable_gun_slot);
    let racked = state == WeaponState::E_WS_WEAPON_RACK as i32;
    let set_down = state == WeaponState::E_WS_DROPPED as i32
        && !weapon.m_InfinitePickup
        && !weapon.m_SpawnedFromRule.pn.is_null()
        && weapon.m_Holder.pn.is_null();
    gun && (racked || set_down)
}

/// How many of `weapon`'s guns the snatch offers on their own: both of a rack's pistol pair, and
/// otherwise the first.
///
/// # Safety
///
/// `weapon` must be live.
unsafe fn shown_guns(weapon: &WeaponBase) -> u8 {
    let pair = unsafe { slot_of(weapon) } == Some(WeaponSlot::E_SLOT_DUAL_WIELD)
        && unsafe { paired::whole_pair(weapon) }
        && !single::is(weapon as *const WeaponBase as usize);
    if pair { 2 } else { 1 }
}

/// `weapon`'s gun `gun`'s position, world space, while its model shows.
///
/// # Safety
///
/// `weapon` must be live, on the game thread.
unsafe fn shown_point(weapon: &WeaponBase, gun: u8) -> Option<Vec3> {
    let data = if gun == 0 {
        unsafe { engine::gun_at(weapon, 0) }
    } else {
        unsafe { paired::gun(weapon, usize::from(gun)) }
    }?;
    let model = unsafe { data.m_BaseModelInstance.as_ref() }?;
    if !model.m_Flags.contains(ModelInstanceFlags::VISIBLE) {
        return None;
    }
    let at = Mat4::from(data.m_WorldMatrixT1).w_axis.truncate();
    at.is_finite().then_some(at)
}
