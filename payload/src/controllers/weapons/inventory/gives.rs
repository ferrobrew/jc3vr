//! The game's gives, routed into the loadout table. The design is the two-handed inventory's "The
//! takes" (`docs/mod/input/two-handed-inventory.md`), the engine side in
//! `docs/engine/gameplay/weapon-gives-and-restores.md`.
//!
//! Every weapon the game gives the player (a mission's, a challenge's, the graph script's, an
//! event's, a reward, and a rack's copy) takes one chain: [`Inventory::GivePlayerWeapon`] writes the
//! slot's respawn-table entry, and the spawned weapon arrives through [`Inventory::WeaponSpawned`],
//! which equips it when the entry says so (putting the wielded weapon away) and replaces the slot's
//! occupant. An ordinary world weapon never passes through it, and the respawn after a spawn reset
//! is the reset's own.
//!
//! So the give's detour records each give to the local player's inventory while the table runs and
//! no reset is under way, by slot and spawn tag, with whether it is a rack's copy (a give made
//! inside the pickup start of a weapon with unlimited stock). The arrival's detour, for a weapon
//! that matches a record, does not run the hand-over at once: it keeps the weapon and queues it,
//! and the next input tick hands it over on the main thread ([`input_tick`]), with the slot's
//! occupant first taken out of the slot onto the body where the table has it, and the entry's
//! equip turned off. The weapon is then in its slot, not equipped, and the table places it: a hand
//! that asked a rack for a copy takes it, and anything else goes by the table's give rule, an empty
//! hand (the dominant first), else a free socket that fits it, else the player's feet, where it is
//! dropped. The reconcile then wields, arms, or docks it.
//!
//! A give the reset owns (one made before the reset began, as a checkpoint's restore is, or during
//! it) is left to the game and to the reset's completion, as is a give while a mission holds the
//! player's weapons (its give-back is the table's cache restored) and everything while the table
//! does not run or the weapons keep the game's own layout.

use std::cell::Cell;

use detours_macro::detour;
use jc3gi::{
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    equipment::weapon::WeaponBase,
    game::GameObject,
    types::shared_ptr::SharedPtr,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        hands::ik,
        loadout::{GiveSource, Place, live, slot_label},
        weapons::{
            hold::{
                equip,
                rack::{
                    self,
                    paired::{CopyRequest, Member},
                },
                refs::Strong,
            },
            inventory::{self, engine, reconcile},
        },
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GIVE_PLAYER_WEAPON_BINDER)
        .with_static_binder(&WEAPON_SPAWNED_BINDER)
}

/// Run `start`, the local player's pickup start of a weapon with unlimited stock, noting that the
/// give it makes is a rack's copy.
pub(crate) fn within_copy_start(start: impl FnOnce()) {
    COPY_START.with(|copy| copy.set(true));
    start();
    COPY_START.with(|copy| copy.set(false));
}

/// Request the next native rack give for exactly one physical source member.
pub(crate) fn within_member_start(member: Member, start: impl FnOnce()) {
    let previous = MEMBER_START.with(|copy| copy.replace(Some(member)));
    within_copy_start(start);
    MEMBER_START.with(|copy| copy.set(previous));
}

/// Whether a native source request still owns the pistol callback, including a canceled request.
pub(crate) fn member_pending() -> bool {
    let state = STATE.lock();
    state.records.iter().flatten().any(|r| r.member.is_some())
        || state.queued.iter().any(|a| a.member.is_some())
}

/// Forget the gives recorded: the spawn reset owns every give made before it. Called at the reset's
/// pre-reset event.
pub(crate) fn on_reset() {
    rack::paired::clear();
    let mut state = STATE.lock();
    for record in &mut state.records {
        if record.is_some_and(|r| r.member.is_none()) {
            *record = None;
        }
    }
}

/// Hand the queued gives over and place them in the table. Called each input tick while the table
/// runs, before the table's feed, so the feed sees them where the table has them.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(owner: &mut Character) {
    let queued = std::mem::take(&mut STATE.lock().queued);
    for arrival in queued {
        unsafe { hand_over(owner, arrival) };
    }
}

/// Hand the queued gives over to the game as they came, the table not placing them. Called when
/// the table stops.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn stand_down() {
    rack::paired::clear();
    let mut state = STATE.lock();
    for record in &mut state.records {
        if record.is_some_and(|r| r.member.is_none()) {
            *record = None;
        }
    }
    let queued = std::mem::take(&mut state.queued);
    drop(state);
    for arrival in queued {
        if arrival.member.is_some() {
            unsafe { (*engine::weapon_of(&arrival.weapon)).Despawn() };
        } else {
            let mut shared = shared_weapon(arrival.weapon);
            unsafe { call_original(arrival.inventory, &mut shared) };
        }
    }
}

/// The slots of the respawn table.
const SLOTS: usize = 8;

/// A give recorded at the give's call.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Record {
    tag_index: i32,
    source: GiveSource,
    member: Option<CopyRequest>,
}

/// A given weapon that arrived, held until the next input tick.
struct Arrival {
    inventory: *mut Inventory,
    weapon: Strong,
    slot: WeaponSlot,
    source: GiveSource,
    member: Option<CopyRequest>,
}

// SAFETY: the inventory is the local player's, reached only on the input tick while the character
// lives, and the weapon's reference is atomically counted.
unsafe impl Send for Arrival {}

struct State {
    records: [Option<Record>; SLOTS],
    queued: Vec<Arrival>,
}

static STATE: Mutex<State> = Mutex::new(State {
    records: [None; SLOTS],
    queued: Vec::new(),
});

thread_local! {
    /// Whether this thread is inside the local player's pickup start of a weapon with unlimited
    /// stock.
    static COPY_START: Cell<bool> = const { Cell::new(false) };
    static MEMBER_START: Cell<Option<Member>> = const { Cell::new(None) };
}

/// Whether `inventory` is the local player's.
fn is_local(inventory: *const Inventory) -> bool {
    let local = ik::local_character();
    // SAFETY: the detours are given live inventories.
    local != 0 && unsafe { inventory.as_ref() }.is_some_and(|i| i.m_Character as usize == local)
}

/// Whether the table places the gives now: it runs, no reset is under way, no mission holds the
/// player's weapons (whose give-back the table restores from its cache), and the weapons ride the
/// hands that took them.
fn table_places() -> bool {
    Config::lock_query(|c| c.controllers.weapons.hold.either_hand)
        && live::running()
        && !live::resetting()
        && !live::mission_holds()
}

/// The slots whose gives the table places: the ones it tracks.
fn tracked(slot: WeaponSlot) -> bool {
    engine::tracks(slot)
}

/// A strong `boost::shared_ptr<CWeaponBase>` given up from `strong`, for a callee that releases it.
fn shared_weapon(strong: Strong) -> SharedPtr<WeaponBase> {
    let shared = strong.into_raw();
    SharedPtr {
        px: (shared.px as usize).wrapping_sub(WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
            as *mut WeaponBase,
        pn: shared.pn,
    }
}

/// Run the game's hand-over of `weapon`, which releases it.
unsafe fn call_original(inventory: *mut Inventory, weapon: &mut SharedPtr<WeaponBase>) {
    WEAPON_SPAWNED.get().unwrap().call(inventory, weapon);
}

/// Hand `arrival` over to the slot without the equip, then place it in the table and make the game
/// follow.
unsafe fn hand_over(owner: &mut Character, arrival: Arrival) {
    let Arrival {
        inventory,
        weapon,
        slot,
        source,
        member,
    } = arrival;
    if let Some(request) = member {
        unsafe { stage_member(owner, inventory, weapon, slot, request) };
        return;
    }
    let object = engine::weapon_of(&weapon) as usize;
    if inventory != &raw mut owner.m_Inventory {
        let mut shared = shared_weapon(weapon);
        unsafe { call_original(inventory, &mut shared) };
        return;
    }
    let occupant = owner.m_Inventory.m_Weapons[slot as usize].px as usize;
    if occupant != 0 && occupant != object {
        // The give's add would drop or despawn it; it stays where the table has it.
        unsafe { reconcile::vacate(owner, slot) };
    }
    let equipped = owner.m_Inventory.m_WeaponToSpawnEquippedWeapon;
    // The arrival equips the entry's equipped slot, and with none the selected slot; any other
    // slot named there equips nothing.
    if equipped == slot || equipped == WeaponSlot::E_NONE {
        owner.m_Inventory.m_WeaponToSpawnEquippedWeapon = WeaponSlot::E_NUM_WEAPON_SLOTS;
    }
    let mut shared = shared_weapon(weapon);
    unsafe { call_original(inventory, &mut shared) };
    // The arrival of the equipped slot's weapon clears the equipped slot.
    owner.m_Inventory.m_WeaponToSpawnEquippedWeapon = if equipped == slot {
        WeaponSlot::E_NONE
    } else {
        equipped
    };
    if owner.m_Inventory.m_Weapons[slot as usize].px as usize != object {
        tracing::info!(target: "controllers", slot = slot_label(slot), "inventory: the game's give did not arrive in its slot");
        return;
    }
    // SAFETY: the slot keeps the weapon alive.
    let Some(held) = (unsafe { inventory::held_of(object) }) else {
        return;
    };
    let Some((place, copy)) = live::given(held, source) else {
        return;
    };
    if matches!(place, Place::World(_)) {
        unsafe { equip::drop_occupant(owner, slot) };
    }
    unsafe { reconcile::apply(owner) };
    if let Some(hand) = copy {
        rack::arrived(object, hand);
    }
    tracing::info!(
        target: "controllers",
        slot = slot_label(slot),
        source = source.label(),
        %place,
        "inventory: the game's give went to {place}"
    );
}

/// Initialize a source member through the game's arrival without adopting it into the loadout.
unsafe fn stage_member(
    owner: &mut Character,
    inventory: *mut Inventory,
    weapon: Strong,
    slot: WeaponSlot,
    request: CopyRequest,
) {
    let object = engine::weapon_of(&weapon) as usize;
    if inventory != &raw mut owner.m_Inventory || !rack::paired::waiting(request.member) {
        unsafe { (*engine::weapon_of(&weapon)).Despawn() };
        return;
    }
    let occupant = owner.m_Inventory.m_Weapons[slot as usize].px as usize;
    if occupant != 0 && occupant != object && !unsafe { reconcile::vacate(owner, slot) } {
        unsafe { (*engine::weapon_of(&weapon)).Despawn() };
        tracing::warn!(target: "controllers", "rack: cannot preserve the slot occupant while staging a source member");
        return;
    }
    // Keep a second reference while WeaponSpawned consumes its by-value shared pointer.
    let Some(retained) = (unsafe { engine::strong_to(&*engine::weapon_of(&weapon)) }) else {
        unsafe { reconcile::apply(owner) };
        return;
    };
    let equipped = owner.m_Inventory.m_WeaponToSpawnEquippedWeapon;
    owner.m_Inventory.m_WeaponToSpawnEquippedWeapon = WeaponSlot::E_NUM_WEAPON_SLOTS;
    let mut shared = shared_weapon(weapon);
    unsafe { call_original(inventory, &mut shared) };
    owner.m_Inventory.m_WeaponToSpawnEquippedWeapon = if equipped == slot {
        WeaponSlot::E_NONE
    } else {
        equipped
    };
    let initialized = owner.m_Inventory.m_Weapons[slot as usize].px as usize == object;
    if initialized {
        unsafe { owner.m_Inventory.RemoveWeapon(slot) };
    }
    unsafe { reconcile::apply(owner) };
    if initialized {
        unsafe { rack::paired::staged(request.member, retained) };
    } else {
        unsafe { (*engine::weapon_of(&retained)).Despawn() };
        tracing::warn!(target: "controllers", "rack: the native give did not initialize its source member");
    }
}

#[detour(address = jc3gi::character::inventory::Inventory::GivePlayerWeapon_ADDRESS)]
fn give_player_weapon(
    inventory: *mut Inventory,
    tag_index: i32,
    ammo: i32,
    slot: WeaponSlot,
    start_equipped: bool,
    extra_ammo_to_inventory: bool,
    drop_old: bool,
) {
    let member = MEMBER_START
        .with(Cell::get)
        .map(|member| rack::paired::copy_request(member, ammo));
    if is_local(inventory) && tracked(slot) {
        let record = table_places().then(|| Record {
            tag_index,
            source: if COPY_START.with(Cell::get) {
                GiveSource::RackCopy
            } else {
                GiveSource::Game
            },
            member,
        });
        STATE.lock().records[slot as usize] = record;
    }
    GIVE_PLAYER_WEAPON.get().unwrap().call(
        inventory,
        tag_index,
        member.map_or(ammo, |request| request.ammo),
        slot,
        start_equipped && member.is_none(),
        extra_ammo_to_inventory && member.is_none(),
        drop_old && member.is_none(),
    );
}

#[detour(address = jc3gi::character::inventory::Inventory::WeaponSpawned_ADDRESS)]
fn weapon_spawned(inventory: *mut Inventory, weapon: *mut SharedPtr<WeaponBase>) {
    let queued = is_local(inventory) && unsafe { queue(inventory, weapon) };
    if !queued {
        WEAPON_SPAWNED.get().unwrap().call(inventory, weapon);
    }
}

/// Queue the arriving `weapon` for the next input tick when it is a give the table places, taking
/// over the reference the caller handed the arrival. Returns whether it was queued.
unsafe fn queue(inventory: *mut Inventory, weapon: *mut SharedPtr<WeaponBase>) -> bool {
    let Some(shared) = (unsafe { weapon.as_ref() }) else {
        return false;
    };
    // SAFETY: the arrival is given a live weapon.
    let Some(w) = (unsafe { shared.px.as_ref() }) else {
        return false;
    };
    let Some(slot) = (unsafe { engine::slot_of(w) }).filter(|&slot| tracked(slot)) else {
        return false;
    };
    let tag_index = unsafe {
        (*((w as *const WeaponBase as usize + WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
            as *const GameObject))
            .GetUniqueSpawnTagIndex()
    };
    let mut state = STATE.lock();
    let Some(record) = state.records[slot as usize].filter(|r| r.tag_index == tag_index) else {
        return false;
    };
    state.records[slot as usize] = None;
    if !table_places() && record.member.is_none() {
        return false;
    }
    let object = SharedPtr {
        px: (shared.px as usize + WeaponBase::GAME_OBJECT_BASE_OFFSET as usize) as *mut GameObject,
        pn: shared.pn,
    };
    // SAFETY: the arrival owns the reference it is handed, which now falls to the queue.
    let Some(strong) = (unsafe { Strong::adopt(object) }) else {
        return false;
    };
    state.queued.push(Arrival {
        inventory,
        weapon: strong,
        slot,
        source: record.source,
        member: record.member,
    });
    true
}
