//! Taking a weapon off a rack, or out of a locker. The design is in
//! `docs/mod/input/physical-weapons.md`, the engine side in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`.
//!
//! A rack's weapon is placed by its weapon placement rule in the rack state, which keeps no body in
//! the physics world, so the hands' reach cannot find it. The game offers it to the player through
//! the interaction system instead: among the player's valid interaction entries
//! ([`InteractionUserProxy::m_ValidEntries`]), either the weapon's own pickup interaction (a rack
//! that uses the ordinary pickup), or a node standing for the rack's placement rule
//! ([`WeaponPlacementRule`](jc3gi::weapon_placement_rule::WeaponPlacementRule)), whose spawned
//! weapon it is. [`targets`] walks those entries as a swimming character's pickup does
//! ([`entries`]), and the weapon handling offers each weapon to a free hand
//! ([`crate::controllers::weapons::hold::world`]). A weapon an entry offers is taken in the rack
//! state or lying in the world (a locker's weapon is offered the same way out of the rack state,
//! presumably set down at a spot by its placement rule); one a character holds, one already being
//! picked up, and a mounted gun, which a placement rule also spawns, are left out.
//!
//! A grab of one takes it through the hands' take from a source
//! ([`crate::controllers::weapons::hold::source`]), found again among the entries ([`find`]) so
//! that it is known alive, and then ([`take`]):
//!
//! - a weapon the rack hands over goes straight into the grabbing hand, as a weapon off the ground
//!   does, after the call that starts the game's own pickup of it ([`WeaponBase::PickupStart`]);
//! - a rack with unlimited stock hands out a copy instead ([`copy`]): the take asks the rack for
//!   one, which spawns a few frames later where the rack's weapon hangs, and then goes into the
//!   hand as the rack's own weapon would, while the rack's weapon is hidden until it has left.

pub(crate) mod copy;
pub(crate) mod entries;
pub(crate) mod offer;
pub(crate) mod paired;
pub(crate) mod remote;

use std::sync::atomic::{AtomicU64, Ordering};

use glam::Vec3;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponState},
};

use crate::controllers::{
    frame::Hand,
    loadout::{TakeSource, slot_label},
    weapons::{
        engine::{self, character_object, slot_of, takeable_gun_slot, weapon_at},
        hold::{
            pickup::{self, Refusal, TakeOptions},
            refs::Strong,
            source::SourceRefusal,
            steal,
            world::{TargetKind, WorldTarget},
        },
        inventory::engine as inventory_engine,
    },
};

/// The rack weapons the player's interaction entries offer now, with their points carried into the
/// cockpit frame by `to_cockpit`.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub(crate) unsafe fn targets(
    owner: &Character,
    to_cockpit: impl Fn(Vec3) -> Vec3,
) -> Vec<WorldTarget> {
    let mut out: Vec<WorldTarget> = Vec::new();
    unsafe {
        walk(owner, |weapon, alive| {
            if paired::targets(owner, weapon, alive, &to_cockpit, &mut out) {
                return false;
            }
            let address = weapon as *const WeaponBase as usize;
            if out.iter().any(|t| t.weapon == address) {
                return false;
            }
            let Some(slot) = rack_slot(weapon) else {
                return false;
            };
            let Some((points, gun)) = steal::gun_points(weapon) else {
                return false;
            };
            out.push(WorldTarget {
                kind: TargetKind::Rack { weapon: address },
                weapon: address,
                slot,
                points: points.map(&to_cockpit),
                gun,
            });
            false
        })
    };
    out
}

/// The weapon at `weapon` while the local player's interaction entries still offer it, held alive.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn find(weapon: usize) -> Option<Strong> {
    let owner = unsafe { engine::local_player() }?;
    let mut found = None;
    unsafe {
        walk(owner, |offered, strong| {
            if offered as *const WeaponBase as usize == weapon {
                found = Some(strong);
                return true;
            }
            false
        })
    };
    found
}

/// Whether the weapon `alive` holds is in the rack state.
pub(crate) fn racked(alive: &Strong) -> bool {
    // SAFETY: the strong reference keeps the weapon alive.
    unsafe { (*inventory_engine::weapon_of(alive)).m_State == WeaponState::E_WS_WEAPON_RACK }
}

/// Take the offered weapon `alive` holds off its rack, or out of its locker, into `hand`.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn take(
    hand: Hand,
    alive: &Strong,
    options: TakeOptions,
) -> Result<WeaponSlot, SourceRefusal> {
    let owner =
        unsafe { engine::local_player() }.ok_or(SourceRefusal::Take(Refusal::NoCharacter))?;
    // SAFETY: the strong reference keeps the weapon alive.
    let weapon = unsafe { &mut *inventory_engine::weapon_of(alive) };
    if !offered_state(weapon) {
        return Err(SourceRefusal::Gone);
    }
    // A copy is asked of a rack or locker with unlimited stock, whose pickup gives one whatever its
    // state; anything else hands its own weapon over.
    let racked = weapon.m_State == WeaponState::E_WS_WEAPON_RACK;
    let restocks = weapon.m_InfinitePickup;
    let admitted =
        unsafe { pickup::preflight(owner, weapon, hand, restocks) }.map_err(SourceRefusal::Take)?;
    if !restocks {
        // A rack's weapon is offered in the rack state, and a locker's lying in the world where its
        // placement rule set it down.
        if !racked && weapon.m_State != WeaponState::E_WS_DROPPED {
            let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
            return Err(SourceRefusal::Take(Refusal::NotOffered { state }));
        }
        if racked {
            // Takes it out of the rack's state and tells the rack it was picked up.
            unsafe { weapon.PickupStart(character_object(owner)) };
        }
        let options = TakeOptions {
            source: if racked {
                TakeSource::Rack
            } else {
                TakeSource::Locker
            },
            ..options
        };
        let slot = unsafe { pickup::deliver(owner, hand, weapon, admitted, options) }
            .map_err(SourceRefusal::Take)?;
        RACK_TAKES.fetch_add(1, Ordering::Relaxed);
        tracing::info!(
            target: "controllers",
            hand = hand.label(),
            slot = slot_label(slot),
            "hold: took the weapon off a rack into the {} hand",
            hand.label()
        );
        return Ok(slot);
    }
    unsafe { copy::request(owner, hand, weapon) }?;
    Ok(admitted.0)
}

/// Retire the pairs no longer offered, and show a rack's weapon again once its copy has left.
/// Called every input tick.
pub(crate) fn tick() {
    paired::tick();
    copy::tick();
}

/// Show the racks' weapons again, forget the copies and the pairs, and forget the weapons noted for
/// the snatch. Called when the weapon handling stands down.
pub(crate) fn stand_down() {
    paired::clear();
    copy::clear();
    remote::clear();
}

/// The takes off racks since the payload loaded: weapons handed over, and copies asked for.
pub(crate) fn counts() -> (u64, u64) {
    (RACK_TAKES.load(Ordering::Relaxed), copy::count())
}

static RACK_TAKES: AtomicU64 = AtomicU64::new(0);

/// Walk the weapons `owner`'s valid interaction entries offer that sit on a rack, with a strong
/// reference holding each alive, until `visit` returns true.
unsafe fn walk(owner: &Character, mut visit: impl FnMut(&WeaponBase, Strong) -> bool) {
    unsafe {
        entries::walk(&owner.m_InteractionUserProxy.m_ValidEntries, |strong| {
            let weapon = weapon_at(strong.object()).as_ref();
            weapon
                .filter(|w| offered_state(w))
                .is_some_and(|w| visit(w, strong))
        })
    };
}

/// The slot of the first weapon `owner`'s valid interaction entries offer, as a swimming
/// character's pickup takes it, with whether the table would take that weapon.
///
/// # Safety
///
/// `owner` must be the live local player character.
pub(crate) unsafe fn first_offered(
    owner: &Character,
    takes: impl Fn(&WeaponBase) -> bool,
) -> Option<WeaponSlot> {
    let mut found = None;
    unsafe {
        walk(owner, |weapon, _| {
            found = takes(weapon).then(|| slot_of(weapon)).flatten();
            true
        })
    };
    found
}

/// Whether an offered weapon in `weapon`'s state may be taken: any but one a character holds,
/// wields, or carries, and one already being picked up.
fn offered_state(weapon: &WeaponBase) -> bool {
    // Read as the raw value, so that a state the enum does not name is not misread.
    let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
    ![
        WeaponState::E_WS_WIELDING,
        WeaponState::E_WS_DUAL_WIELDING,
        WeaponState::E_WS_HOLSTERED,
        WeaponState::E_WS_HOLSTERED_INVISIBLE,
        WeaponState::E_WS_PICKING_UP,
        WeaponState::E_WS_WINGSUIT_WEAPON,
        WeaponState::E_WS_MOUNTED,
        WeaponState::E_WS_TURRET,
        WeaponState::E_WS_VEHICLE,
    ]
    .into_iter()
    .any(|s| s as i32 == state)
}

/// The slot a rack weapon goes into, for the slots a hand holds.
fn rack_slot(weapon: &WeaponBase) -> Option<WeaponSlot> {
    unsafe { slot_of(weapon) }.filter(|&slot| takeable_gun_slot(slot))
}
