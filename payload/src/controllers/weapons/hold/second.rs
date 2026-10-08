//! A second weapon in the other hand, fired by the mod: the design's "two weapons, one per hand".
//!
//! The game wields one weapon, the selected slot's. While it is in one hand, the free hand can take
//! a long gun of another slot off its shoulder, and hold it there as the second weapon. The second
//! weapon stays in the holstered state, so the game's inventory, selection, HUD, and saves see it
//! as holstered, and its bodies stay out of the physics world:
//!
//! - its guns' holstered entries in the attach-bone table name the hand's attach bone, written at
//!   the same three points as the held weapon's ([`apply`], from the hold's detours), and its
//!   holstered scale ([`WeaponData::m_HolsteredScaleValue`]) is one while it is in the hand, both
//!   restored when it goes back;
//! - it is given a user ([`WeaponBase::StartUsing`]), so its fire code runs as the player's, and
//!   loses it when it goes back ([`WeaponBase::StopUsing`], which leaves a holstered weapon
//!   holstered);
//! - after the inventory's own weapon updates ([`Inventory::UpdateWeapons_Parallel`] and
//!   [`Inventory::UpdateWeapons_Serial`]), the mod updates it the same way, through its
//!   [`WeaponBase::UpdateFromUserPostSim_Parallel`] and `_Serial` virtuals, with the hand's trigger
//!   as the fire input while the animation allows shooting
//!   ([`Inventory::GetAnimationAllowsShooting`]) and the magazine holds a round, and the gun's own
//!   sight line's aim point.
//!
//! The rigid grip, the gun aim, the per-shot seam, and the render-time late update treat it as a
//! held weapon ([`is`]). Its magazine is its own; the true reload, the HUD's readout, and the game's
//! reload act are the selected weapon's only, so the second weapon fires until it is empty. It goes
//! back on its shoulder at a grab of its holster by its hand, and is let go of (left holstered)
//! when the game wields it, takes it from its slot, or the weapon handling stands down.
//!
//! The detours run on the simulation's worker threads for every character, so they compare against
//! the second weapon's address before taking any lock.

use std::{
    fmt,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::Vec3;
use jc3gi::{
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    equipment::weapon::{WeaponBase, WeaponInputs, WeaponState},
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    hands::ik,
    weapons::{
        engine,
        hold::{self, equip, layout},
        holster::slot_label,
    },
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_WEAPONS_PARALLEL_BINDER)
        .with_static_binder(&UPDATE_WEAPONS_SERIAL_BINDER)
}

/// Why a second weapon was not taken.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SecondRefusal {
    NoCharacter,
    /// The slot is not a long gun's.
    Unsupported,
    EmptySlot,
    /// The slot's weapon is not on the body, or is in use.
    NotHolstered,
    /// A second weapon is already in the hands.
    Occupied,
}

impl fmt::Display for SecondRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            SecondRefusal::NoCharacter => "there is no local player",
            SecondRefusal::Unsupported => "the slot is not a long gun's",
            SecondRefusal::EmptySlot => "the slot holds no weapon",
            SecondRefusal::NotHolstered => "the slot's weapon is not on the body",
            SecondRefusal::Occupied => "a second weapon is already in the hands",
        };
        write!(f, "second weapon: {why}")
    }
}

impl std::error::Error for SecondRefusal {}

/// The second weapon's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct SecondStatus {
    /// The hand holding it and its slot.
    pub held: Option<(Hand, WeaponSlot)>,
    /// Whether its hand's trigger was pulled at the last input tick.
    pub trigger: bool,
    /// The updates the mod gave it since the payload loaded.
    pub updates: u64,
    pub last_refusal: Option<SecondRefusal>,
}

pub fn status() -> SecondStatus {
    let state = STATE.lock();
    SecondStatus {
        held: state.held.map(|h| (h.hand, h.slot)),
        trigger: TRIGGER.load(Ordering::Relaxed),
        updates: UPDATES.load(Ordering::Relaxed),
        last_refusal: state.last_refusal,
    }
}

/// Whether the weapon at `weapon` is the second weapon.
pub(crate) fn is(weapon: usize) -> bool {
    weapon != 0 && ADDRESS.load(Ordering::Relaxed) == weapon
}

/// The hand holding the second weapon.
pub(crate) fn hand() -> Option<Hand> {
    (ADDRESS.load(Ordering::Relaxed) != 0)
        .then(|| match HAND.load(Ordering::Relaxed) {
            1 => Some(Hand::Left),
            2 => Some(Hand::Right),
            _ => None,
        })
        .flatten()
}

/// The second weapon, when `owner`'s inventory holds it.
///
/// # Safety
///
/// `owner` must be the live local player character.
pub(crate) unsafe fn weapon(owner: &Character) -> Option<&'static WeaponBase> {
    let address = ADDRESS.load(Ordering::Relaxed);
    if address == 0 {
        return None;
    }
    owner
        .m_Inventory
        .m_Weapons
        .iter()
        .find(|slot| slot.px as usize == address)
        .and_then(|slot| unsafe { slot.px.as_ref() })
}

/// The slot of the second weapon.
pub(crate) fn slot() -> Option<WeaponSlot> {
    STATE.lock().held.map(|h| h.slot)
}

/// Take `slot`'s holstered long gun into `hand` as the second weapon.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn take(hand: Hand, slot: WeaponSlot) -> Result<(), SecondRefusal> {
    let result = unsafe { take_inner(hand, slot) };
    let mut state = STATE.lock();
    match result {
        Ok(held) => {
            state.held = Some(held);
            HAND.store(
                match hand {
                    Hand::Left => 1,
                    Hand::Right => 2,
                },
                Ordering::Relaxed,
            );
            ADDRESS.store(held.weapon, Ordering::Relaxed);
            drop(state);
            tracing::info!(
                target: "controllers",
                hand = hand.label(),
                slot = slot_label(slot),
                "hold: the second weapon is in the {} hand",
                hand.label()
            );
            Ok(())
        }
        Err(refusal) => {
            state.last_refusal = Some(refusal);
            drop(state);
            hold::refused(refusal);
            Err(refusal)
        }
    }
}

/// Put the second weapon back on its shoulder.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn put_back() {
    let Some(owner) = (unsafe { engine::local_player() }) else {
        forget();
        return;
    };
    unsafe { release(owner, true) };
}

/// Check the second weapon is still in its slot, holstered, and used by `owner`; let it go
/// otherwise (left as the game has it).
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn validate(owner: &mut Character) {
    let Some(held) = STATE.lock().held else {
        return;
    };
    let live = unsafe { weapon_in(&owner.m_Inventory, held.slot) }
        .filter(|w| *w as *const WeaponBase as usize == held.weapon);
    let Some(weapon) = live else {
        // The weapon left its slot: nothing to restore on it.
        forget();
        tracing::info!(target: "controllers", "hold: the second weapon left its slot");
        return;
    };
    let used = std::ptr::eq(weapon.m_User[0].cast::<Character>(), owner);
    if weapon.m_State != WeaponState::E_WS_HOLSTERED || !used {
        unsafe { release(owner, false) };
        tracing::info!(target: "controllers", "hold: the game took the second weapon");
    }
}

/// Publish the second weapon's hand's trigger for its updates.
pub(crate) fn set_trigger(pulled: bool) {
    TRIGGER.store(pulled, Ordering::Relaxed);
}

/// Publish the second weapon's aim point, from its gun's sight line, at the aim update.
pub(crate) fn publish_target(target: Option<Vec3>) {
    *TARGET.lock() = target.map(|t| (t, Instant::now()));
}

/// Write the second weapon's hand into its holstered entries, when `weapon` is it.
///
/// # Safety
///
/// `weapon` must be live, on the thread updating its holder.
pub(crate) unsafe fn apply(weapon: &mut WeaponBase) {
    let address = weapon as *mut WeaponBase as usize;
    if !is(address) {
        return;
    }
    let Some(hand) = hand() else {
        return;
    };
    let bone = layout::hand_bone(hand);
    for gun in unsafe { guns_mut(weapon) } {
        if let Some(entry) = gun
            .m_lBoneAttachementPerWeaponState
            .get_mut(WeaponState::E_WS_HOLSTERED as usize)
        {
            *entry = bone;
        }
    }
}

/// Write the second weapon's hand into its holstered entries, when `inventory` holds it.
///
/// # Safety
///
/// `inventory` must be null or live, on the thread updating its owner.
pub(crate) unsafe fn apply_owned(inventory: *mut Inventory) {
    if let Some(weapon) = unsafe { second_in(inventory) } {
        unsafe { apply(weapon) };
    }
}

/// Stop: let the second weapon go, left as the game has it. Called when the weapon handling
/// stands down.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn stand_down() {
    if STATE.lock().held.is_none() {
        return;
    }
    match unsafe { engine::local_player() } {
        Some(owner) => unsafe { release(owner, true) },
        None => forget(),
    }
}

/// The second weapon while it is held.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Held {
    weapon: usize,
    slot: WeaponSlot,
    hand: Hand,
    /// Each gun's holstered bone and holstered scale before it came into the hand.
    saved: [Option<(u32, f32)>; gun::MAX_GUNS],
}

struct State {
    held: Option<Held>,
    last_refusal: Option<SecondRefusal>,
}

static STATE: Mutex<State> = Mutex::new(State {
    held: None,
    last_refusal: None,
});
/// The second weapon's address, or zero: the detours' check before any lock.
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
/// The hand holding it: one for the left, two for the right.
static HAND: AtomicU8 = AtomicU8::new(0);
static TRIGGER: AtomicBool = AtomicBool::new(false);
static TARGET: Mutex<Option<(Vec3, Instant)>> = Mutex::new(None);
static UPDATES: AtomicU64 = AtomicU64::new(0);

/// How long the aim update's point stays current for the second weapon's update.
const MAX_TARGET_AGE: Duration = Duration::from_millis(250);

unsafe fn take_inner(hand: Hand, slot: WeaponSlot) -> Result<Held, SecondRefusal> {
    if STATE.lock().held.is_some() {
        return Err(SecondRefusal::Occupied);
    }
    if !equip::handles(slot) {
        return Err(SecondRefusal::Unsupported);
    }
    let owner = unsafe { engine::local_player() }.ok_or(SecondRefusal::NoCharacter)?;
    let weapon = unsafe { weapon_in(&owner.m_Inventory, slot) }.ok_or(SecondRefusal::EmptySlot)?;
    if weapon.m_State != WeaponState::E_WS_HOLSTERED || !weapon.m_User[0].is_null() {
        return Err(SecondRefusal::NotHolstered);
    }
    let bone = layout::hand_bone(hand);
    let mut saved = [None; gun::MAX_GUNS];
    for (gun, slot_saved) in unsafe { guns_mut(weapon) }.zip(saved.iter_mut()) {
        let Some(entry) = gun
            .m_lBoneAttachementPerWeaponState
            .get_mut(WeaponState::E_WS_HOLSTERED as usize)
        else {
            continue;
        };
        *slot_saved = Some((*entry, gun.m_HolsteredScaleValue));
        *entry = bone;
        gun.m_HolsteredScaleValue = 1.0;
    }
    unsafe { skip_interpolation(weapon) };
    unsafe { weapon.StartUsing(owner) };
    Ok(Held {
        weapon: weapon as *mut WeaponBase as usize,
        slot,
        hand,
        saved,
    })
}

/// Restore the second weapon's table and scale, stop using it when `stop` and it is still the
/// player's, and forget it.
unsafe fn release(owner: &mut Character, stop: bool) {
    let Some(held) = STATE.lock().held else {
        return;
    };
    forget();
    let Some(weapon) = (unsafe { weapon_in(&owner.m_Inventory, held.slot) })
        .filter(|w| *w as *const WeaponBase as usize == held.weapon)
    else {
        return;
    };
    for (gun, saved) in unsafe { guns_mut(weapon) }.zip(held.saved) {
        let Some((bone, scale)) = saved else {
            continue;
        };
        if let Some(entry) = gun
            .m_lBoneAttachementPerWeaponState
            .get_mut(WeaponState::E_WS_HOLSTERED as usize)
        {
            *entry = bone;
        }
        gun.m_HolsteredScaleValue = scale;
    }
    unsafe { skip_interpolation(weapon) };
    let used = std::ptr::eq(weapon.m_User[0].cast::<Character>(), owner);
    if stop && used && weapon.m_State == WeaponState::E_WS_HOLSTERED {
        unsafe { weapon.StopUsing() };
    }
    tracing::info!(target: "controllers", "hold: the second weapon is back on the body");
}

fn forget() {
    STATE.lock().held = None;
    ADDRESS.store(0, Ordering::Relaxed);
    HAND.store(0, Ordering::Relaxed);
    TRIGGER.store(false, Ordering::Relaxed);
}

/// The weapon in `inventory`'s `slot`.
unsafe fn weapon_in(inventory: &Inventory, slot: WeaponSlot) -> Option<&'static mut WeaponBase> {
    unsafe { inventory.m_Weapons.get(slot as usize)?.px.as_mut() }
}

/// The weapon's guns, to write.
unsafe fn guns_mut(
    weapon: &mut WeaponBase,
) -> impl Iterator<Item = &'static mut jc3gi::equipment::weapon::WeaponData> {
    let list = &weapon.m_lWeaponData;
    let begin = list.m_Begin;
    let count = if begin.is_null() || list.m_End < begin {
        0
    } else {
        unsafe { list.m_End.offset_from(begin) as usize }
    };
    (0..count.min(gun::MAX_GUNS)).filter_map(move |i| unsafe { (*begin.add(i)).as_mut() })
}

unsafe fn skip_interpolation(weapon: &mut WeaponBase) {
    let count = unsafe { gun::guns(weapon) }.count();
    for index in 0..count {
        if let Ok(index) = i32::try_from(index) {
            unsafe { weapon.SkipRenderInterpolationForOneFrame(index) };
        }
    }
}

/// The second weapon of `inventory`, when the inventory is the local player's and holds it.
unsafe fn second_in(inventory: *mut Inventory) -> Option<&'static mut WeaponBase> {
    let address = ADDRESS.load(Ordering::Relaxed);
    if address == 0 {
        return None;
    }
    let inventory = unsafe { inventory.as_ref() }?;
    let local = ik::local_character();
    if local == 0 || inventory.m_Character as usize != local {
        return None;
    }
    inventory
        .m_Weapons
        .iter()
        .find(|slot| slot.px as usize == address)
        .and_then(|slot| unsafe { slot.px.as_mut() })
}

/// The second weapon's inputs and aim point for one update, or `None` without a current aim point.
/// An empty magazine fires nothing: the weapon's ammunition check would otherwise ask the character
/// to reload, which reloads the wielded weapon, or switch weapons.
unsafe fn update_arguments(
    inventory: &Inventory,
    weapon: &WeaponBase,
) -> Option<(WeaponInputs, Vector3)> {
    let (target, at) = (*TARGET.lock())?;
    if at.elapsed() > MAX_TARGET_AGE || !target.is_finite() {
        return None;
    }
    let loaded = weapon.m_MagazineAmmo > 0 && !weapon.m_WeaponIsWithoutMagazine;
    let fire = loaded
        && TRIGGER.load(Ordering::Relaxed)
        && unsafe { inventory.GetAnimationAllowsShooting() };
    Some((
        WeaponInputs {
            m_PrimaryFireInput: if fire { 1.0 } else { 0.0 },
            m_SecondaryFireInput: 0.0,
        },
        Vector3 {
            data: target.to_array(),
        },
    ))
}

#[detour(address = jc3gi::character::inventory::Inventory::UpdateWeapons_Parallel_ADDRESS)]
fn update_weapons_parallel(
    inventory: *mut Inventory,
    dt: f32,
    muzzle_target: *const Vector3,
    grenade_target: *const Vector3,
) {
    UPDATE_WEAPONS_PARALLEL
        .get()
        .unwrap()
        .call(inventory, dt, muzzle_target, grenade_target);
    let Some(weapon) = (unsafe { second_in(inventory) }) else {
        return;
    };
    // SAFETY: `second_in` found the inventory live.
    let Some((inputs, aim)) = (unsafe { update_arguments(&*inventory, weapon) }) else {
        return;
    };
    unsafe { weapon.UpdateFromUserPostSim_Parallel(dt, &aim, &inputs) };
    UPDATES.fetch_add(1, Ordering::Relaxed);
}

#[detour(address = jc3gi::character::inventory::Inventory::UpdateWeapons_Serial_ADDRESS)]
fn update_weapons_serial(
    inventory: *mut Inventory,
    dt: f32,
    muzzle_target: *const Vector3,
    grenade_target: *const Vector3,
) {
    UPDATE_WEAPONS_SERIAL
        .get()
        .unwrap()
        .call(inventory, dt, muzzle_target, grenade_target);
    let Some(weapon) = (unsafe { second_in(inventory) }) else {
        return;
    };
    // SAFETY: `second_in` found the inventory live.
    let Some((inputs, aim)) = (unsafe { update_arguments(&*inventory, weapon) }) else {
        return;
    };
    unsafe { weapon.UpdateFromUserPostSim_Serial(dt, &aim, &inputs) };
}
