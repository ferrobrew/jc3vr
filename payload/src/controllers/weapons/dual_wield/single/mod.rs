//! Single pistols: each pistol of the dual-wield slot as a weapon of its own. The design is in
//! `docs/mod/input/physical-weapons.md`, "Separate pistols"; the engine side is in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`, "The dual-wield weapon".
//!
//! The game builds every dual-wield weapon as one object with two guns, and wields, holsters,
//! drops, and reloads the two together. It also has a way of holding one: a dual-wield weapon
//! wielded rather than dual-wielded shows and fires its first gun alone
//! ([`WeaponBase::SetVisibleBase`], [`WeaponBase::m_NextWeaponToFire`]). A single pistol is such
//! an object kept to its first gun whatever its state:
//!
//! - its guns past the first are hidden, with their model parts, after every show the game makes
//!   ([`hide_extra`], from the detour of the visibility's base, and each input tick for the
//!   pistols the player owns), and its next gun is kept on the first ([`pin`]);
//! - everything of the mod's that walks a weapon's guns sees one gun
//!   ([`crate::controllers::aim::gun::guns`]), so the hold places it in the holding hand as it
//!   places any weapon of one gun, and the dual-wield split leaves it alone;
//! - its magazine holds half the tuned size ([`magazine`]), with the reserve shared.
//!
//! The player's pistols become two such objects. A pair the game hands over keeps the slot, and
//! a second pistol of its kind, the twin, is spawned and carried on the other hip by the VR
//! inventory's carry set, half the pair's magazine in each ([`grant`]). A pistol taken from the
//! world is one pistol ([`pickup_item`]). Each is then picked up, held, dropped, and reloaded on
//! its own, through the paths every weapon takes: the slot's is the wielded weapon, a carried one
//! is held beside it as the second weapon, and a drop is a drop of that object. Which hip each
//! sits on is [`hips`].
//!
//! The set of single pistols ([`set`]) is the VR inventory's to keep: an object leaves it at the
//! first input tick after the inventory stops tracking it, and while the hands stand down only
//! the pistols the slot and the carry set keep alive stay in it, so a dead object's address is
//! taken for a single pistol at most until that tick. A twin that is not wanted any more is put
//! away out of reach ([`park`]), never left in the world.
//! The settings switch only the making of new ones; pistols already single stay single.

pub mod grant;
pub mod hips;
pub mod magazine;
mod set;

use std::{
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    time::Instant,
};

use detours_macro::detour;
use jc3gi::{
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    equipment::weapon::{AmmunitionType, ModelAttachementWeaponComponent, WeaponBase, WeaponState},
    game::GameObject,
    graphics_engine::model_instance::ModelInstanceFlags,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        hands::ik,
        loadout::{Restored, Socket, live},
        weapons::{
            dual_wield::{
                attach,
                single::{
                    grant::{Grant, GrantTick, Grants, OneWhy, TwinState},
                    hips::{hip_socket, home_for, promote_sides, socket_side},
                    magazine::Dealt,
                },
            },
            engine::slot_from_raw,
            hold::{self, layout, refs::Strong, second},
            inventory::{
                carry,
                config::InventoryConfig,
                engine as inventory_engine,
                registry::ObjectKey,
                restore,
                sockets::{Fit, Layout, fit_of},
            },
        },
    },
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SET_VISIBLE_BASE_BINDER)
        .with_static_binder(&PICKUP_ITEM_BINDER)
        .with_static_binder(&PICKUP_START_BINDER)
}

/// The single pistols' readout.
#[derive(Debug, Clone, PartialEq)]
pub struct SingleStatus {
    /// Whether new pistols are made single.
    pub active: bool,
    /// How many single pistols the player owns or let go of nearby.
    pub singles: usize,
    /// Whether the slot's weapon is a single pistol, and its hip.
    pub slot_single: bool,
    pub slot_home: Option<Hand>,
    /// Where the slot's pair is in getting its twin.
    pub twin: TwinState,
    /// Pairs split in two, pistols that arrived as one, pistols taken from the world as one, and
    /// pairs left as the game's, since the payload loaded.
    pub splits: u64,
    pub ones: u64,
    pub picked: u64,
    pub pairs: u64,
    /// The last thing that happened, for the readout.
    pub last: Option<String>,
}

pub fn status() -> SingleStatus {
    let state = STATE.lock();
    SingleStatus {
        active: ACTIVE.load(Ordering::Relaxed),
        singles: set::len(),
        slot_single: state.slot_single,
        slot_home: slot_home(),
        twin: state.grants.state(),
        splits: state.splits,
        ones: state.ones,
        picked: PICKED.load(Ordering::Relaxed),
        pairs: state.pairs,
        last: state.last.clone(),
    }
}

/// Whether the weapon at `object` is a single pistol.
pub(crate) fn is(object: usize) -> bool {
    set::contains(object)
}

/// Whether new pistols are made single: the setting is on, and the VR inventory tracks and carries
/// weapons.
pub(crate) fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Make `weapon` a single pistol: its guns past the first hidden from now on. Returns whether it
/// became one; a weapon outside the dual-wield slot, one of a single gun, one already single, or
/// one the table has no room for does not.
///
/// # Safety
///
/// `weapon` must be live, on the main thread outside the world update or on a thread updating
/// its holder.
pub(crate) unsafe fn designate(weapon: &mut WeaponBase, why: &str) -> bool {
    // A weapon of one gun is one weapon already.
    if unsafe { slot_of(weapon) } != Some(WeaponSlot::E_SLOT_DUAL_WIELD)
        || unsafe { gun_count(weapon) } < 2
    {
        return false;
    }
    let object = weapon as *mut WeaponBase as usize;
    match set::insert(object) {
        Ok(true) => {
            unsafe { hide_extra(weapon) };
            pin(weapon);
            tracing::info!(target: "controllers", why, "single pistols: a pistol is one pistol");
            true
        }
        Ok(false) => false,
        Err(full) => {
            tracing::warn!(target: "controllers", %full, "single pistols: the pistol stays the game's pair");
            false
        }
    }
}

/// Keep `weapon`'s next gun on its first, when it is a single pistol, so that its update fires the
/// gun that is shown.
pub(crate) fn pin(weapon: &mut WeaponBase) {
    if set::contains(weapon as *mut WeaponBase as usize) {
        weapon.m_NextWeaponToFire = 0;
    }
}

/// The magazine size of `weapon` as the hands treat it: half the tuned size for a single pistol.
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn magazine_size(weapon: &WeaponBase) -> i32 {
    let tuned = unsafe { weapon.GetMagazineSize() };
    if set::contains(weapon as *const WeaponBase as usize) {
        magazine::capacity(tuned)
    } else {
        tuned
    }
}

/// Bring a single pistol's magazine down to its half, the rounds over it back to `owner`'s
/// reserve. Does nothing for another weapon.
///
/// # Safety
///
/// `owner` must be the live local player character and `weapon` live, on the main thread outside
/// the world update.
pub(crate) unsafe fn clamp_magazine(owner: &mut Character, weapon: &mut WeaponBase) {
    if !set::contains(weapon as *mut WeaponBase as usize) {
        return;
    }
    let tuned = unsafe { weapon.GetMagazineSize() };
    let unlimited = unsafe { weapon.HasUnlimitedAmmo() };
    let Some(clamped) = magazine::clamp(weapon.m_MagazineAmmo, tuned, unlimited) else {
        return;
    };
    unsafe { weapon.SetMagazineAmmo(clamped.magazine) };
    unsafe { return_rounds(owner, weapon, clamped.returned) };
    tracing::debug!(
        target: "controllers",
        magazine = clamped.magazine,
        returned = clamped.returned,
        "single pistols: brought a pistol's magazine down to its half"
    );
}

/// The slot's single pistol's hip, while the slot holds one.
pub(crate) fn slot_home() -> Option<Hand> {
    home().map(|(_, side)| side)
}

/// Write the slot's single pistol's hip into its holstered attach bone, when `inventory` is the
/// local player's and holds it. Called at the hold's write points, which the game's own rewrite
/// of the table at the end of each pass would otherwise undo.
///
/// # Safety
///
/// `inventory` must be null or live, on the thread updating its owner.
pub(crate) unsafe fn apply_home(inventory: *mut Inventory) {
    let Some((object, side)) = home() else {
        return;
    };
    let Some(inventory) = (unsafe { inventory.as_ref() }) else {
        return;
    };
    let local = ik::local_character();
    if local == 0 || inventory.m_Character as usize != local {
        return;
    }
    let in_slot = inventory
        .m_Weapons
        .get(WeaponSlot::E_SLOT_DUAL_WIELD as usize)
        .is_some_and(|slot| slot.px as usize == object);
    if !in_slot {
        return;
    }
    let bone = layout::hip_bone(side);
    // SAFETY: the slot keeps the weapon alive.
    let weapon = unsafe { &mut *(object as *mut WeaponBase) };
    if let Some(first) = unsafe { gun_mut(weapon, 0) }
        && let Some(entry) = first
            .m_lBoneAttachementPerWeaponState
            .get_mut(WeaponState::E_WS_HOLSTERED as usize)
    {
        *entry = bone;
    }
}

/// The carried single pistol `promoted`, on `from`, moves into the slot, whose occupant comes out:
/// returns the socket the occupant goes onto, its own hip's when that is free, and makes the
/// newcomer's hip the slot's home. `None` when either is not a single pistol on a hip, for the
/// carry set's own choice.
pub(crate) fn promote_socket(
    promoted: ObjectKey,
    from: Socket,
    occupant: ObjectKey,
    free: impl Fn(Socket) -> bool,
) -> Option<Socket> {
    if !(set::contains(promoted) && set::contains(occupant)) {
        return None;
    }
    let promoted_side = socket_side(from)?;
    let home = home()
        .filter(|&(object, _)| object == occupant)
        .map(|(_, side)| side)?;
    let (occupant_side, new_home) = promote_sides(promoted_side, home, free(hip_socket(home)));
    set_home(promoted, new_home);
    Some(hip_socket(occupant_side))
}

/// The carried single pistol `object`, on `from`, moved into an empty slot: its hip is the slot's
/// home from now on.
pub(crate) fn entered_slot(object: ObjectKey, from: Socket) {
    if set::contains(object)
        && let Some(side) = socket_side(from)
    {
        set_home(object, side);
    }
}

/// Make `side` the hip of the slot's single pistol at `object`: called before a holster of it
/// there, so the holster's slide already ends on that hip.
///
/// # Safety
///
/// `object` must be the live weapon in the local player's pistol slot, on the main thread outside
/// the world update.
pub(crate) unsafe fn rehome(object: ObjectKey, side: Hand) {
    if set::contains(object) {
        set_home(object, side);
        // SAFETY: the caller's weapon is live.
        let weapon = unsafe { &mut *(object as *mut WeaponBase) };
        if let Some(first) = unsafe { gun_mut(weapon, 0) }
            && let Some(entry) = first
                .m_lBoneAttachementPerWeaponState
                .get_mut(WeaponState::E_WS_HOLSTERED as usize)
        {
            *entry = layout::hip_bone(side);
        }
        tracing::info!(
            target: "controllers",
            side = side.label(),
            "single pistols: the slot's pistol now sits on the {} hip",
            side.label()
        );
    }
}

/// The single pistol `weapon` was taken from the world by the mod without the game's pickup: its
/// stored rounds go to `owner`'s reserve and are cleared, as the game's pickup does, and it counts
/// as taken.
///
/// # Safety
///
/// `owner` must be the live local player character and `weapon` live, on the main thread outside
/// the world update.
pub(crate) unsafe fn taken_from_world(owner: &mut Character, weapon: &mut WeaponBase) {
    let stored = weapon.m_StoredNonMagazineAmmo;
    weapon.m_StoredNonMagazineAmmo = 0;
    unsafe { return_rounds(owner, weapon, stored) };
    PICKED.fetch_add(1, Ordering::Relaxed);
}

/// A rack's copy for one pistol arrived as `weapon`: it is one pistol.
///
/// # Safety
///
/// As [`designate`].
pub(crate) unsafe fn rack_copy_arrived(weapon: &mut WeaponBase) {
    if active() {
        unsafe { designate(weapon, "a rack's copy") };
    }
}

/// A reset is under way: the slot's pistol comes back as one pistol, a pair, or with nothing
/// known (`None`). Every single pistol is let go of, since the reset releases every object.
pub(crate) fn on_reset(slot_single: Option<bool>) {
    for object in set::all() {
        set::remove(object);
    }
    clear_home();
    std::mem::take(&mut *PARKED.lock());
    let mut state = STATE.lock();
    state.grants.clear();
    state.grants.cover_respawn(slot_single);
    state.slot_single = false;
    state.unrestored.clear();
}

/// A carried single pistol of the type `tag_hash` never came back after a reset: the slot's single
/// pistol of that type, when there is one, becomes the game's pair again, so the reset loses no
/// pistol.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn pistol_not_restored(owner: &mut Character, tag_hash: u32) {
    let reverted = unsafe { slot_weapon(owner) }.filter(|weapon| {
        let object = *weapon as *const WeaponBase as usize;
        let same_kind =
            unsafe { inventory_engine::kind_of(weapon) }.is_some_and(|k| k.tag_hash == tag_hash);
        same_kind && set::remove(object)
    });
    let Some(weapon) = reverted else {
        // The slot's pistol has not come back yet: when it does, it stays the pair.
        STATE.lock().unrestored.push(tag_hash);
        return;
    };
    clear_home();
    // The game shows both guns again as it would any pair it holds.
    let shown = unsafe { gun_mut(weapon, 0) }
        .and_then(|gun| unsafe { gun.m_BaseModelInstance.as_ref() })
        .is_some_and(|model| model.m_Flags.contains(ModelInstanceFlags::VISIBLE));
    unsafe { weapon.SetVisible(shown, false, false) };
    note(
        |s| s.pairs += 1,
        "the second pistol never came back; the slot's pistol is the pair again".to_owned(),
    );
    tracing::warn!(target: "controllers", "single pistols: the second pistol never came back after the reset; the slot's pistol is the game's pair again");
}

/// The respawn after a reset chose its record: the slot's pistol comes back as the record says.
pub(crate) fn cover_respawn(slot_single: Option<bool>) {
    STATE.lock().grants.cover_respawn(slot_single);
}

/// A mission took the slots' weapons: whether the pistol slot held one pistol, for its give-back.
pub(crate) fn cover_mission(slot_single: bool) {
    STATE.lock().grants.cover_mission(slot_single);
}

/// Whether the slot's weapon is a single pistol, as of the last input tick.
pub(crate) fn slot_single() -> bool {
    STATE.lock().slot_single
}

/// Whether the hips hold single pistols, as of the last input tick: the slot's weapon is one, or
/// the slot is empty while pistols are made single, so each hip is the hip of the pistol on it.
pub(crate) fn hips_single() -> bool {
    let state = STATE.lock();
    state.slot_single || (!state.slot_filled && active())
}

/// What the VR inventory hands the single pistols each input tick.
pub(crate) struct Tick<'a> {
    /// Whether the pistols are made separate (the setting).
    pub separate: bool,
    pub settings: &'a InventoryConfig,
    pub dominant: Hand,
    /// Every weapon the inventory tracks.
    pub tracked: &'a [ObjectKey],
}

/// Follow the player's pistols for one input tick: forget single pistols the inventory no longer
/// tracks, keep the owned ones to one gun and half a magazine, split a pair the game handed over,
/// take a twin that came into the carry set, and put a carried pistol into an empty slot.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(owner: &mut Character, tick: &Tick) {
    let enabled = tick.separate && tick.settings.enabled && tick.settings.carry;
    ACTIVE.store(enabled, Ordering::Relaxed);
    for object in set::all() {
        if !tick.tracked.contains(&object) {
            set::remove(object);
        }
    }
    let slot = unsafe { slot_weapon(owner) };
    let slot_object = slot.as_ref().map(|w| *w as *const WeaponBase as usize);
    let slot_is_single = slot_object.is_some_and(set::contains);
    unsafe { keep_owned(owner, slot_object) };
    unsafe { follow_home(slot_object.filter(|_| slot_is_single)) };
    if RACK_COPY_ASKED.swap(false, Ordering::Relaxed) {
        STATE.lock().grants.cover_rack_copy();
    }
    let layout = tick.settings.layout(tick.dominant);
    let fit = slot.as_ref().and_then(|w| unsafe { fit_of_weapon(w) });
    let guns = slot.as_ref().is_some_and(|w| unsafe { gun_count(w) } >= 2);
    let twin_socket = fit.and_then(|fit| twin_socket(&layout, fit));
    let (grant, dt) = {
        let mut state = STATE.lock();
        let now = Instant::now();
        let dt = state
            .last_tick
            .map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
        state.last_tick = Some(now);
        state.slot_single = slot_is_single;
        state.slot_filled = slot_object.is_some();
        let grant = state.grants.observe(GrantTick {
            enabled,
            slot: slot_object,
            single: slot_is_single,
            guns,
            stashed: carry::stashed(),
            socket_free: twin_socket.is_some(),
            dt,
        });
        (grant, dt)
    };
    unsafe { act_on(owner, grant) };
    for strong in restore::take_twins() {
        unsafe { take_twin(owner, strong, tick) };
    }
    if enabled {
        unsafe { refill(owner, dt) };
    }
}

/// Forget every single pistol and the slot's history: the inventory is off.
pub(crate) fn clear() {
    ACTIVE.store(false, Ordering::Relaxed);
    for object in set::all() {
        set::remove(object);
    }
    clear_home();
    std::mem::take(&mut *PARKED.lock());
    STATE.lock().grants.clear();
}

/// Stop making pistols single while the weapon handling stands down, and keep only the single
/// pistols something keeps alive for the inventory's return: the slot's and the carried ones. A
/// pistol lying in the world may be destroyed meanwhile, and its address taken by another object,
/// which must not be mistaken for a single pistol.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn stand_down() {
    ACTIVE.store(false, Ordering::Relaxed);
    if set::len() == 0 {
        return;
    }
    let slot = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .and_then(|owner| unsafe { slot_weapon(owner) })
        .map(|w| w as *mut WeaponBase as usize);
    for object in set::all() {
        if Some(object) != slot && !carry::contains(object) {
            set::remove(object);
        }
    }
}

/// Hide `weapon`'s guns past the first and their model parts, when they are shown.
///
/// # Safety
///
/// `weapon` must be live, on the main thread outside the world update or on a thread updating its
/// holder.
pub(crate) unsafe fn hide_extra(weapon: &mut WeaponBase) {
    let count = unsafe { gun_count(weapon) };
    let mut hid = false;
    for index in 1..count {
        let Some(gun) = (unsafe { gun_mut(weapon, index) }) else {
            continue;
        };
        let Some(model) = (unsafe { gun.m_BaseModelInstance.as_mut() }) else {
            continue;
        };
        if model.m_Flags.contains(ModelInstanceFlags::VISIBLE) {
            model
                .m_Flags
                .remove(ModelInstanceFlags::VISIBLE | ModelInstanceFlags::FLAG_ENABLED);
            hid = true;
        }
    }
    if !hid {
        return;
    }
    // A model part shows only while its own gun's model does, so asking it to hide now that the
    // gun is hidden hides it, and a later show of the part keeps it hidden.
    for component in unsafe { weapon.m_WeaponComponents.as_slice() } {
        let Some(component) = (unsafe { component.px.as_mut() }) else {
            continue;
        };
        // SAFETY: the component belongs to the live weapon; the test also takes the barrel recoil
        // and spin parts, which derive from the model part.
        if !unsafe { component.IsType(ModelAttachementWeaponComponent::TYPE_ID()) } {
            continue;
        }
        let part = unsafe { &*(component as *mut _ as *const ModelAttachementWeaponComponent) };
        if part.m_WeaponDataIndex >= 1 {
            unsafe { component.SetVisible(false, false) };
        }
    }
}

struct State {
    grants: Grants,
    /// The types of carried single pistols that never came back after the last reset, for the
    /// slot's pistol that has not come back yet.
    unrestored: Vec<u32>,
    slot_single: bool,
    slot_filled: bool,
    /// How long the slot has been empty, seconds.
    empty_s: f32,
    last_tick: Option<Instant>,
    splits: u64,
    ones: u64,
    pairs: u64,
    last: Option<String>,
}

static STATE: Mutex<State> = Mutex::new(State {
    grants: Grants::new(),
    unrestored: Vec::new(),
    slot_single: false,
    slot_filled: false,
    empty_s: 0.0,
    last_tick: None,
    splits: 0,
    ones: 0,
    pairs: 0,
    last: None,
});
/// How long the pistol slot stays empty before a carried pistol is put into it, seconds: long
/// enough for a weapon the game is putting there to arrive first.
const REFILL_AFTER_S: f32 = 0.5;
/// Whether new pistols are made single, for the detours.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Set by the pickup start's detour for a rack's copy; the input tick covers the copy.
static RACK_COPY_ASKED: AtomicBool = AtomicBool::new(false);
/// The slot's single pistol and its hip, packed into one word so a reader on another thread never
/// pairs one pistol with another's hip: the object's address in the low bits and the hip's code
/// ([`side_code`]) in the top byte, or zero without one.
static HOME: AtomicUsize = AtomicUsize::new(0);
/// Pistols taken from the world as one pistol since the payload loaded.
static PICKED: AtomicU64 = AtomicU64::new(0);
/// Twins that were not wanted, hidden and kept out of reach until the next reset ([`park`]).
static PARKED: Mutex<Vec<Strong>> = Mutex::new(Vec::new());

const SIDE_LEFT: u8 = 1;
const SIDE_RIGHT: u8 = 2;

const fn side_code(side: Hand) -> u8 {
    match side {
        Hand::Left => SIDE_LEFT,
        Hand::Right => SIDE_RIGHT,
    }
}

const fn side_of(code: u8) -> Hand {
    if code == SIDE_LEFT {
        Hand::Left
    } else {
        Hand::Right
    }
}

fn set_home(object: ObjectKey, side: Hand) {
    HOME.store(
        (object & HOME_OBJECT_MASK) | ((side_code(side) as usize) << HOME_SIDE_SHIFT),
        Ordering::Relaxed,
    );
}

fn clear_home() {
    HOME.store(0, Ordering::Relaxed);
}

/// The slot's single pistol and its hip, when there is one.
fn home() -> Option<(ObjectKey, Hand)> {
    let word = HOME.load(Ordering::Relaxed);
    let object = word & HOME_OBJECT_MASK;
    (object != 0).then(|| (object, side_of((word >> HOME_SIDE_SHIFT) as u8)))
}

/// Where the hip's code sits in [`HOME`]: above every user-space address.
const HOME_SIDE_SHIFT: u32 = 56;
const HOME_OBJECT_MASK: usize = (1 << HOME_SIDE_SHIFT) - 1;

/// Keep the pistols the player owns to one gun and half a magazine: the slot's and the carried
/// ones.
unsafe fn keep_owned(owner: &mut Character, slot: Option<usize>) {
    let carried = unsafe { carry::seen() };
    let owned = slot
        .into_iter()
        .chain(carried.iter().map(|(seen, _)| seen.object))
        .filter(|&object| set::contains(object));
    for object in owned.collect::<Vec<_>>() {
        // SAFETY: the slot or the carry set keeps the weapon alive.
        let weapon = unsafe { &mut *(object as *mut WeaponBase) };
        unsafe { hide_extra(weapon) };
        pin(weapon);
        unsafe { clamp_magazine(owner, weapon) };
    }
}

/// Give the slot's single pistol its hip when it is new to the slot: the right, unless a carried
/// pistol sits there and the left is free. A pistol the mod moved into the slot already has one.
unsafe fn follow_home(single: Option<usize>) {
    let Some(object) = single else {
        clear_home();
        return;
    };
    if home().is_some_and(|(o, _)| o == object) {
        return;
    }
    let carried = Hand::BOTH.map(|side| carry::on_socket(hip_socket(side)).is_some());
    set_home(object, home_for(carried));
}

/// Carry out the grant's decision.
unsafe fn act_on(owner: &mut Character, grant: Grant) {
    match grant {
        Grant::Nothing => {}
        Grant::One { object, why } => {
            // SAFETY: the decision is about the slot's weapon this tick, which the slot keeps
            // alive.
            let weapon = unsafe { &mut *(object as *mut WeaponBase) };
            // A respawned pistol whose carried half never came back stays the pair it was.
            let tag = unsafe { inventory_engine::kind_of(weapon) }.map(|k| k.tag_hash);
            let unrestored = why == OneWhy::Respawn && {
                let mut state = STATE.lock();
                let index = tag.and_then(|t| state.unrestored.iter().position(|&u| u == t));
                index.map(|i| state.unrestored.remove(i)).is_some()
            };
            if unrestored {
                note(
                    |s| s.pairs += 1,
                    "the second pistol never came back; the slot's pistol is the pair again"
                        .to_owned(),
                );
                tracing::warn!(target: "controllers", "single pistols: the second pistol never came back after the reset; the slot's pistol stays the game's pair");
            } else if unsafe { designate(weapon, why.label()) } {
                note(
                    |s| s.ones += 1,
                    format!("the slot's pistol is one pistol: {}", why.label()),
                );
            }
        }
        Grant::Request { object } => {
            // SAFETY: as above.
            let weapon = unsafe { &*(object as *const WeaponBase) };
            let Some(kind) = (unsafe { inventory_engine::kind_of(weapon) }) else {
                return;
            };
            if unsafe { restore::request_twin(owner, kind.tag_hash) } {
                tracing::info!(
                    target: "controllers",
                    tag = format_args!("{:#010x}", kind.tag_hash),
                    "single pistols: asked for the second pistol of the pair"
                );
            } else {
                STATE.lock().grants.refused(object);
                note(
                    |s| s.pairs += 1,
                    "the second pistol could not be asked for; the pair stays the game's"
                        .to_owned(),
                );
            }
        }
        Grant::Pair { why, .. } => {
            note(
                |s| s.pairs += 1,
                format!("the pair stays the game's: {}", why.label()),
            );
            tracing::info!(target: "controllers", why = why.label(), "single pistols: the pair stays the game's");
        }
        Grant::Cancelled { .. } => {
            tracing::info!(target: "controllers", "single pistols: the pair left the slot before its second pistol came");
        }
    }
}

/// The free socket a twin of `fit` goes onto: the hip the slot's pistol does not take, when it is
/// free, or the first free socket that fits it.
fn twin_socket(layout: &Layout, fit: Fit) -> Option<Socket> {
    let taken = carry::taken();
    let home = slot_home().unwrap_or(Hand::Right);
    let other = hip_socket(home.other());
    if layout.accepts(other, fit) && !taken.contains(&other) {
        return Some(other);
    }
    layout.choose(fit, &taken)
}

/// What fits `weapon` on the body, read as raw values: an unset slot or class is out of its enum's
/// range.
unsafe fn fit_of_weapon(weapon: &WeaponBase) -> Option<Fit> {
    let slot = unsafe { slot_of(weapon) }?;
    let class = unsafe { (&raw const weapon.m_Class).cast::<i32>().read() };
    fit_of(slot, class)
}

/// A twin came: carry it on the hip the slot's pistol does not take, and deal the pair's magazine
/// between the two, or let it go when the pair it was asked for is not in the slot any more.
unsafe fn take_twin(owner: &mut Character, strong: Strong, tick: &Tick) {
    let slot = unsafe { slot_weapon(owner) }.map(|w| w as *mut WeaponBase as usize);
    // SAFETY: the strong reference keeps the twin alive.
    let twin_kind = unsafe { inventory_engine::kind_of(&*inventory_engine::weapon_of(&strong)) };
    // SAFETY: the slot keeps its weapon alive.
    let slot_kind = slot
        .and_then(|object| unsafe { inventory_engine::kind_of(&*(object as *const WeaponBase)) });
    let same_kind = twin_kind.is_some_and(|t| slot_kind.is_some_and(|s| s.tag_hash == t.tag_hash));
    // Both halves need a place in the table, or the pair stays whole.
    let room = set::CAPACITY - set::len() >= 2;
    let wanted = {
        let mut state = STATE.lock();
        if same_kind && room {
            state.grants.arrived(slot, slot.is_some_and(set::contains))
        } else {
            None
        }
    };
    let Some(pair) = wanted else {
        unsafe { park(owner, strong, "it came for a pair no longer in the slot") };
        return;
    };
    let layout = tick.settings.layout(tick.dominant);
    // The pair becomes the slot's pistol first, so its home is known when the socket is chosen.
    // SAFETY: the slot keeps the pair alive.
    let slot_pistol = unsafe { &mut *(pair as *mut WeaponBase) };
    let carried = Hand::BOTH.map(|side| carry::on_socket(hip_socket(side)).is_some());
    set_home(pair, home_for(carried));
    let socket = unsafe { fit_of_weapon(slot_pistol) }.and_then(|fit| twin_socket(&layout, fit));
    let Some(socket) = socket else {
        STATE.lock().grants.refused(pair);
        clear_home();
        note(
            |s| s.pairs += 1,
            "no socket is free for the second pistol".to_owned(),
        );
        unsafe { park(owner, strong, "no socket is free for it") };
        return;
    };
    // A second reference, so a twin the carry set refuses is still the mod's to put away.
    // SAFETY: the strong reference keeps the twin alive.
    let keep = unsafe { inventory_engine::strong_to(&*inventory_engine::weapon_of(&strong)) };
    let offset = tick.settings.offsets.of(socket);
    let twin = match unsafe { carry::carry_loose(owner, strong, socket, offset) } {
        Ok(twin) => twin,
        Err(refusal) => {
            STATE.lock().grants.refused(pair);
            clear_home();
            tracing::warn!(target: "controllers", %refusal, "single pistols: the second pistol was not carried; the pair stays the game's");
            if let Some(keep) = keep {
                unsafe { park(owner, keep, "the carry set refused it") };
            }
            return;
        }
    };
    drop(keep);
    // SAFETY: the carry set keeps the twin alive.
    let twin_pistol = unsafe { &mut *(twin as *mut WeaponBase) };
    let tuned = unsafe { slot_pistol.GetMagazineSize() };
    let dealt = magazine::split(slot_pistol.m_MagazineAmmo, tuned);
    if !(unsafe { designate(slot_pistol, "the slot's half of a pair") }
        && unsafe { designate(twin_pistol, "the second half of a pair") })
    {
        // The table filled meanwhile: put the pair back as it was and the twin away.
        set::remove(pair);
        set::remove(twin);
        STATE.lock().grants.refused(pair);
        clear_home();
        if let Some(strong) = carry::release(twin) {
            unsafe { park(owner, strong, "the pair could not be made two pistols") };
        }
        return;
    }
    unsafe { deal(owner, slot_pistol, twin_pistol, dealt) };
    if let (Some(object), Some(weapon)) = (inventory_engine::object_address(twin_pistol), unsafe {
        inventory_engine::weapon_type(twin_pistol)
    }) {
        live::restored(Restored {
            object,
            weapon,
            socket,
        });
    }
    unsafe { share_hands(twin) };
    let side = socket_side(socket).unwrap_or(Hand::Left);
    note(
        |s| s.splits += 1,
        format!(
            "the pair is two pistols; the second is on the {} hip",
            side.label()
        ),
    );
    tracing::info!(
        target: "controllers",
        socket = socket.label(),
        slot_magazine = dealt.slot,
        twin_magazine = dealt.twin,
        "single pistols: split the pair; the second pistol is on the {}",
        socket.label()
    );
}

/// Put away a twin that is not wanted: the spawn system keeps its own reference to it and would
/// leave it lying at the player's feet, a pistol too many. It is taken into the player's keeping,
/// which turns its pickup off and its bodies out of the world, then hidden, and kept until the
/// next reset releases every object.
unsafe fn park(owner: &mut Character, strong: Strong, why: &str) {
    // SAFETY: the strong reference keeps the weapon alive.
    let weapon = unsafe { &mut *inventory_engine::weapon_of(&strong) };
    let holder = inventory_engine::player_object(owner);
    if weapon.m_Holder.px as usize != holder {
        unsafe { weapon.OnPickup(holder as *mut GameObject) };
    }
    unsafe { weapon.SetState(WeaponState::E_WS_HOLSTERED_INVISIBLE, true) };
    unsafe { weapon.SetVisible(false, false, false) };
    PARKED.lock().push(strong);
    tracing::warn!(target: "controllers", why, "single pistols: a second pistol is not wanted; it is hidden and kept out of reach");
}

/// A pair split while in the hands keeps a pistol in each hand that held one: the slot's pistol
/// stays in its hand, and with both drawn the second pistol comes off its hip into the other hand
/// as the second weapon at once.
unsafe fn share_hands(twin: ObjectKey) {
    let attach = attach::status();
    let Some(gun_hands) = attach.gun_hands.filter(|_| attach.wielded) else {
        return;
    };
    let (slot_hand, twin_hand) = hips::split_hands(attach.drawn, gun_hands);
    if let Some(hand) = slot_hand {
        hold::hand_over(hand);
    }
    let second_allowed = Config::lock_query(|c| {
        let hold = c.controllers.weapons.hold;
        hold.either_hand && hold.second_weapon
    });
    if let Some(hand) = twin_hand.filter(|_| second_allowed)
        && let Err(refusal) = unsafe { second::take_carried(hand, twin) }
    {
        tracing::info!(target: "controllers", %refusal, "single pistols: the second pistol stays on its hip");
    }
}

/// Give each half of a split pair its share of the magazine.
unsafe fn deal(owner: &mut Character, slot: &mut WeaponBase, twin: &mut WeaponBase, dealt: Dealt) {
    unsafe { slot.SetMagazineAmmo(dealt.slot) };
    unsafe { twin.SetMagazineAmmo(dealt.twin) };
    if dealt.over > 0 && !unsafe { slot.HasUnlimitedAmmo() } {
        unsafe { return_rounds(owner, slot, dealt.over) };
    }
}

/// Put a carried single pistol into the pistol slot once it has been empty a moment, unequipped,
/// so the HUD and the weapon wheel show the pistols again. Not while the carried weapons are
/// stashed, nor while a respawn may still fill the slot.
unsafe fn refill(owner: &mut Character, dt: f32) {
    let empty = unsafe { slot_weapon(owner) }.is_none();
    let quiet = !carry::stashed() && restore::status().spawning == 0;
    let due = {
        let mut state = STATE.lock();
        state.empty_s = if empty { state.empty_s + dt } else { 0.0 };
        empty && quiet && state.empty_s >= REFILL_AFTER_S && !state.grants.respawn_pending()
    };
    if !due {
        return;
    }
    let candidate = Hand::BOTH.into_iter().find_map(|side| {
        let socket = hip_socket(side);
        carry::on_socket(socket)
            .filter(|&object| set::contains(object))
            .map(|object| (object, socket))
    });
    let Some((object, socket)) = candidate else {
        return;
    };
    match unsafe { carry::promote_unequipped(owner, object) } {
        Ok(()) => {
            entered_slot(object, socket);
            tracing::info!(
                target: "controllers",
                socket = socket.label(),
                "single pistols: the pistol on the {} is the slot's now",
                socket.label()
            );
        }
        Err(refusal) => {
            tracing::debug!(target: "controllers", %refusal, "single pistols: the empty slot was not refilled");
        }
    }
}

/// Add `rounds` to `owner`'s reserve of `weapon`'s ammunition.
unsafe fn return_rounds(owner: &mut Character, weapon: &WeaponBase, rounds: i32) {
    if rounds <= 0 {
        return;
    }
    // Read as the raw value: an unset type is out of the enum's range.
    let raw = unsafe { (&raw const weapon.m_AmmunitionType).cast::<i32>().read() };
    if !(0..AmmunitionType::NOF_AMMO_TYPES as i32).contains(&raw)
        || raw == AmmunitionType::AMMO_UNLIMITED as i32
    {
        return;
    }
    let ammo_type = weapon.m_AmmunitionType;
    let inventory: &mut Inventory = &mut owner.m_Inventory;
    let reserve = unsafe { inventory.GetAmmoCount(ammo_type) };
    unsafe { inventory.SetAmmoCount(ammo_type, reserve.saturating_add(rounds), true) };
}

fn note(count: impl FnOnce(&mut State), what: String) {
    let mut state = STATE.lock();
    count(&mut state);
    state.last = Some(what);
}

/// The weapon in `owner`'s dual-wield slot.
unsafe fn slot_weapon(owner: &Character) -> Option<&'static mut WeaponBase> {
    unsafe {
        owner
            .m_Inventory
            .m_Weapons
            .get(WeaponSlot::E_SLOT_DUAL_WIELD as usize)?
            .px
            .as_mut()
    }
}

/// `weapon`'s slot, read as the raw value: an unset slot is out of the enum's range.
unsafe fn slot_of(weapon: &WeaponBase) -> Option<WeaponSlot> {
    slot_from_raw(unsafe { (&raw const weapon.m_EquipSlot).cast::<i32>().read() })
}

/// How many guns `weapon` has, all of them.
unsafe fn gun_count(weapon: &WeaponBase) -> usize {
    let list = &weapon.m_lWeaponData;
    if list.m_Begin.is_null() || list.m_End < list.m_Begin {
        0
    } else {
        unsafe { list.m_End.offset_from(list.m_Begin) as usize }
    }
}

/// Gun `index` of `weapon`, to write: each gun is its own object, not part of the weapon.
unsafe fn gun_mut(
    weapon: &mut WeaponBase,
    index: usize,
) -> Option<&'static mut jc3gi::equipment::weapon::WeaponData> {
    if index >= unsafe { gun_count(weapon) } {
        return None;
    }
    unsafe { (*weapon.m_lWeaponData.m_Begin.add(index)).as_mut() }
}

/// Whether `activator` is the local player's `CGameObject` subobject.
fn is_local_player(activator: *mut GameObject) -> bool {
    let local = ik::local_character();
    local != 0 && activator as usize == local + Character::GAME_OBJECT_BASE_OFFSET as usize
}

#[detour(address = jc3gi::equipment::weapon::WeaponBase::SetVisibleBase_ADDRESS)]
fn set_visible_base(weapon: *mut WeaponBase, visible: bool, fade: bool, force: bool) {
    SET_VISIBLE_BASE
        .get()
        .unwrap()
        .call(weapon, visible, fade, force);
    if set::len() == 0 || !set::contains(weapon as usize) {
        return;
    }
    // SAFETY: the weapon was live for the call, and the caller is updating it.
    if let Some(weapon) = unsafe { weapon.as_mut() } {
        unsafe { hide_extra(weapon) };
    }
}

/// The local player's pickup of a pistol from the world: while pistols are made single, the
/// pistol taken is one pistol, made single before the inventory's add equips and shows it.
#[detour(address = jc3gi::equipment::weapon::WeaponBase::PickupItem_ADDRESS)]
fn pickup_item(weapon: *mut WeaponBase, activator: *mut GameObject) {
    if ACTIVE.load(Ordering::Relaxed) && is_local_player(activator) {
        // SAFETY: the pickup is given a live weapon.
        if let Some(w) = unsafe { weapon.as_mut() }.filter(|w| !w.m_InfinitePickup)
            && unsafe { designate(w, "a pistol taken from the world") }
        {
            PICKED.fetch_add(1, Ordering::Relaxed);
        }
    }
    PICKUP_ITEM.get().unwrap().call(weapon, activator);
}

/// The local player's pickup start of a rack's pistol with unlimited stock asks the inventory for
/// a copy, which arrives a few frames later through the inventory's add: the copy is one pistol.
#[detour(address = jc3gi::equipment::weapon::WeaponBase::PickupStart_ADDRESS)]
fn pickup_start(weapon: *mut WeaponBase, activator: *mut GameObject) {
    if ACTIVE.load(Ordering::Relaxed) && is_local_player(activator) {
        // SAFETY: the pickup is given a live weapon.
        let copies = unsafe { weapon.as_ref() }.is_some_and(|w| {
            w.m_InfinitePickup && unsafe { slot_of(w) } == Some(WeaponSlot::E_SLOT_DUAL_WIELD)
        });
        if copies {
            RACK_COPY_ASKED.store(true, Ordering::Relaxed);
        }
    }
    PICKUP_START.get().unwrap().call(weapon, activator);
}
