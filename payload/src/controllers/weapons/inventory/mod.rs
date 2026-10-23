//! The VR inventory: every weapon the player owns, holds, or has let go of nearby, as one object
//! that belongs to the world, a hand, or the body. The design is in
//! `docs/mod/input/two-handed-inventory.md`, the engine side in
//! `docs/engine/gameplay/weapon-ownership-and-despawn.md`.
//!
//! - [`world`]: the game's side of the loadout table's weapons, what became of each the old
//!   handling no longer keeps, and the pins' references;
//! - [`pins`]: which weapons lying in the world are kept from the game's despawn;
//! - [`sockets`]: the places on the body a weapon beyond the slots docks;
//! - [`carry`]: the table's weapons on the body that no slot holds, kept alive, placed, and shown;
//! - [`reconcile`]: the game's slots, its wielded weapon, and the weapons armed in the hands, made
//!   to follow the table each tick;
//! - [`persist`] and [`restore`]: the carried weapons across resets, saves, and loads;
//! - [`missions`]: the missions' holds of the player's weapons and the checkpoint snapshot;
//! - [`challenges`]: the challenges' holds of the player's weapons;
//! - [`lost`]: the lost-weapon return;
//! - [`traversal`]: the base layer's weapon tasks refused, and the forced neutral state undone;
//! - [`gives`]: the game's gives, placed by the table rather than equipped;
//! - [`ammo`]: the ammunition auto-pickup, for every weapon the table has on the body;
//! - [`engine`]: the engine reads behind them;
//! - [`reset`]: the spawn system's reset, which replaces or removes every weapon;
//! - [`config`]: the settings.
//!
//! Each input tick feeds the live loadout table ([`crate::controllers::loadout::live`]) what the
//! game did, and then makes the game follow it. The table is the record of every weapon the player
//! has, the ones lying in the world included, which the pins read, and the only writer of where
//! they are.
//!
//! A weapon is kept alive and out of the game's despawn by a strong reference: an inventory slot
//! is one, and the inventory takes its own for a weapon it pins. Everything here runs on the input
//! tick on the main thread outside the world update, except the reset's detour, which takes the
//! state's lock only for the local player's inventory.

mod ammo;
pub(crate) mod carry;
mod challenges;
pub mod config;
pub(crate) mod engine;
pub(crate) mod gives;
pub mod lost;
mod missions;
pub mod persist;
pub mod pins;
pub mod reconcile;
mod reset;
pub mod restore;
pub mod sockets;
pub mod traversal;
mod world;

use std::sync::atomic::{AtomicBool, Ordering};

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        loadout::{ItemKind, ObjectAddress, Place, Restored, SlotObject, Socket, TakeSource, live},
        physical_hands,
        weapons::{
            c4,
            dual_wield::single,
            engine as weapons_engine, grenade,
            hold::{armed, refs::Strong, use_item},
            inventory::{
                config::InventoryConfig, engine::ObjectKey, pins::WorldWeapon, world::World,
            },
        },
    },
};

/// The inventory's hooks, aggregated by [`crate::controllers::weapons::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(reset::hook_library())
        .with_hook_library(carry::hook_library())
        .with_hook_library(missions::hook_library())
        .with_hook_library(challenges::hook_library())
        .with_hook_library(gives::hook_library())
        .with_hook_library(ammo::hook_library())
        .with_hook_library(traversal::hook_library())
}

/// The inventory's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InventoryStatus {
    /// Whether the last input tick ran the inventory.
    pub active: bool,
    /// How many weapons the loadout table has on the body, the hands included.
    pub owned: usize,
    /// The weapons the table has lying in the world, each with its distance and its pin.
    pub world: Vec<WorldWeapon>,
    /// The carried weapons: each one's slot and socket.
    pub carried: Vec<(WeaponSlot, Socket)>,
    /// How many weapons the player let go of another character holds.
    pub with_others: usize,
    /// Weapons pinned and forgotten since the payload loaded.
    pub pins: u64,
    pub forgotten: u64,
    /// The spawn system resets seen since the payload loaded.
    pub resets: u64,
    /// Whether the carried weapons are stashed while a mission holds the player's weapons.
    pub stashed: bool,
    /// Weapons a pickup would have replaced that kept their place on the body, since the payload
    /// loaded.
    pub carried_replaced: u64,
}

impl InventoryStatus {
    /// How many weapons in the world are pinned.
    pub fn pinned(&self) -> usize {
        self.world.iter().filter(|w| w.pinned).count()
    }
}

pub fn status() -> InventoryStatus {
    STATE.lock().status.clone()
}

/// What the weapon handling hands the inventory each input tick.
pub(crate) struct TickInputs {
    pub settings: InventoryConfig,
    /// The dominant hand, whose hip takes a pistol first, and which a weapon the game wields on
    /// its own lands in when it is free.
    pub dominant: Hand,
    /// The weapon the game wields in the hands.
    pub wielded: Option<ObjectKey>,
    /// Whether the pistols are made separate ([`single`]).
    pub separate_pistols: bool,
}

/// Fold one input tick of the game into the loadout table, make the game's slots and hands follow
/// the table ([`reconcile`]), and act on the pins. Returns the wielded weapon's arrival in a hand,
/// when it came into one this tick.
///
/// The table runs while the inventory is on and its slots are a projection of the table
/// (`project_slots`). Switching either off, or the scheme away from the controllers, stops it
/// ([`stop`]): the table is put back into the game's slots, and the game's own one-weapon model
/// runs. The weapons the slots cannot hold stay carried, stashed until the table runs again,
/// which starts it from the slots and brings them back onto their sockets.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(
    owner: &mut Character,
    inputs: &TickInputs,
) -> Option<live::Arrival> {
    let on = inputs.settings.enabled && inputs.settings.project_slots;
    traversal::publish(on && inputs.settings.keep_through_traversal);
    engine::set_heavy_tracked(inputs.settings.heavy_on_back);
    // Read before the inventory's lock: each is behind its own subsystem's lock.
    let others = held_others();
    if on {
        let slots = unsafe { live_slots(owner) };
        let spawning = engine::tracked_slots()
            .any(|slot| owner.m_Inventory.m_WeaponPendingSpawn[slot as usize]);
        live::begin_tick(inputs.dominant, &slots, spawning);
        unsafe { restore::input_tick(owner, &inputs.settings) };
        unsafe { use_item::input_tick(owner) };
        unsafe { gives::input_tick(owner) };
        unsafe { resume() };
    } else {
        unsafe { stop(owner, "the inventory is off", inputs.settings.carry) };
        return None;
    }
    let mut state = STATE.lock();
    if !inputs.settings.carry && carry::count() > 0 {
        unsafe { carry::drop_all() };
    }
    state.status.active = true;
    let player = engine::position(owner);
    let slots = unsafe { engine::slots(owner) };
    unsafe { missions::input_tick() };
    for (_, weapon) in &slots {
        // SAFETY: the slot keeps the weapon alive.
        unsafe { state.world.watch(weapon) };
    }
    let offsets = inputs.settings.offsets;
    // A pistol on a hip sits in its holster, unless the game's pair takes both holsters.
    let pair = slots
        .iter()
        .find(|(s, _)| s.kind.slot == WeaponSlot::E_SLOT_DUAL_WIELD)
        .is_some_and(|(s, _)| !single::is(s.object));
    carry::set_offsets(|socket| sockets::placed_offset(&offsets, socket, pair));
    let carried = unsafe { carry::seen() };
    for (seen, _) in &carried {
        // SAFETY: the carry set keeps the weapon alive.
        unsafe { state.world.watch(&*(seen.object as *const WeaponBase)) };
    }
    // SAFETY: the wielded weapon was read live this tick.
    let wielded = inputs.wielded.and_then(|object| unsafe { held_of(object) });
    if let Some(held) = wielded {
        // SAFETY: as above; a heavy weapon is in no tracked slot, and is watched here.
        unsafe {
            state
                .world
                .watch(&*(held.object.get() as *const WeaponBase))
        };
    }
    let armed = Hand::BOTH.map(|hand| armed::in_hand(hand).and_then(ObjectAddress::from_address));
    let live_slots: Vec<SlotObject> = slots
        .iter()
        .filter_map(|(s, _)| live_slot(s.object))
        .collect();
    let live_carried: Vec<(ObjectAddress, u32)> = carried
        .iter()
        .filter_map(|(s, _)| Some((ObjectAddress::from_address(s.object)?, s.kind.tag_hash)))
        .collect();
    // The table's weapons in no slot, the carry set, or a hand of the game's, and the ones another
    // character took, are asked what became of them.
    let kept_by_game = |object: &ObjectAddress| {
        live_slots.iter().any(|slot| slot.object == *object)
            || live_carried.iter().any(|(o, _)| o == object)
            || wielded.is_some_and(|w| w.object == *object)
            || armed.contains(&Some(*object))
    };
    let unplaced: Vec<ObjectAddress> = live::bound()
        .into_iter()
        .map(|(object, _)| object)
        .filter(|object| !kept_by_game(object))
        .chain(state.world.with_others())
        .collect();
    let fates = unsafe { state.world.fates(&unplaced, owner, player) };
    let arrival = live::input_tick(&live::Tick {
        slots: &live_slots,
        carried: &live_carried,
        wielded,
        armed,
        fates: &fates,
        others: &others,
    });
    let returning: Vec<(lost::Lying, Option<Strong>)> = live::bound()
        .into_iter()
        .filter_map(|(object, place)| match place {
            Place::World(departure) => Some((object, departure)),
            Place::Hand(_) | Place::Socket(_) => None,
        })
        .map(|(object, departure)| {
            let strong = state.world.strong_of(object);
            // SAFETY: the strong reference keeps the weapon alive while it is read.
            let at = strong
                .as_ref()
                .and_then(|s| unsafe { engine::origin(&*engine::weapon_of(s)) });
            (
                lost::Lying {
                    object,
                    departure,
                    at,
                },
                strong,
            )
        })
        .collect();
    unsafe {
        lost::input_tick(
            owner,
            lost::LostTick {
                enabled: inputs.settings.return_lost && inputs.settings.carry,
                lost_speed_mps: inputs.settings.lost_speed_mps,
                rules: inputs.settings.lost_rules(),
                player,
                lying: returning,
            },
        )
    };
    let lying: Vec<ObjectAddress> = live::bound()
        .into_iter()
        .filter(|(_, place)| matches!(place, Place::World(_)))
        .map(|(object, _)| object)
        .collect();
    let plan = pins::plan(&state.world.world(&lying), &inputs.settings.pins());
    state.status.pins += state.world.apply(&plan);
    if !plan.forget.is_empty() {
        state.status.forgotten += plan.forget.len() as u64;
        live::forgotten(&plan.forget);
    }
    let bound = live::bound();
    let kept: Vec<ObjectAddress> = bound.iter().map(|(object, _)| *object).collect();
    let lying: Vec<ObjectAddress> = bound
        .iter()
        .filter(|(_, place)| matches!(place, Place::World(_)))
        .map(|(object, _)| *object)
        .collect();
    let taken: Vec<ObjectAddress> = fates
        .iter()
        .filter(|(_, fate)| *fate == live::Fate::HeldByOther)
        .map(|(object, _)| *object)
        .collect();
    let forget_m = inputs.settings.pins().forget_m;
    state.world.settle(&kept, &lying, &taken, forget_m);
    let world = state.world.world(&lying);
    state.status.owned = bound.len() - world.len();
    state.status.world = world;
    state.status.carried = carried
        .iter()
        .map(|(seen, socket)| (seen.kind.slot, *socket))
        .collect();
    state.status.with_others = state.world.held_by_others();
    state.status.stashed = carry::stashed();
    let tracked: Vec<ObjectKey> = kept
        .iter()
        .copied()
        .chain(state.world.with_others())
        .map(ObjectAddress::get)
        .collect();
    // The single pistols lying in the world are kept to their first gun too.
    let singles: Vec<ObjectAddress> = lying
        .iter()
        .copied()
        .filter(|object| single::is(object.get()))
        .collect();
    let world_singles: Vec<Strong> = state.world.strong(&singles);
    drop(state);
    for strong in &world_singles {
        // SAFETY: the strong reference keeps the weapon alive while it is written.
        unsafe { single::hide_extra(&mut *engine::weapon_of(strong)) };
    }
    drop(world_singles);
    unsafe {
        single::input_tick(
            owner,
            &single::Tick {
                separate: inputs.separate_pistols,
                settings: &inputs.settings,
                dominant: inputs.dominant,
                tracked: &tracked,
            },
        )
    };
    unsafe { reconcile::apply(owner) };
    let slot_objects: Vec<usize> = owner
        .m_Inventory
        .m_Weapons
        .iter()
        .map(|slot| slot.px as usize)
        .filter(|&object| object != 0)
        .collect();
    // SAFETY: the slots were read live this tick.
    unsafe { ammo::publish(&slot_objects) };
    arrival
}

/// Stop the table for the scheme leaving the controllers. Called on every input tick the bindings
/// do not inject; it does nothing once stopped.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn stand_down() {
    let Some(owner) = (unsafe { weapons_engine::local_player() }) else {
        return;
    };
    let carry = Config::lock_query(|c| c.controllers.weapons.inventory.carry);
    unsafe { stop(owner, "the controllers are not in use", carry) };
}

/// Stop the table (`why` for the log): put it back into the game's slots, so the game's own
/// one-weapon model takes over with the best of what the player has, and keep the weapons the
/// slots cannot hold carried and stashed (`keep`) until the table runs again, or let them fall
/// where they are without `keep`. Does nothing while the table is stopped.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
unsafe fn stop(owner: &mut Character, why: &str, keep: bool) {
    unsafe { use_item::stand_down() };
    unsafe { gives::stand_down() };
    ammo::clear();
    lost::clear();
    traversal::publish(false);
    let active = STATE.lock().status.active;
    if active {
        unsafe { reconcile::project_back(owner) };
    }
    live::stop();
    if !active {
        return;
    }
    STATE.lock().release_all(why);
    if keep && carry::count() > 0 {
        unsafe { carry::set_stashed(true) };
        KEPT_STOPPED.store(true, Ordering::Relaxed);
        tracing::info!(
            target: "controllers",
            carried = carry::count(),
            why,
            "inventory: the table stopped; the weapons beyond the slots are kept stashed"
        );
    } else {
        unsafe { carry::drop_all() };
        restore::clear();
        single::clear();
    }
}

/// The table runs again after a stop that kept the weapons beyond the slots: it started from the
/// slots, and each kept weapon comes back onto its socket, with any a reset recorded meanwhile.
///
/// # Safety
///
/// The main thread, outside the world update, after the table's start.
unsafe fn resume() {
    if !KEPT_STOPPED.swap(false, Ordering::Relaxed) {
        return;
    }
    let kept = unsafe { carry::seen() };
    for (seen, socket) in &kept {
        // SAFETY: the carry set keeps the weapon alive.
        let Some(weapon) = (unsafe { (seen.object as *const WeaponBase).as_ref() }) else {
            continue;
        };
        let (Some(object), Some(weapon)) = (engine::object_address(weapon), unsafe {
            engine::weapon_type(weapon)
        }) else {
            continue;
        };
        live::restored(Restored {
            object,
            weapon,
            socket: *socket,
        });
    }
    unsafe { carry::set_stashed(false) };
    let respawning = restore::give_back();
    tracing::info!(
        target: "controllers",
        kept = kept.len(),
        respawning,
        "inventory: the table runs again; the kept weapons are back on the body"
    );
}

/// Whether the last stop kept the weapons beyond the slots for the table's next start.
static KEPT_STOPPED: AtomicBool = AtomicBool::new(false);

/// The weapon at `object`, read live this tick, as the table takes one into a hand: a weapon of
/// a tracked slot, or a heavy weapon, which the table holds only while a hand holds it.
///
/// # Safety
///
/// `object` must be a live weapon, read this tick.
pub(crate) unsafe fn held_of(object: ObjectKey) -> Option<live::Held> {
    let weapon = unsafe { (object as *const WeaponBase).as_ref() }?;
    Some(live::Held {
        object: engine::object_address(weapon)?,
        weapon: unsafe { engine::weapon_type(weapon) }
            .filter(|w| engine::tracks(w.slot()) || w.slot() == WeaponSlot::E_SLOT_HEAVY)?,
    })
}

/// What the hands hold that is not a weapon, from the subsystems that hold them: the physical
/// hands' props, the throw's grenade or the caught one, and the C4 throw's charge, each with the
/// hands that threw theirs since the last tick.
fn held_others() -> live::HeldOthers {
    let mut hands = [None; 2];
    for (hand, prop) in Hand::BOTH.into_iter().zip(physical_hands::held_props()) {
        hands[hand.index()] = prop.map(|object| live::HeldOther {
            object: ObjectAddress::new(object),
            kind: ItemKind::Prop,
            source: TakeSource::World,
        });
    }
    let grenade =
        grenade::held().map(|(hand, object, source)| (hand, object, ItemKind::Grenade, source));
    let charge =
        c4::held().map(|(hand, object)| (hand, object, ItemKind::Charge, TakeSource::Pouch));
    for (hand, object, kind, source) in grenade.into_iter().chain(charge) {
        if let Some(object) = ObjectAddress::from_address(object) {
            hands[hand.index()] = Some(live::HeldOther {
                object,
                kind,
                source,
            });
        }
    }
    let grenades = grenade::take_thrown();
    let charges = c4::take_thrown();
    live::HeldOthers {
        hands,
        thrown: Hand::BOTH.map(|h| grenades[h.index()] || charges[h.index()]),
    }
}

/// The weapons in `owner`'s tracked slots, as the live table reads them.
///
/// # Safety
///
/// `owner` must be the live local player character.
unsafe fn live_slots(owner: &Character) -> Vec<SlotObject> {
    unsafe { engine::slots(owner) }
        .into_iter()
        .filter_map(|(seen, _)| live_slot(seen.object))
        .collect()
}

/// The slot weapon at `object`, which its slot keeps alive, as the live table reads it.
fn live_slot(object: ObjectKey) -> Option<SlotObject> {
    // SAFETY: the slot keeps the weapon alive on the input tick.
    let weapon = unsafe { (object as *const WeaponBase).as_ref() }?;
    Some(SlotObject {
        object: engine::object_address(weapon)?,
        weapon: unsafe { engine::weapon_type(weapon) }?,
    })
}

/// The spawn system is resetting: release every weapon the inventory keeps and forget them all,
/// since the reset removes the weapons in the world and replaces the slots' weapons.
fn on_reset() {
    let released: Vec<ObjectAddress> = carry::objects()
        .into_iter()
        .filter_map(ObjectAddress::from_address)
        .collect();
    live::reset_began(&released);
    gives::on_reset();
    let slot_single = single::slot_single();
    restore::on_pre_reset(carry::records(), slot_single, carry::stashed());
    single::on_reset(Some(slot_single));
    let mut state = STATE.lock();
    state.status.resets += 1;
    state.release_all("the spawn system reset");
    drop(state);
    let carried = carry::release_all();
    if carried > 0 {
        tracing::info!(target: "controllers", carried, "inventory: released the carried weapons");
    }
}

fn count_carried_replaced() {
    STATE.lock().status.carried_replaced += 1;
}

struct State {
    world: World,
    status: InventoryStatus,
}

impl State {
    fn release_all(&mut self, why: &str) {
        if self.status.active {
            tracing::info!(target: "controllers", why, "inventory: released every weapon");
        }
        self.world.clear();
        self.status.active = false;
        self.status.owned = 0;
        self.status.world.clear();
        self.status.carried.clear();
        self.status.with_others = 0;
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    world: World::new(),
    status: InventoryStatus {
        active: false,
        owned: 0,
        world: Vec::new(),
        carried: Vec::new(),
        with_others: 0,
        pins: 0,
        forgotten: 0,
        resets: 0,
        stashed: false,
        carried_replaced: 0,
    },
});
