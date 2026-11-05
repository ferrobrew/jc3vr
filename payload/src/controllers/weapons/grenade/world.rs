//! Grenades the world drops, made physical where they appear: a character's drop of a finite hand
//! grenade becomes the mod's own grenade lying in the world, the same inert, pinned item a hand
//! lets go of, rather than a native pickup ("Grenades lying in the world" in
//! `docs/mod/input/grenades.md`).
//!
//! A detour of the drop's base implementation ([`WeaponBase::OnDropBase`]) notes each finite hand
//! grenade a character other than the player drops ([`finite_world_pickup`]); the drop runs in the
//! world update, where nothing may be spawned, so the next input tick's game stage makes the
//! replacement ([`input_tick`]): an item at the native grenade's pose, moving as it moved, made
//! under the draw's pool and stage rules ([`safe::spawn_in_world`]), and only then is the native
//! grenade consumed ([`consume_world_pickup`]). A refused allocation leaves the native grenade
//! where it lies, for the take of a native grenade lying in the world to reach
//! ([`take_world_pickup`](super::take_world_pickup)). Only under the controller scheme: gamepad
//! play keeps the game's pickups.

use std::num::NonZeroUsize;

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{character::character::Character, equipment::weapon::WeaponBase};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::gun,
    physical_hands, scheme,
    weapons::{
        engine::{character_object, gun_pose},
        grenade::{Release, consume_world_pickup, finite_world_pickup, safe},
        hold::refs::{Strong, Weak},
        inventory::engine::weapon_of,
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&ON_DROP_BINDER)
}

/// Replace the dropped grenades noted since the last tick with grenades of the mod's own.
///
/// # Safety
///
/// The input tick's game stage, on the game thread.
pub(super) unsafe fn input_tick() {
    let pending = std::mem::take(&mut *PENDING.lock());
    let mut retry = Vec::new();
    for mut drop in pending {
        let Some(alive) = drop.weapon.lock() else {
            continue;
        };
        match unsafe { replace(&alive) } {
            Replacement::Replaced | Replacement::NotAPickup => {}
            Replacement::Refused if drop.ticks < MAX_TICKS => {
                drop.ticks += 1;
                retry.push(drop);
            }
            Replacement::Refused => {
                tracing::debug!(target: "controllers", "grenade: a dropped native grenade stays a pickup; no grenade of the mod's own could be made");
            }
        }
    }
    let mut pending = PENDING.lock();
    retry.append(&mut pending);
    retry.truncate(MAX_PENDING);
    *pending = retry;
}

/// Forget the drops waiting to be replaced: the controllers stood down.
pub(super) fn clear() {
    PENDING.lock().clear();
}

/// A noted drop.
struct Pending {
    weapon: Weak,
    /// Input ticks the replacement has been refused on.
    ticks: u32,
}

static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());

/// How many drops wait at once; the oldest beyond it are left as pickups.
const MAX_PENDING: usize = 16;
/// How many input ticks a refused replacement is tried again on before the drop is left a pickup.
const MAX_TICKS: u32 = 30;

enum Replacement {
    Replaced,
    /// The weapon is no longer a finite hand grenade lying in the world.
    NotAPickup,
    /// The pool, the stage, or the table refused the item.
    Refused,
}

/// Make a grenade of the mod's own where the native grenade `alive` lies, and consume the native
/// one.
///
/// # Safety
///
/// The input tick's game stage, on the game thread.
unsafe fn replace(alive: &Strong) -> Replacement {
    // SAFETY: the strong reference keeps the weapon alive.
    let weapon = unsafe { &mut *weapon_of(alive) };
    if !finite_world_pickup(weapon) {
        return Replacement::NotAPickup;
    }
    let Some((world, release)) = (unsafe { motion(weapon) }) else {
        return Replacement::NotAPickup;
    };
    if !unsafe { safe::spawn_in_world(world, release) } {
        return Replacement::Refused;
    }
    unsafe { consume_world_pickup(weapon) };
    Replacement::Replaced
}

/// The native grenade's pose, without its scale, and its body's motion, at rest when the body
/// cannot be read.
///
/// # Safety
///
/// `weapon` must be live, on the game thread outside the physics step.
unsafe fn motion(weapon: &WeaponBase) -> Option<(Mat4, Release)> {
    let pose = unsafe { gun_pose(weapon) }?;
    let world = Mat4::from_rotation_translation(pose.orientation, pose.position);
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    let owner = NonZeroUsize::new(weapon as *const WeaponBase as usize)?;
    let release = unsafe { first.m_PfxInstance.as_ref() }
        .and_then(|instance| unsafe { physical_hands::body_state(instance.m_BodyId, owner) }.ok())
        .map(|state| Release {
            velocity: state.velocities.linear,
            angular: state.velocities.angular,
        })
        .filter(|release| release.velocity.is_finite() && release.angular.is_finite())
        .unwrap_or(Release::REST);
    Some((world, release))
}

/// Whether `weapon`'s holder is the local player, whose own grenade weapon is never replaced.
///
/// # Safety
///
/// `weapon` must be live.
unsafe fn held_by_player(weapon: &WeaponBase) -> bool {
    let Some(holder) = (unsafe { Strong::lock(&weapon.m_Holder) }) else {
        return false;
    };
    let player = unsafe { Character::GetLocalPlayerCharacter() };
    !player.is_null() && holder.object() == character_object(player)
}

#[detour(address = jc3gi::equipment::weapon::WeaponBase::OnDropBase_ADDRESS)]
fn on_drop(weapon: *mut WeaponBase) {
    // Read before the drop clears the holder.
    // SAFETY: the engine's own live weapon, for the duration of its drop.
    let by_player = unsafe { weapon.as_ref() }.is_none_or(|w| unsafe { held_by_player(w) });
    ON_DROP.get().unwrap().call(weapon);
    if by_player || !scheme::is_controllers() {
        return;
    }
    // SAFETY: as above.
    let Some(weapon) = (unsafe { weapon.as_ref() }) else {
        return;
    };
    if !finite_world_pickup(weapon) || weapon.m_WeakThis.pn.is_null() {
        return;
    }
    // SAFETY: the weapon's own weak reference to itself, live with it; the count is atomic, so the
    // reference may be taken here, wherever the drop runs.
    let reference = unsafe { Weak::of(&weapon.m_WeakThis) };
    let mut pending = PENDING.lock();
    if pending.len() < MAX_PENDING {
        pending.push(Pending {
            weapon: reference,
            ticks: 0,
        });
    }
}
