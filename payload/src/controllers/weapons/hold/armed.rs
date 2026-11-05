//! The weapons a hand holds that the game does not wield: each is armed in its hand by the mod.
//!
//! The game wields one weapon, the selected slot's: the primary hand's. A weapon the loadout table
//! holds in the other hand (a slot's weapon, or one the VR inventory carries beyond the slots)
//! stays in the holstered state, so the game's inventory, selection, HUD, and saves see a slot's weapon as
//! holstered, and its bodies stay out of the physics world. Arming it in its hand ([`arm`]):
//!
//! - its guns' holstered entries in the attach-bone table name the hand's attach bone, written at
//!   the same three points as the wielded weapon's ([`apply_in`], from the hold's detours), and its
//!   holstered scale ([`WeaponData::m_HolsteredScaleValue`]) is one while it is in the hand; both
//!   are restored when it is disarmed ([`disarm`]). A slot's weapon is placed by the inventory, and
//!   a carried one by the carry set ([`crate::controllers::weapons::inventory::carry`]);
//! - it is given a user ([`WeaponBase::StartUsing`]), so its fire code runs as the player's, and
//!   loses it when it is disarmed ([`WeaponBase::StopUsing`], which leaves a holstered weapon
//!   holstered). Its holder stays the player, as a slot's or a carried weapon's is, so its bullets
//!   ignore the player's own bodies.
//!
//! The per-hand tick updates it with its hand's own inputs
//! ([`crate::controllers::weapons::hold::tick`]); the rigid grip, the gun aim, the per-shot seam,
//! and the render-time late update treat it as a held weapon ([`hand_of`]). This is per-hand
//! runtime state beside the table, not an item's place: the table says what the hand holds, and
//! this says what the mod did to the object to hold it there.
//!
//! The detours run on the simulation's worker threads for every character, so the armed weapons
//! are published as one address per hand, compared before any lock. Arming and disarming run on
//! the input tick, on the game thread before the world update.

use std::{
    fmt,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use jc3gi::{
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    equipment::weapon::{WeaponBase, WeaponData, WeaponState},
};
use parking_lot::Mutex;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    hands::ik,
    weapons::{
        dual_wield::single,
        engine::{self, HeldWeapon, Wielded, slot_of},
        heft::HeftClass,
        hold::layout,
        inventory::{carry, engine as inventory_engine},
    },
};

/// Why a weapon was not armed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ArmRefusal {
    /// The weapon is not holstered, or another character uses it.
    NotHolstered,
    /// The hand has a weapon armed already.
    Occupied,
}

impl fmt::Display for ArmRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            ArmRefusal::NotHolstered => "the weapon is not on the body, or is in use",
            ArmRefusal::Occupied => "the hand has a weapon armed already",
        };
        write!(f, "armed: {why}")
    }
}

impl std::error::Error for ArmRefusal {}

/// One armed weapon, as the readouts show it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ArmedReadout {
    pub slot: WeaponSlot,
    /// Its magazine and the magazine's size as of the last input tick.
    pub magazine: (i32, i32),
}

/// The weapons armed in the hands as of the last input tick, by [`Hand::index`].
pub fn status() -> [Option<ArmedReadout>; 2] {
    *READOUTS.lock()
}

/// The hand the weapon at `address` is armed in.
pub(crate) fn hand_of(address: usize) -> Option<Hand> {
    if address == 0 {
        return None;
    }
    Hand::BOTH
        .into_iter()
        .find(|hand| ARMED[hand.index()].load(Ordering::Relaxed) == address)
}

/// Whether the weapon at `address` is armed in a hand.
pub(crate) fn is(address: usize) -> bool {
    hand_of(address).is_some()
}

/// The weapon armed in `hand`.
pub(crate) fn in_hand(hand: Hand) -> Option<usize> {
    match ARMED[hand.index()].load(Ordering::Relaxed) {
        0 => None,
        address => Some(address),
    }
}

/// Arm the holstered `weapon` in `hand` for `owner`: its holstered entries name the hand's bone,
/// its holstered scale is one, and the character uses it.
///
/// # Safety
///
/// `owner` must be the live local player character and `weapon` live, on the game thread before
/// the world update.
pub(crate) unsafe fn arm(
    owner: &mut Character,
    weapon: &mut WeaponBase,
    hand: Hand,
) -> Result<(), ArmRefusal> {
    if weapon.m_State != WeaponState::E_WS_HOLSTERED || !weapon.m_User[0].is_null() {
        return Err(ArmRefusal::NotHolstered);
    }
    let mut hands = HANDS.lock();
    if hands[hand.index()].is_some() {
        return Err(ArmRefusal::Occupied);
    }
    let saved = unsafe { arm_layout(weapon, hand) };
    let address = weapon as *mut WeaponBase as usize;
    hands[hand.index()] = Some(Armed {
        weapon: address,
        saved,
    });
    ARMED[hand.index()].store(address, Ordering::Relaxed);
    drop(hands);
    unsafe { skip_interpolation(weapon) };
    unsafe { weapon.StartUsing(owner) };
    Ok(())
}

/// Disarm `hand`'s weapon: put back the guns' holstered entries and scales the hand took over, and
/// stop `owner` using it when `stop` and it is still holstered and the player's. `weapon` is the
/// armed weapon, live, or `None` when it is gone, which only forgets it.
///
/// # Safety
///
/// `owner` must be the live local player character, and `weapon` the weapon armed in `hand` when
/// given, on the game thread before the world update. No weapon borrow may overlap this call;
/// the slot or carry-set source must remain retained until restoration and `StopUsing` return.
pub(crate) unsafe fn disarm(
    owner: &mut Character,
    hand: Hand,
    weapon: Option<NonNull<WeaponBase>>,
    stop: bool,
) {
    let Some(armed) = forget(hand) else {
        return;
    };
    let Some(mut weapon) = weapon.filter(|w| w.as_ptr() as usize == armed.weapon) else {
        return;
    };
    // SAFETY: the caller serializes native mutation and keeps the validated weapon alive.
    let weapon = unsafe { weapon.as_mut() };
    unsafe { restore_layout(weapon, armed.saved) };
    unsafe { skip_interpolation(weapon) };
    let used = std::ptr::eq(weapon.m_User[0].cast::<Character>(), owner);
    if stop && used && weapon.m_State == WeaponState::E_WS_HOLSTERED {
        unsafe { weapon.StopUsing() };
    }
}

/// Forget `hand`'s armed weapon without touching it: its object is gone, or the game took it.
/// Returns what was armed.
pub(crate) fn forget(hand: Hand) -> Option<Armed> {
    let armed = HANDS.lock()[hand.index()].take();
    ARMED[hand.index()].store(0, Ordering::Relaxed);
    armed
}

/// Write each armed weapon's hand into its holstered entries, for the ones `inventory` holds, or
/// that the carry set keeps when `inventory` is the local player's.
///
/// # Safety
///
/// `inventory` must be null or live, on the thread updating its owner.
pub(crate) unsafe fn apply_in(inventory: *mut Inventory) {
    for hand in Hand::BOTH {
        let Some(address) = in_hand(hand) else {
            continue;
        };
        if let Some(mut weapon) = unsafe { weapon_in(inventory, address) } {
            // SAFETY: this inventory phase serializes each weapon write and retains its source.
            unsafe { write_hand(weapon.as_mut(), hand) };
        }
    }
}

/// Release an armed weapon's input without passing an unavailable aim target to native updates.
///
/// # Safety
///
/// The inventory phase must retain the source and serialize this weapon's native mutation.
pub(crate) unsafe fn release_fire(weapon: &mut WeaponBase) {
    unsafe { release_fire_with(weapon, |weapon, gun| weapon.Fire_Stop(gun)) };
}

unsafe fn release_fire_with(
    weapon: &mut WeaponBase,
    mut stop: impl FnMut(&mut WeaponBase, *mut WeaponData),
) {
    let firing = weapon.m_IsFiring;
    let guns = unsafe { gun_handles(weapon) };
    // UpdateWeapon_Parallel owns these mapped input latches and pending shot requests. Clearing
    // them supplies a target-independent release without running its aim checks or firing paths.
    weapon.m_IsTryingToFire = false;
    weapon.m_IsTriggerPulled = false;
    weapon.m_FiredThisInput = false;
    weapon.m_IsFiringCancelled = false;
    weapon.m_IsFeatheringTrigger = false;
    weapon.m_InputBufferTimer = 0.0;
    weapon.m_ForceFireProjectile = false;
    weapon.m_ForceFireTimer = 0.0;
    weapon.m_ForceFireCount = 0;
    for mut gun in guns.into_iter().flatten() {
        // SAFETY: the unique handles borrow each retained gun only for this assignment.
        unsafe { gun.as_mut().m_BurstShotsLeft = 0 };
        if firing {
            stop(weapon, gun.as_ptr());
        }
    }
    // Fire_StopBase clears this flag; also clear it for an empty native gun list.
    weapon.m_IsFiring = false;
}

/// The weapons armed in the hands that the carry set keeps rather than `inventory`, when
/// `inventory` is the local player's.
///
/// # Safety
///
/// The inventory must belong to the live local owner. Its update phase must retain the carry-set
/// sources without replacement or removal for the returned borrow.
pub(crate) unsafe fn carried_for(inventory: &Inventory) -> Vec<&WeaponBase> {
    let local = ik::local_character();
    if local == 0 || inventory.m_Character as usize != local {
        return Vec::new();
    }
    Hand::BOTH
        .into_iter()
        .filter_map(in_hand)
        .filter(|&address| {
            !inventory
                .m_Weapons
                .iter()
                .any(|slot| slot.px as usize == address)
                && carry::contains(address)
        })
        // SAFETY: the carry set keeps a carried weapon alive.
        .filter_map(|address| unsafe { (address as *const WeaponBase).as_ref() })
        .collect()
}

/// The weapon armed in `hand` as the weapon handling sees a weapon in the hands: every gun in that
/// hand, and a long gun by its slot, since it stays in the holstered state.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub(crate) unsafe fn held_weapon(owner: &Character, hand: Hand) -> Option<HeldWeapon> {
    let weapon = unsafe { weapon_of(owner, hand) }?;
    let slot = unsafe { slot_of(weapon) }?;
    // Read as the raw value: an unset or unknown class is out of the enum's range.
    let class = unsafe { (&raw const weapon.m_Class).cast::<i32>().read() };
    // Held in the hand rather than mounted, a long gun is held in both hands' reach.
    let two_handed = engine::long_slot(slot);
    Some(HeldWeapon {
        address: weapon as *const WeaponBase as usize,
        wielded: Wielded {
            slot,
            hands: Hand::BOTH.map(|h| h == hand),
        },
        holder: Some(hand),
        two_handed,
        class: HeftClass::of(class, two_handed),
    })
}

/// Read each armed weapon's magazine for the readouts ([`status`]).
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub(crate) unsafe fn sample_readouts(owner: &Character) {
    let read = Hand::BOTH.map(|hand| {
        let weapon = unsafe { weapon_of(owner, hand) }?;
        let slot = unsafe { slot_of(weapon) }?;
        Some(ArmedReadout {
            slot,
            magazine: (weapon.m_MagazineAmmo, unsafe {
                single::magazine_size(weapon)
            }),
        })
    });
    *READOUTS.lock() = read;
}

/// Disarm the weapons in the hands, left on the body where the game has them, or forget them when
/// there is no local player. Called when the weapon handling stands down.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn stand_down() {
    *READOUTS.lock() = [None; 2];
    let Some(owner) = (unsafe { engine::local_player() }) else {
        for hand in Hand::BOTH {
            forget(hand);
        }
        return;
    };
    for hand in Hand::BOTH {
        let weapon = unsafe { weapon_handle(owner, hand) };
        unsafe { disarm(owner, hand, weapon, true) };
    }
}

/// One armed weapon.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Armed {
    pub weapon: usize,
    /// Each gun's holstered bone and holstered scale before it came into the hand.
    saved: [Option<SavedGun>; MAX_ARMED_GUNS],
}

const MAX_ARMED_GUNS: usize = 2;

#[derive(Debug, Copy, Clone, PartialEq)]
struct SavedGun {
    address: usize,
    bone: u32,
    scale: f32,
}

/// The armed weapons, by [`Hand::index`].
static HANDS: Mutex<[Option<Armed>; 2]> = Mutex::new([None; 2]);
/// The armed weapons' addresses, or zero, by [`Hand::index`]: the detours' check before any lock.
static ARMED: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
static READOUTS: Mutex<[Option<ArmedReadout>; 2]> = Mutex::new([None; 2]);

#[cfg(test)]
mod tests;

/// The weapon armed in `hand`, when `owner`'s inventory or the carry set holds it.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update or
/// on the thread updating it. The phase must retain the slot or carry-set source without native
/// mutation for the returned shared borrow.
pub(crate) unsafe fn weapon_of(owner: &Character, hand: Hand) -> Option<&WeaponBase> {
    let handle = unsafe { weapon_handle(owner, hand) }?;
    // SAFETY: the owner phase keeps the slot or carry-set source alive for this shared borrow.
    Some(unsafe { handle.as_ref() })
}

/// A validated weapon handle, without creating an exclusive native borrow.
///
/// # Safety
///
/// As [`weapon_of`]. Dereferencing the handle additionally requires the caller to keep its source
/// alive, serialize native mutation, and end each borrow before another call can touch the weapon.
pub(crate) unsafe fn weapon_handle(owner: &Character, hand: Hand) -> Option<NonNull<WeaponBase>> {
    let inventory = &owner.m_Inventory as *const Inventory as *mut Inventory;
    unsafe { weapon_in(inventory, in_hand(hand)?) }
}

/// The armed weapon at `address`, when `inventory` holds it, or when it is a carried one and
/// `inventory` is the local player's.
///
/// # Safety
///
/// `inventory` must be null or live, on the thread updating its owner.
pub(crate) unsafe fn weapon_in(
    inventory: *mut Inventory,
    address: usize,
) -> Option<NonNull<WeaponBase>> {
    let inventory = unsafe { inventory.as_ref() }?;
    let local = ik::local_character();
    if local == 0 || inventory.m_Character as usize != local {
        return None;
    }
    inventory_engine::slot_handle(inventory, address)
        .or_else(|| unsafe { carry::weapon_handle(address) })
}

/// Write `hand`'s attach bone into `weapon`'s holstered entries.
unsafe fn write_hand(weapon: &mut WeaponBase, hand: Hand) {
    let bone = layout::hand_bone(hand);
    unsafe {
        for_each_gun_mut(weapon, |_, gun| {
            if let Some(entry) = gun
                .m_lBoneAttachementPerWeaponState
                .get_mut(WeaponState::E_WS_HOLSTERED as usize)
            {
                *entry = bone;
            }
        })
    };
}

unsafe fn arm_layout(weapon: &mut WeaponBase, hand: Hand) -> [Option<SavedGun>; MAX_ARMED_GUNS] {
    let bone = layout::hand_bone(hand);
    let mut saved = [None; MAX_ARMED_GUNS];
    unsafe {
        for_each_gun_mut(weapon, |index, gun| {
            let address = gun as *mut WeaponData as usize;
            let Some(entry) = gun
                .m_lBoneAttachementPerWeaponState
                .get_mut(WeaponState::E_WS_HOLSTERED as usize)
            else {
                return;
            };
            saved[index] = Some(SavedGun {
                address,
                bone: *entry,
                scale: gun.m_HolsteredScaleValue,
            });
            *entry = bone;
            gun.m_HolsteredScaleValue = 1.0;
        })
    };
    saved
}

unsafe fn restore_layout(weapon: &mut WeaponBase, saved: [Option<SavedGun>; MAX_ARMED_GUNS]) {
    unsafe {
        for_each_gun_mut(weapon, |_, gun| {
            let Some(saved) = saved
                .into_iter()
                .flatten()
                .find(|saved| saved.address == gun as *mut WeaponData as usize)
            else {
                return;
            };
            if let Some(entry) = gun
                .m_lBoneAttachementPerWeaponState
                .get_mut(WeaponState::E_WS_HOLSTERED as usize)
            {
                *entry = saved.bone;
            }
            gun.m_HolsteredScaleValue = saved.scale;
        })
    };
}

/// Borrow each distinct native gun once, with no borrow surviving the callback.
unsafe fn for_each_gun_mut(weapon: &mut WeaponBase, mut visit: impl FnMut(usize, &mut WeaponData)) {
    for (index, mut gun) in unsafe { gun_handles(weapon) }
        .into_iter()
        .flatten()
        .enumerate()
    {
        visit(index, unsafe { gun.as_mut() });
    }
}

unsafe fn gun_handles(weapon: &WeaponBase) -> [Option<NonNull<WeaponData>>; MAX_ARMED_GUNS] {
    let list = &weapon.m_lWeaponData;
    let begin = list.m_Begin;
    let count = if begin.is_null() || list.m_End < begin {
        0
    } else {
        unsafe { list.m_End.offset_from(begin) as usize }
    };
    let mut guns = [None; MAX_ARMED_GUNS];
    let mut next = 0;
    for index in 0..count.min(MAX_ARMED_GUNS) {
        let Some(gun) = NonNull::new(unsafe { *begin.add(index) }) else {
            continue;
        };
        if !guns.contains(&Some(gun)) {
            guns[next] = Some(gun);
            next += 1;
        }
    }
    guns
}

unsafe fn skip_interpolation(weapon: &mut WeaponBase) {
    let count = unsafe { gun::guns(weapon) }.count();
    for index in 0..count {
        if let Ok(index) = i32::try_from(index) {
            unsafe { weapon.SkipRenderInterpolationForOneFrame(index) };
        }
    }
}
