//! The reload's engine reads, calls, and detours.
//!
//! The reads and calls run on the input tick, on the main thread outside the world update, and
//! take the weapon by address so that nothing holds a borrow of it across them. The detours run on
//! the threads that update the weapons and characters; they read only [`gate`]'s published state.
//!
//! - [`Character::StartReloadingWieldingWeapon`]: every animated reload start, refused while the
//!   weapon is reloaded by hand.
//! - [`SniperWeapon::UpdateFromUserPostSim_Parallel`]: the bolt rifle's update, run with its
//!   bolt-action setting cleared while the bolt is worked by hand, so it never queues the bolt act;
//!   the rifle's own bolt lock ([`SniperWeapon::m_HasFiredThisInput`]) keeps it from firing until
//!   the worked bolt clears it.
//! - [`DoActWeaponComponent::Fire_Start`]: the pump's per-shot act, skipped while the pump is
//!   worked by hand.

use std::sync::OnceLock;

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    animation::symbol_table::EventIdSymbolTable,
    character::{
        character::{Character, WeaponInput},
        inventory::Inventory,
    },
    equipment::weapon::{
        AmmunitionType, DoActWeaponComponent, ModelAttachementWeaponComponent, ReloadType,
        SniperWeapon, WeaponBase, WeaponComponentType, WeaponData, WeaponInputs,
    },
    types::math::Vector3,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::{
        aim::gun,
        frame::{Hand, Pose},
        hands::{ik::TickTarget, late},
        weapons::reload::{chamber::ActionKind, dual::Transfer, frames, gate, loose::EjectRounds},
    },
    hooks::character::acts::resolve_act_id,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&START_RELOADING_WIELDING_WEAPON_BINDER)
        .with_static_binder(&SNIPER_UPDATE_FROM_USER_POST_SIM_PARALLEL_BINDER)
        .with_static_binder(&DO_ACT_FIRE_START_BINDER)
}

/// What the reload reads of a weapon each tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Reading {
    /// The weapon's class, as its raw value.
    pub class: i32,
    pub name_hash: u32,
    /// The magazine size; negative means no limit.
    pub capacity: i32,
    pub magazine: i32,
    /// The game's magazine-out flag.
    pub out: bool,
    /// The weapon has unlimited magazines or an untimed reload.
    pub exempt: bool,
    /// The weapon's user has unlimited ammunition.
    pub unlimited: bool,
}

/// Read `weapon`.
///
/// # Safety
///
/// `weapon` must be live; main thread, outside the world update.
pub unsafe fn read(weapon: &WeaponBase) -> Reading {
    // Read as raw values: an unset class or reload type is out of its enum's range.
    let class = unsafe { (&raw const weapon.m_Class).cast::<i32>().read() };
    let reload_type = unsafe { (&raw const weapon.m_ReloadType).cast::<i32>().read() };
    let ammo_type = unsafe { (&raw const weapon.m_AmmunitionType).cast::<i32>().read() };
    let capacity = unsafe { weapon.GetMagazineSize() };
    Reading {
        class,
        name_hash: weapon.m_WeaponNameHash,
        capacity,
        magazine: weapon.m_MagazineAmmo,
        out: weapon.m_WeaponIsWithoutMagazine,
        exempt: reload_type == ReloadType::E_RELOAD_NO_ANIMATION as i32
            || ammo_type == AmmunitionType::AMMO_UNLIMITED as i32
            || ammo_type < 0
            || ammo_type >= AmmunitionType::NOF_AMMO_TYPES as i32,
        unlimited: unsafe { weapon.HasUnlimitedAmmo() },
    }
}

/// The reserve of `weapon`'s ammunition in its user's inventory; zero without a user.
///
/// # Safety
///
/// As [`read`], with `weapon`'s ammunition type one [`read`] accepted.
pub unsafe fn reserve(weapon: &WeaponBase) -> i32 {
    let Some(user) = (unsafe { user(weapon) }) else {
        return 0;
    };
    unsafe { user.m_Inventory.GetAmmoCount(weapon.m_AmmunitionType) }
}

/// The index of `weapon`'s gun that rides `hand`.
///
/// # Safety
///
/// As [`read`].
pub unsafe fn gun_index(weapon: &WeaponBase, hand: Hand) -> Option<usize> {
    unsafe { gun::guns(weapon) }.position(|gun| {
        gun.and_then(|g| {
            g.m_lBoneAttachementPerWeaponState
                .get(weapon.m_State as usize)
        })
        .and_then(|&bone| late::attach_hand(bone))
            == Some(hand)
    })
}

/// Gun `index` of `weapon` relative to the wrist, measured against the wrist target `target` its
/// placement followed. `None` unless the target was reached at full weight.
///
/// # Safety
///
/// As [`read`].
pub unsafe fn measure_gun(
    weapon: &WeaponBase,
    index: usize,
    target: TickTarget,
    world_scale: f32,
) -> Option<Pose> {
    const FULL_WEIGHT: f32 = 0.999;
    if target.weight < FULL_WEIGHT {
        return None;
    }
    let gun = unsafe { gun::guns(weapon) }.nth(index).flatten()?;
    frames::gun_in_wrist(Mat4::from(gun.m_WorldMatrixT1), target.wrist, world_scale)
}

/// Drop the magazine of gun `index` of the weapon at `address` as the game's throw-away-magazine
/// message does: the spare prop thrown from the magazine part (hiding it) or from the gun, the next
/// spare requested, and, with `set_out`, the magazine-out flag set.
///
/// # Safety
///
/// `address` must be the live weapon read this tick; main thread, outside the world update.
pub unsafe fn drop_magazine(address: usize, index: usize, set_out: bool, loose: bool) {
    let Some(weapon) = (unsafe { (address as *mut WeaponBase).as_mut() }) else {
        return;
    };
    let character = weapon.m_User[0] as *mut Character;
    let part = unsafe { magazine_part(weapon) }
        .map_or(WeaponData::NO_MAGAZINE_MODEL_NAME_HASH, |p| p.m_NameHash);
    // A magazine that stays in the world takes the gun's rounds with it; a split pistol's go back
    // to the reserve through the split, so its magazine drops empty.
    let rounds = if set_out {
        weapon.m_MagazineAmmo.max(0)
    } else {
        0
    };
    let Some(gun) = (unsafe { gun_mut(weapon, index) }) else {
        return;
    };
    let mut throw = || unsafe { gun.ThrowAwayMagazineObject(character, part) };
    let thrown = if loose {
        EjectRounds::with(rounds, throw)
    } else {
        throw()
    };
    if thrown {
        unsafe { gun.SpawnMagazineObject() };
    }
    if loose && set_out && thrown {
        unsafe { weapon.SetMagazineAmmo(0) };
    }
    if set_out {
        weapon.m_WeaponIsWithoutMagazine = true;
    }
}

/// Seat a magazine of `rounds` from the world into the weapon at `address`: the rounds still
/// counted in the weapon go back to the reserve, the magazine holds the seated magazine's rounds
/// (up to its size), and the magazine part shows again.
///
/// # Safety
///
/// As [`drop_magazine`].
pub unsafe fn load_rounds(address: usize, rounds: i32) {
    let Some(weapon) = (unsafe { (address as *mut WeaponBase).as_mut() }) else {
        return;
    };
    let capacity = unsafe { weapon.GetMagazineSize() };
    let rounds = if capacity >= 0 {
        rounds.clamp(0, capacity)
    } else {
        rounds.max(0)
    };
    let leftover = weapon.m_MagazineAmmo.max(0);
    let unlimited = unsafe { weapon.HasUnlimitedAmmo() };
    if leftover > 0
        && !unlimited
        && let Some(user) = unsafe { user_mut(weapon) }
    {
        let ammo_type = weapon.m_AmmunitionType;
        let inventory: &mut Inventory = &mut user.m_Inventory;
        let reserve = unsafe { inventory.GetAmmoCount(ammo_type) };
        unsafe { inventory.SetAmmoCount(ammo_type, reserve + leftover, true) };
    }
    unsafe { weapon.SetMagazineAmmo(rounds) };
    weapon.m_KeepReloading = false;
    weapon.m_WeaponIsWithoutMagazine = false;
    unsafe { show_magazine(weapon) };
}

/// The magazine spawn tag of gun `index` of the weapon at `address`.
///
/// # Safety
///
/// As [`drop_magazine`].
pub unsafe fn magazine_tag(address: usize, index: usize) -> Option<i32> {
    let weapon = unsafe { (address as *mut WeaponBase).as_mut() }?;
    let gun = unsafe { gun_mut(weapon, index) }?;
    Some(gun.m_MagazineUniqueTag)
}

/// The local player character's world matrix.
///
/// # Safety
///
/// The main thread, outside the world update.
pub unsafe fn local_player_matrix() -> Option<Mat4> {
    let player = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    Some(Mat4::from(player.m_WorldMatrixT1))
}

/// Load a whole magazine into the weapon at `address` through its own reload, show its magazine
/// part again, and end a shell-by-shell reload's loop. Returns whether it loaded.
///
/// # Safety
///
/// As [`drop_magazine`].
pub unsafe fn load_full(address: usize) -> bool {
    let Some(weapon) = (unsafe { (address as *mut WeaponBase).as_mut() }) else {
        return false;
    };
    let loaded = unsafe { weapon.Reload() };
    weapon.m_KeepReloading = false;
    weapon.m_WeaponIsWithoutMagazine = false;
    unsafe { show_magazine(weapon) };
    loaded
}

/// Load one round into the weapon at `address`, taking it from the reserve unless the ammunition
/// is unlimited. Returns whether it loaded.
///
/// # Safety
///
/// As [`drop_magazine`].
pub unsafe fn load_one(address: usize) -> bool {
    let Some(weapon) = (unsafe { (address as *mut WeaponBase).as_mut() }) else {
        return false;
    };
    let capacity = unsafe { weapon.GetMagazineSize() };
    if weapon.m_MagazineAmmo >= capacity {
        return false;
    }
    let unlimited = unsafe { weapon.HasUnlimitedAmmo() };
    let reserve = unsafe { reserve(weapon) };
    if !unlimited && reserve <= 0 {
        return false;
    }
    let ammo = weapon.m_MagazineAmmo.max(0) + 1;
    unsafe { weapon.SetMagazineAmmo(ammo) };
    if !unlimited && let Some(user) = unsafe { user_mut(weapon) } {
        let ammo_type = weapon.m_AmmunitionType;
        unsafe { user.m_Inventory.SetAmmoCount(ammo_type, reserve - 1, true) };
    }
    weapon.m_WeaponIsWithoutMagazine = false;
    true
}

/// Apply a split magazine's transfer to the weapon at `address`: its magazine and its reserve.
///
/// # Safety
///
/// As [`drop_magazine`].
pub unsafe fn transfer(address: usize, transfer: Transfer) {
    let Some(weapon) = (unsafe { (address as *mut WeaponBase).as_mut() }) else {
        return;
    };
    unsafe { weapon.SetMagazineAmmo(transfer.magazine.max(0)) };
    if transfer.reserve_delta != 0
        && let Some(user) = unsafe { user_mut(weapon) }
    {
        let ammo_type = weapon.m_AmmunitionType;
        let inventory: &mut Inventory = &mut user.m_Inventory;
        let reserve = unsafe { inventory.GetAmmoCount(ammo_type) };
        unsafe {
            inventory.SetAmmoCount(ammo_type, (reserve + transfer.reserve_delta).max(0), true)
        };
    }
    unsafe { show_magazine(weapon) };
}

/// Tell the weapon at `address` that its action was worked by hand: the bolt rifle's lock cleared
/// and, with `clip`, the gun's own bolt or pump clip played.
///
/// # Safety
///
/// As [`drop_magazine`].
pub unsafe fn cycled(address: usize, kind: ActionKind, clip: bool) {
    let Some(weapon) = (unsafe { (address as *mut WeaponBase).as_mut() }) else {
        return;
    };
    let act = match kind {
        ActionKind::Bolt => {
            if is_sniper(weapon) {
                let sniper = unsafe { &mut *(weapon as *mut WeaponBase).cast::<SniperWeapon>() };
                sniper.m_HasFiredThisInput = false;
            }
            sniper_bolt_act()
        }
        ActionKind::Pump => pump_act(),
        _ => None,
    };
    let Some(act) = act.filter(|_| clip) else {
        return;
    };
    let act = act as u32;
    let count = unsafe { gun::guns(weapon) }.count();
    for index in 0..count {
        if let Some(gun) = unsafe { gun_mut(weapon, index) } {
            unsafe { gun.DoAct(&act) };
        }
    }
}

/// The address of gun `index` of the weapon at `address`, whose skeleton the action's bone rides.
///
/// # Safety
///
/// As [`drop_magazine`].
pub unsafe fn gun_address(address: usize, index: usize) -> Option<usize> {
    unsafe { gun_mut(address as *const WeaponBase, index) }
        .map(|gun| gun as *mut WeaponData as usize)
}

/// The local player's reload start, refused while the weapon is reloaded by hand (see [`gate`]).
#[detour(address = jc3gi::character::character::Character::StartReloadingWieldingWeapon_ADDRESS)]
fn start_reloading_wielding_weapon(character: *mut Character) {
    let reload_input = unsafe { character.as_ref() }.map_or(0.0, |c| {
        c.m_WeaponInputs[WeaponInput::E_INPUT_RELOAD as usize]
    });
    let local = crate::controllers::hands::ik::local_character();
    if gate::stops_reload(character as usize, local, reload_input) {
        return;
    }
    START_RELOADING_WIELDING_WEAPON
        .get()
        .unwrap()
        .call(character);
}

/// The bolt rifle's update, with its bolt-action setting cleared for the call while the bolt is
/// worked by hand.
#[detour(address = jc3gi::equipment::weapon::SniperWeapon::UpdateFromUserPostSim_Parallel_ADDRESS)]
fn sniper_update_from_user_post_sim_parallel(
    sniper: *mut SniperWeapon,
    dt: f32,
    aim_position: *const Vector3,
    input: *const WeaponInputs,
) {
    let original = || {
        SNIPER_UPDATE_FROM_USER_POST_SIM_PARALLEL
            .get()
            .unwrap()
            .call(sniper, dt, aim_position, input)
    };
    let local =
        unsafe { sniper.cast::<WeaponBase>().as_ref() }.is_some_and(|w| w.m_IsUserLocalPlayer);
    if !local || !gate::manual_action(sniper as usize) {
        return original();
    }
    let Some(s) = (unsafe { sniper.as_mut() }) else {
        return original();
    };
    let bolt_action = std::mem::replace(&mut s.m_BoltAction, false);
    original();
    if let Some(s) = unsafe { sniper.as_mut() } {
        s.m_BoltAction = bolt_action;
    }
}

/// The do-act component's per-shot act, skipped for the local player's weapon while its pump is
/// worked by hand.
#[detour(address = jc3gi::equipment::weapon::DoActWeaponComponent::Fire_Start_ADDRESS)]
fn do_act_fire_start(component: *mut DoActWeaponComponent, weapon_data: *mut WeaponData) {
    let owner = unsafe { component.as_ref() }.map_or(std::ptr::null_mut(), |c| c.m_Owner);
    let local = unsafe { owner.as_ref() }.is_some_and(|w| w.m_IsUserLocalPlayer);
    if local && gate::manual_action(owner as usize) {
        return;
    }
    DO_ACT_FIRE_START
        .get()
        .unwrap()
        .call(component, weapon_data);
}

/// The weapon's user.
unsafe fn user(weapon: &WeaponBase) -> Option<&Character> {
    // `m_User` is a `weak_ptr<CCharacter>`; its first word is the character.
    unsafe { (weapon.m_User[0] as *const Character).as_ref() }
}

/// The weapon's user, mutably: the character is its own object, not part of the weapon.
unsafe fn user_mut<'a>(weapon: *const WeaponBase) -> Option<&'a mut Character> {
    unsafe { (weapon.as_ref()?.m_User[0] as *mut Character).as_mut() }
}

/// Gun `index` of `weapon`, mutably: each gun is its own object, not part of the weapon.
unsafe fn gun_mut<'a>(weapon: *const WeaponBase, index: usize) -> Option<&'a mut WeaponData> {
    let guns = unsafe { &weapon.as_ref()?.m_lWeaponData };
    if guns.m_Begin.is_null() {
        return None;
    }
    let count = (guns.m_End as usize - guns.m_Begin as usize) / size_of::<*mut WeaponData>();
    if index >= count {
        return None;
    }
    unsafe { (*guns.m_Begin.add(index)).as_mut() }
}

/// The weapon's magazine part: the model part the game's reload hides and shows, by name.
unsafe fn magazine_part<'a>(
    weapon: *const WeaponBase,
) -> Option<&'a mut ModelAttachementWeaponComponent> {
    unsafe { model_parts(weapon) }.find(|p| {
        matches!(
            p.m_NameHash,
            ModelAttachementWeaponComponent::MAGAZINE_NAME_HASH
                | ModelAttachementWeaponComponent::ROCKET_POD_NAME_HASH
                | ModelAttachementWeaponComponent::CARTRIDGE_FULL_NAME_HASH
        )
    })
}

/// The weapon's plain model parts, as the game's own part lookup takes them: by component kind.
/// Each component is its own object, not part of the weapon.
unsafe fn model_parts<'a>(
    weapon: *const WeaponBase,
) -> impl Iterator<Item = &'a mut ModelAttachementWeaponComponent> {
    unsafe { weapon.as_ref() }
        .map_or(&[][..], |w| unsafe { w.m_WeaponComponents.as_slice() })
        .iter()
        .filter_map(|c| unsafe { c.px.as_mut() })
        .filter(|c| c.m_Type == WeaponComponentType::E_WEAPON_COMPONENT_MODEL_ATTACHEMENT)
        .map(|c| unsafe { &mut *(c as *mut _ as *mut ModelAttachementWeaponComponent) })
}

/// Show or hide the magazine part of the weapon at `address`.
///
/// # Safety
///
/// As [`drop_magazine`].
pub unsafe fn show_magazine_part(address: usize, shown: bool) {
    let Some(weapon) = (unsafe { (address as *const WeaponBase).as_ref() }) else {
        return;
    };
    if let Some(part) = unsafe { magazine_part(weapon) }
        && part.m_Enabled != shown
    {
        unsafe { part.Enable(shown) };
    }
}

/// Show the weapon's magazine part, as the reload clips' enable message does.
unsafe fn show_magazine(weapon: &WeaponBase) {
    if let Some(part) = unsafe { magazine_part(weapon) }
        && !part.m_Enabled
    {
        unsafe { part.Enable(true) };
    }
}

fn is_sniper(weapon: &WeaponBase) -> bool {
    let class = unsafe { (&raw const weapon.m_Class).cast::<i32>().read() };
    class == jc3gi::equipment::weapon::WeaponClass::E_WEAPON_CLASS_SNIPER as i32
}

/// The gun-side bolt act's runtime id, once the symbol table is live.
fn sniper_bolt_act() -> Option<i32> {
    static ID: OnceLock<i32> = OnceLock::new();
    cached(&ID, EventIdSymbolTable::ACT_SNIPER_BOLT_ACTION)
}

/// The gun-side pump act's runtime id, once the symbol table is live.
fn pump_act() -> Option<i32> {
    static ID: OnceLock<i32> = OnceLock::new();
    cached(&ID, EventIdSymbolTable::ACT_SHOTGUN_PUMP)
}

fn cached(cell: &OnceLock<i32>, name: &std::ffi::CStr) -> Option<i32> {
    if let Some(id) = cell.get() {
        return Some(*id);
    }
    let id = resolve_act_id(name)?;
    Some(*cell.get_or_init(|| id))
}
