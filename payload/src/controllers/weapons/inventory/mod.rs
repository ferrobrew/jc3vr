//! The VR inventory: every weapon the player owns, holds, or has let go of nearby, as one object
//! that belongs to the world, a hand, or the body. The design is in
//! `docs/mod/input/vr-inventory.md`, the engine side in
//! `docs/engine/gameplay/weapon-ownership-and-despawn.md`.
//!
//! - [`registry`]: the weapons and where each is, mirrored from the game's slots each input tick;
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
pub mod registry;
mod reset;
pub mod restore;
pub mod sockets;

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    frame::Hand,
    weapons::{
        dual_wield::single,
        hold::{
            refs::{Strong, Weak},
            second,
        },
        holster::slot_label,
        inventory::{
            config::InventoryConfig,
            registry::{Entry, Location, ObjectKey, Observation, Registry, Role, Transition},
            sockets::SocketId,
        },
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
    /// Every weapon the registry knows.
    pub entries: Vec<Entry>,
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
    /// The carried weapons: each one's slot and socket.
    pub fn carried(&self) -> impl Iterator<Item = (WeaponSlot, SocketId)> + '_ {
        self.entries.iter().filter_map(|e| match e.location {
            Location::Socket(socket) => Some((e.kind.slot, socket)),
            _ => None,
        })
    }

    /// How many of the player's weapons another character holds.
    pub fn with_others(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.location == Location::Npc)
            .count()
    }

    /// How many weapons in the world are pinned, and how many the registry knows there.
    pub fn world(&self) -> (usize, usize) {
        let world = self
            .entries
            .iter()
            .filter(|e| matches!(e.location, Location::World { .. }));
        let (mut pinned, mut all) = (0, 0);
        for entry in world {
            all += 1;
            if entry.location == (Location::World { pinned: true }) {
                pinned += 1;
            }
        }
        (pinned, all)
    }
}

pub fn status() -> InventoryStatus {
    STATE.lock().status.clone()
}

/// What the weapon handling hands the inventory each input tick.
pub(crate) struct TickInputs {
    pub settings: InventoryConfig,
    /// The dominant hand, whose hip takes a pistol first.
    pub dominant: Hand,
    /// The wielded weapon and the hand holding it.
    pub wielded: Option<(ObjectKey, Option<Hand>)>,
    /// The weapon the mod drives in a hand, and that hand.
    pub driven: Option<(ObjectKey, Hand)>,
    /// Whether the pistols are made separate ([`single`]).
    pub separate_pistols: bool,
}

/// Fold one input tick into the registry and act on the pins.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(owner: &mut Character, inputs: &TickInputs) {
    if inputs.settings.enabled {
        unsafe { restore::input_tick(owner, &inputs.settings, inputs.dominant) };
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
        return;
    }
    if !inputs.settings.carry && carry::count() > 0 {
        second::carry_released();
        unsafe { carry::drop_all() };
    }
    state.status.active = true;
    let player = engine::position(owner);
    let slots = unsafe { engine::slots(owner) };
    unsafe { missions::input_tick(!slots.is_empty()) };
    for (seen, weapon) in &slots {
        if !state.handles.iter().any(|h| h.object == seen.object) {
            state.handles.push(Handle {
                object: seen.object,
                // SAFETY: the slot keeps the weapon alive.
                weak: unsafe { engine::weak_to(weapon) },
                pin: None,
            });
        }
    }
    let seen: Vec<_> = slots.into_iter().map(|(seen, _)| seen).collect();
    let offsets = inputs.settings.offsets;
    // A carried pistol sits in a hip holster the slot's pistol does not take.
    let pistol = seen
        .iter()
        .find(|s| s.kind.slot == WeaponSlot::E_SLOT_DUAL_WIELD)
        .map(|s| s.object);
    let pair = pistol.is_some_and(|object| !single::is(object));
    let home = single::slot_home().filter(|_| pistol.is_some_and(single::is));
    carry::set_offsets(|socket| {
        single::hips::hip_offset(socket, home, pair || !single::active(), offsets.of(socket))
    });
    let carried = unsafe { carry::seen() };
    for (seen, _) in &carried {
        if !state.handles.iter().any(|h| h.object == seen.object) {
            state.handles.push(Handle {
                object: seen.object,
                // SAFETY: the carry set keeps the weapon alive.
                weak: unsafe { engine::weak_to(&*(seen.object as *const WeaponBase)) },
                pin: None,
            });
        }
    }
    let mut hands = [None; 2];
    if let Some((object, Some(hand))) = inputs.wielded {
        hands[hand.index()] = Some((object, Role::Wielded));
    }
    if let Some((object, hand)) = inputs.driven {
        hands[hand.index()] = Some((object, Role::Driven));
    }
    let fates = state
        .registry
        .unplaced(&seen, &carried)
        .into_iter()
        .map(|object| {
            let fate = match state.handles.iter().find(|h| h.object == object) {
                Some(handle) => unsafe { engine::fate(&handle.weak, owner, player) },
                None => registry::Fate::Gone,
            };
            (object, fate)
        })
        .collect();
    let transitions = state.registry.observe(&Observation {
        slots: seen,
        carried,
        hands,
        fates,
    });
    for transition in &transitions {
        log(transition);
    }
    state.apply_pins(&inputs.settings);
    state.prune();
    state.status.entries = state.registry.entries().to_vec();
    state.status.stashed = carry::stashed();
    let tracked: Vec<ObjectKey> = state.registry.entries().iter().map(|e| e.object).collect();
    // The single pistols lying in the world are kept to their first gun too.
    let world_singles: Vec<Strong> = state
        .handles
        .iter()
        .filter(|h| single::is(h.object))
        .filter(|h| {
            state
                .registry
                .find(h.object)
                .is_some_and(|e| matches!(e.location, Location::World { .. }))
        })
        .filter_map(|h| h.weak.lock())
        .collect();
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
}

/// The spawn system is resetting: release every weapon the inventory keeps and forget them all,
/// since the reset removes the weapons in the world and replaces the slots' weapons.
fn on_reset() {
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

/// One weapon the inventory holds references to.
struct Handle {
    object: ObjectKey,
    /// Tells whether the weapon still lives without keeping it alive.
    weak: Weak,
    /// The pin's strong reference, while the weapon lies in the world pinned.
    pin: Option<Strong>,
}

struct State {
    registry: Registry,
    handles: Vec<Handle>,
    status: InventoryStatus,
}

impl State {
    /// Plan the pins and carry them out: a pin takes a strong reference, an unpin releases it, and
    /// a forgotten weapon leaves the registry.
    fn apply_pins(&mut self, settings: &InventoryConfig) {
        let plan = pins::plan(self.registry.entries(), &settings.pins());
        for id in plan.pin {
            let Some(object) = self.registry.get(id).map(|e| e.object) else {
                continue;
            };
            let strong = self
                .handles
                .iter()
                .find(|h| h.object == object)
                .and_then(|h| h.weak.lock());
            let Some(strong) = strong else {
                continue;
            };
            if let Some(handle) = self.handles.iter_mut().find(|h| h.object == object) {
                handle.pin = Some(strong);
            }
            if let Some(transition) = self.registry.set_pinned(id, true) {
                self.status.pins += 1;
                log(&transition);
            }
        }
        for id in plan.unpin {
            if let Some(transition) = self.registry.set_pinned(id, false) {
                log(&transition);
            }
        }
        for id in plan.forget {
            if let Some(transition) = self.registry.remove(id, registry::GoneWhy::Forgotten) {
                self.status.forgotten += 1;
                log(&transition);
            }
        }
    }

    /// Release the pins of weapons no longer pinned and the handles of weapons no longer known.
    fn prune(&mut self) {
        let registry = &self.registry;
        self.handles.retain(|h| registry.find(h.object).is_some());
        for handle in &mut self.handles {
            let pinned = registry
                .find(handle.object)
                .is_some_and(|e| e.location == (Location::World { pinned: true }));
            if !pinned {
                handle.pin = None;
            }
        }
    }

    fn release_all(&mut self, why: &str) {
        let transitions = self.registry.reset();
        if !transitions.is_empty() {
            tracing::info!(target: "controllers", weapons = transitions.len(), why, "inventory: released every weapon");
        }
        self.handles.clear();
        self.status.active = false;
        self.status.entries.clear();
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    registry: Registry::new(),
    handles: Vec::new(),
    status: InventoryStatus {
        active: false,
        entries: Vec::new(),
        pins: 0,
        forgotten: 0,
        resets: 0,
        stashed: false,
        draws: 0,
        stows: 0,
        carried_replaced: 0,
    },
});

/// Log a registry change.
fn log(transition: &Transition) {
    match *transition {
        Transition::Added { id, kind, to } => tracing::info!(
            target: "controllers",
            entry = id.0,
            tag = format_args!("{:#010x}", kind.tag_hash),
            "inventory: tracking a weapon of the {} slot, {}",
            slot_label(kind.slot),
            to.label()
        ),
        Transition::Moved { id, kind, from, to } => tracing::info!(
            target: "controllers",
            entry = id.0,
            tag = format_args!("{:#010x}", kind.tag_hash),
            from = %from.label(),
            "inventory: a weapon of the {} slot is {}",
            slot_label(kind.slot),
            to.label()
        ),
        Transition::Gone {
            id,
            kind,
            from,
            why,
        } => tracing::info!(
            target: "controllers",
            entry = id.0,
            tag = format_args!("{:#010x}", kind.tag_hash),
            from = %from.label(),
            "inventory: no longer tracking a weapon of the {} slot: {}",
            slot_label(kind.slot),
            why.label()
        ),
    }
}
