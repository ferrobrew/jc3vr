//! The VR inventory: every weapon the player owns, holds, or has let go of nearby, as one object
//! that belongs to the world, a hand, or the body. The design is in
//! `docs/mod/input/vr-inventory.md`, the engine side in
//! `docs/engine/gameplay/weapon-ownership-and-despawn.md`.
//!
//! - [`world`]: the game's side of the loadout table's weapons, what became of each the old
//!   handling no longer keeps, and the pins' references;
//! - [`pins`]: which weapons lying in the world are kept from the game's despawn;
//! - [`sockets`]: the places on the body a weapon beyond the slots docks;
//! - [`carry`]: the weapons carried beyond the slots, their placement and visibility;
//! - [`actions`]: drawing a carried weapon (into its slot, or as the second weapon beside the
//!   wielded one), putting a held one onto a socket, and carrying the weapon a pickup replaces;
//! - [`decide`]: what a grab of a socket and the second weapon's release do;
//! - [`persist`] and [`restore`]: the carried weapons across resets, saves, and loads;
//! - [`missions`]: the carried weapons stashed while a mission holds the player's weapons;
//! - [`engine`]: the engine reads behind them;
//! - [`reset`]: the spawn system's reset, which replaces or removes every weapon;
//! - [`config`]: the settings.
//!
//! Each input tick also feeds the live loadout table ([`crate::controllers::loadout::live`]),
//! written beside the old handling and checked against it. The table is the record of every weapon
//! the player has, the ones lying in the world included, which the pins read.
//!
//! A weapon is kept alive and out of the game's despawn by a strong reference: an inventory slot
//! is one, and the inventory takes its own for a weapon it pins. Everything here runs on the input
//! tick on the main thread outside the world update, except the reset's detour, which takes the
//! state's lock only for the local player's inventory.

pub(crate) mod actions;
pub(crate) mod carry;
pub mod config;
pub mod decide;
pub(crate) mod engine;
mod missions;
pub mod persist;
pub mod pins;
mod reset;
pub mod restore;
pub mod sockets;
mod world;

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    frame::Hand,
    loadout::{ObjectAddress, Place, SlotObject, Socket, live},
    weapons::{
        dual_wield::single,
        hold::{refs::Strong, second},
        inventory::{config::InventoryConfig, engine::ObjectKey, pins::WorldWeapon, world::World},
    },
};

/// The inventory's hooks, aggregated by [`crate::controllers::weapons::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(reset::hook_library())
        .with_hook_library(carry::hook_library())
        .with_hook_library(missions::hook_library())
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
    /// Carried weapons drawn into a hand, held weapons put onto a socket, and weapons a pickup
    /// replaced that went onto a socket rather than the ground, since the payload loaded.
    pub draws: u64,
    pub stows: u64,
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
    /// The weapon the mod drives in a hand, and that hand.
    pub driven: Option<(ObjectKey, Hand)>,
    /// Whether the pistols are made separate ([`single`]).
    pub separate_pistols: bool,
}

/// Fold one input tick of the old handling into the loadout table, and act on the pins. Returns
/// the wielded weapon's arrival in a hand, when it came into one this tick.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(
    owner: &mut Character,
    inputs: &TickInputs,
) -> Option<live::Arrival> {
    if inputs.settings.enabled {
        let slots = unsafe { live_slots(owner) };
        live::begin_tick(inputs.dominant, &slots);
        unsafe { restore::input_tick(owner, &inputs.settings, inputs.dominant) };
    } else {
        live::stop();
    }
    let mut state = STATE.lock();
    if !inputs.settings.enabled {
        if state.status.active {
            state.release_all("the inventory is off");
            drop(state);
            second::carry_released();
            unsafe { carry::drop_all() };
            restore::clear();
            single::clear();
        }
        return None;
    }
    if !inputs.settings.carry && carry::count() > 0 {
        second::carry_released();
        unsafe { carry::drop_all() };
    }
    state.status.active = true;
    let player = engine::position(owner);
    let slots = unsafe { engine::slots(owner) };
    unsafe { missions::input_tick(!slots.is_empty()) };
    for (_, weapon) in &slots {
        // SAFETY: the slot keeps the weapon alive.
        unsafe { state.world.watch(weapon) };
    }
    let offsets = inputs.settings.offsets;
    // A carried pistol sits in a hip holster the slot's pistol does not take.
    let pistol = slots
        .iter()
        .find(|(s, _)| s.kind.slot == WeaponSlot::E_SLOT_DUAL_WIELD)
        .map(|(s, _)| s.object);
    let pair = pistol.is_some_and(|object| !single::is(object));
    let home = single::slot_home().filter(|_| pistol.is_some_and(single::is));
    carry::set_offsets(|socket| {
        single::hips::hip_offset(socket, home, pair || !single::active(), offsets.of(socket))
    });
    let carried = unsafe { carry::seen() };
    for (seen, _) in &carried {
        // SAFETY: the carry set keeps the weapon alive.
        unsafe { state.world.watch(&*(seen.object as *const WeaponBase)) };
    }
    // SAFETY: the hands' weapons were read live this tick.
    let wielded = inputs.wielded.and_then(|object| unsafe { held_of(object) });
    let second = inputs
        .driven
        .and_then(|(object, hand)| Some((hand, unsafe { held_of(object) }?)));
    for held in wielded.iter().chain(second.iter().map(|(_, held)| held)) {
        // SAFETY: as above; a heavy weapon is in no tracked slot, and is watched here.
        unsafe {
            state
                .world
                .watch(&*(held.object.get() as *const WeaponBase))
        };
    }
    let live_slots: Vec<SlotObject> = slots
        .iter()
        .filter_map(|(s, _)| live_slot(s.object))
        .collect();
    let live_carried: Vec<(ObjectAddress, u32, Socket)> = carried
        .iter()
        .filter_map(|(s, socket)| {
            Some((
                ObjectAddress::from_address(s.object)?,
                s.kind.tag_hash,
                *socket,
            ))
        })
        .collect();
    let old = old_places(&live_slots, &live_carried, wielded, second);
    // The table's weapons the old handling keeps nowhere, and the ones another character took,
    // are asked what became of them.
    let unplaced: Vec<ObjectAddress> = live::bound()
        .into_iter()
        .map(|(object, _)| object)
        .filter(|object| !old.iter().any(|(o, _)| o == object))
        .chain(state.world.with_others())
        .collect();
    let fates = unsafe { state.world.fates(&unplaced, owner, player) };
    let arrival = live::input_tick(&live::Tick {
        slots: &live_slots,
        carried: &live_carried,
        wielded,
        second,
        fates: &fates,
        old: &old,
    });
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
    arrival
}

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
        weapon: unsafe { engine::weapon_type(weapon) }.filter(|w| {
            engine::TRACKED_SLOTS.contains(&w.slot()) || w.slot() == WeaponSlot::E_SLOT_HEAVY
        })?,
    })
}

/// Where the old handling keeps each weapon: wielded in the hands, the second weapon in its hand,
/// carried on its socket (or stashed by a mission), or holstered in its slot.
fn old_places(
    slots: &[SlotObject],
    carried: &[(ObjectAddress, u32, Socket)],
    wielded: Option<live::Held>,
    second: Option<(Hand, live::Held)>,
) -> Vec<(ObjectAddress, live::OldPlace)> {
    let in_hands = |object: ObjectAddress| {
        if wielded.is_some_and(|w| w.object == object) {
            return Some(live::OldPlace::Wielded);
        }
        second
            .filter(|(_, held)| held.object == object)
            .map(|(hand, _)| live::OldPlace::Hand(hand))
    };
    let stashed = carry::stashed();
    let slot_places = slots.iter().map(|slot| {
        let place = in_hands(slot.object).unwrap_or(live::OldPlace::Slot(slot.weapon.slot()));
        (slot.object, place)
    });
    let carried_places = carried.iter().map(|&(object, _, socket)| {
        let place = match in_hands(object) {
            Some(place) => place,
            // A mission's stash keeps the carried weapons, which the mission took.
            None if stashed => live::OldPlace::Elsewhere,
            None => live::OldPlace::Socket(socket),
        };
        (object, place)
    });
    let mut places: Vec<_> = slot_places.chain(carried_places).collect();
    // A heavy weapon is in no tracked slot; while wielded, it is in the hands.
    if let Some(w) = wielded
        && !places.iter().any(|(object, _)| *object == w.object)
    {
        places.push((w.object, live::OldPlace::Wielded));
    }
    places
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
fn on_reset(died: bool) {
    let released: Vec<ObjectAddress> = carry::objects()
        .into_iter()
        .filter_map(ObjectAddress::from_address)
        .collect();
    live::reset_began(died, &released);
    let slot_single = single::slot_single();
    restore::on_pre_reset(carry::records(), slot_single);
    single::on_reset(Some(slot_single));
    let mut state = STATE.lock();
    state.status.resets += 1;
    state.release_all("the spawn system reset");
    drop(state);
    second::carry_released();
    let carried = carry::release_all();
    if carried > 0 {
        tracing::info!(target: "controllers", carried, "inventory: released the carried weapons");
    }
}

fn count_draw() {
    STATE.lock().status.draws += 1;
}

fn count_stow() {
    STATE.lock().status.stows += 1;
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
        draws: 0,
        stows: 0,
        carried_replaced: 0,
    },
});
