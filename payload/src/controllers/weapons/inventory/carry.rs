//! The carry set: the weapons the player carries beyond the inventory's slots, each held by a
//! strong reference of the mod's and docked on a socket of the body. The engine side is in
//! `docs/engine/gameplay/weapon-ownership-and-despawn.md`.
//!
//! A carried weapon is a weapon taken into the player's keeping without a slot: its holder is the
//! player and it is holstered, as a slot's weapon on the body is, but no inventory holds it, so
//! nothing of the game's places it, shows or hides it with the character, or lists it on the HUD.
//! The mod does the first two:
//!
//! - it places each carried weapon after the inventory's own placement
//!   ([`Inventory::UpdateAttachmentTransforms`], from the hold's detour), through the weapon's
//!   placement virtual ([`WeaponBase::UpdateAttachmentTransforms`]) with its guns' holstered
//!   attach-bone entries naming the socket's bone, and then moves the guns by the socket's offset
//!   in the character's frame ([`place`]);
//! - it shows or hides the carried weapons whenever the inventory's weapons are
//!   ([`Inventory::UpdateWeaponVisibility`], detoured).
//!
//! A weapon comes into the set from a slot by [`Inventory::RemoveWeapon`], which takes it out of
//! the slot without dropping it ([`carry_from_slot`]), and goes back into a slot by
//! [`Inventory::AddWeapon`] ([`promote`]), whose occupant comes into the set first so nothing is
//! dropped.
//!
//! The placement and visibility detours run on the simulation's threads for the local player's
//! inventory only, and take the set's lock; the input tick changes the set only outside the world
//! update, and makes no engine call while holding the lock.

use std::{
    fmt,
    sync::atomic::{AtomicUsize, Ordering},
};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    character::{
        character::Character,
        inventory::{EquipMode, Inventory, WeaponSlot},
    },
    equipment::weapon::{WeaponBase, WeaponState},
    types::math::Matrix4,
    ui::hud_ui::HUDUI,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    hands::ik,
    weapons::{
        hold::{refs::Strong, steal},
        holster::slot_label,
        inventory::{
            engine::{self as inventory_engine, kind_of, shared_view, strong_to},
            registry::{ObjectKey, Seen},
            sockets::SocketId,
        },
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_WEAPON_VISIBILITY_BINDER)
}

/// Why a weapon did not come into the carry set or leave it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CarryRefusal {
    /// The slot holds no weapon.
    EmptySlot,
    /// The weapon is not one the inventory carries.
    Unsupported,
    /// The weapon did not end up holstered on the player, or no longer lives.
    NotDone,
    /// The weapon is not in the carry set.
    NotCarried,
    /// The character's animation forbids drawing now.
    AnimationForbids,
}

impl fmt::Display for CarryRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            CarryRefusal::EmptySlot => "the slot holds no weapon",
            CarryRefusal::Unsupported => "the weapon is not one the inventory carries",
            CarryRefusal::NotDone => "the weapon is not holstered on the player afterwards",
            CarryRefusal::NotCarried => "the weapon is not carried",
            CarryRefusal::AnimationForbids => "the animation forbids drawing",
        };
        write!(f, "carry: {why}")
    }
}

impl std::error::Error for CarryRefusal {}

/// The carried weapons and their sockets, as the registry sees them.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn seen() -> Vec<(Seen, SocketId)> {
    CARRY
        .lock()
        .iter()
        .filter_map(|c| {
            // SAFETY: the set's strong reference keeps the weapon alive.
            let weapon = unsafe { &*c.weapon() };
            let kind = unsafe { kind_of(weapon) }?;
            Some((
                Seen {
                    object: c.object,
                    kind,
                    magazine: weapon.m_MagazineAmmo,
                },
                c.socket,
            ))
        })
        .collect()
}

/// Whether `object` is carried.
pub(crate) fn contains(object: ObjectKey) -> bool {
    CARRY.lock().iter().any(|c| c.object == object)
}

/// The socket `object` docks on, when it is carried.
pub(crate) fn home(object: ObjectKey) -> Option<SocketId> {
    CARRY
        .lock()
        .iter()
        .find(|c| c.object == object)
        .map(|c| c.socket)
}

/// Mark the carried `object` as driven in `hand`, or back on its socket with `None`; with `socket`,
/// it docks there from now on. Returns whether it is carried.
pub(crate) fn set_in_hand(object: ObjectKey, hand: Option<Hand>, socket: Option<SocketId>) -> bool {
    let mut carry = CARRY.lock();
    let Some(carried) = carry.iter_mut().find(|c| c.object == object) else {
        return false;
    };
    carried.in_hand = hand;
    if let Some(socket) = socket {
        carried.socket = socket;
    }
    true
}

/// Take `object` out of the carry set, handing over its strong reference.
pub(crate) fn release(object: ObjectKey) -> Option<Strong> {
    take(object).map(|c| c.strong)
}

/// The sockets taken, including those kept for a carried weapon in a hand.
pub(crate) fn taken() -> Vec<SocketId> {
    CARRY.lock().iter().map(|c| c.socket).collect()
}

/// The carried weapon on `socket`, not counting one a hand holds.
pub(crate) fn on_socket(socket: SocketId) -> Option<ObjectKey> {
    CARRY
        .lock()
        .iter()
        .find(|c| c.socket == socket && c.in_hand.is_none())
        .map(|c| c.object)
}

/// How many weapons are carried.
pub(crate) fn count() -> usize {
    COUNT.load(Ordering::Relaxed)
}

/// Set every carried weapon's offset from its socket's bone, in the character's frame, from the
/// settings.
pub(crate) fn set_offsets(offset_of: impl Fn(SocketId) -> Vec3) {
    for carried in CARRY.lock().iter_mut() {
        carried.offset = offset_of(carried.socket);
    }
}

/// The grip of each carried weapon on its socket, in the world: where a hand closes on it.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn grips() -> Vec<(SocketId, Vec3)> {
    CARRY
        .lock()
        .iter()
        .filter(|c| c.in_hand.is_none())
        .filter_map(|c| {
            // SAFETY: the set's strong reference keeps the weapon alive.
            let weapon = unsafe { &*c.weapon() };
            let (points, _) = unsafe { steal::gun_points(weapon) }?;
            Some((c.socket, points[0]))
        })
        .collect()
}

/// Take `slot`'s weapon out of `owner`'s inventory into the carry set, on `socket`: unequipped
/// first when it is wielded, then taken out of the slot without being dropped, so it stays
/// holstered with the player as its holder. Returns the weapon.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn carry_from_slot(
    owner: &mut Character,
    slot: WeaponSlot,
    socket: SocketId,
    offset: Vec3,
) -> Result<ObjectKey, CarryRefusal> {
    if !inventory_engine::TRACKED_SLOTS.contains(&slot) {
        return Err(CarryRefusal::Unsupported);
    }
    let weapon = unsafe { owner.m_Inventory.m_Weapons[slot as usize].px.as_mut() }
        .ok_or(CarryRefusal::EmptySlot)?;
    if matches!(
        weapon.m_State,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    ) {
        unsafe { weapon.OnUnequip() };
    }
    if weapon.m_State != WeaponState::E_WS_HOLSTERED {
        return Err(CarryRefusal::NotDone);
    }
    let strong = unsafe { strong_to(weapon) }.ok_or(CarryRefusal::NotDone)?;
    unsafe { owner.m_Inventory.RemoveWeapon(slot) };
    unsafe { skip_interpolation(weapon) };
    let object = weapon as *mut WeaponBase as ObjectKey;
    push(Carried {
        object,
        strong,
        socket,
        offset,
        in_hand: None,
    });
    unsafe { refresh_hud() };
    tracing::info!(
        target: "controllers",
        socket = socket.label(),
        "inventory: carried a weapon of the {} slot on the {}",
        slot_label(slot),
        socket.label()
    );
    Ok(object)
}

/// Move the carried weapon `object` into its slot and equip it: the slot's occupant comes into the
/// carry set on the socket the weapon leaves, so nothing is dropped, and the inventory's add puts
/// the weapon in and equips it ([`EquipMode::E_EQUIP`]). Returns the occupant that took the
/// socket, if any.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn promote(
    owner: &mut Character,
    object: ObjectKey,
) -> Result<Option<ObjectKey>, CarryRefusal> {
    if !unsafe { owner.m_Inventory.GetAnimationAllowsDrawing() } {
        return Err(CarryRefusal::AnimationForbids);
    }
    let carried = take(object).ok_or(CarryRefusal::NotCarried)?;
    // SAFETY: the set's strong reference keeps the weapon alive.
    let weapon = unsafe { &mut *carried.weapon() };
    let Some(slot) = (unsafe { inventory_engine::slot_of(weapon) }) else {
        push(carried);
        return Err(CarryRefusal::Unsupported);
    };
    let occupant = if owner.m_Inventory.m_Weapons[slot as usize].px.is_null() {
        None
    } else {
        match unsafe { carry_from_slot(owner, slot, carried.socket, carried.offset) } {
            Ok(occupant) => Some(occupant),
            Err(refusal) => {
                push(carried);
                return Err(refusal);
            }
        }
    };
    let view = shared_view(&carried.strong);
    unsafe { owner.m_Inventory.AddWeapon(&view, EquipMode::E_EQUIP) };
    let in_slot = owner.m_Inventory.m_Weapons[slot as usize].px as ObjectKey == object;
    if !in_slot {
        push(carried);
        return Err(CarryRefusal::NotDone);
    }
    // The slot holds it now; the set's reference goes.
    drop(carried);
    tracing::info!(
        target: "controllers",
        slot = slot_label(slot),
        "inventory: moved a carried weapon into its slot"
    );
    Ok(occupant)
}

/// Release every carried weapon. Returns how many there were.
pub(crate) fn release_all() -> usize {
    let all = std::mem::take(&mut *CARRY.lock());
    COUNT.store(0, Ordering::Relaxed);
    let count = all.len();
    drop(all);
    count
}

/// Let every carried weapon fall where it is and release it: its drop ([`WeaponBase::OnDrop`])
/// clears its holder, puts its bodies in the world, and gives it back its pickup, so it is a
/// weapon lying in the world again. Called when carrying is switched off.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn drop_all() {
    let all = std::mem::take(&mut *CARRY.lock());
    COUNT.store(0, Ordering::Relaxed);
    for carried in &all {
        // SAFETY: the set's strong reference keeps the weapon alive until it is released below.
        let weapon = unsafe { &mut *carried.weapon() };
        if weapon.m_State == WeaponState::E_WS_HOLSTERED {
            unsafe { weapon.OnDrop() };
        }
    }
    if !all.is_empty() {
        tracing::info!(target: "controllers", weapons = all.len(), "inventory: dropped the carried weapons");
    }
}

/// Place the carried weapons on their sockets, after `inventory`'s own placement, when it is the
/// local player's.
///
/// # Safety
///
/// `inventory` and `character` must be null or live, on the thread running the character's pass.
pub(crate) unsafe fn place(
    inventory: *mut Inventory,
    character: *const Character,
    p3: f32,
    p4: f32,
    transform_type: u32,
) {
    if COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    let local = ik::local_character();
    if local == 0 || character as usize != local {
        return;
    }
    let (Some(inventory), Some(character)) =
        (unsafe { inventory.as_ref() }, unsafe { character.as_ref() })
    else {
        return;
    };
    if inventory.m_Character as usize != local {
        return;
    }
    let to_world = Mat4::from(character.m_WorldMatrixT1);
    let carry = CARRY.lock();
    for carried in carry.iter() {
        // SAFETY: the set's strong reference keeps the weapon alive.
        let weapon = unsafe { &mut *carried.weapon() };
        if weapon.m_State != WeaponState::E_WS_HOLSTERED {
            continue;
        }
        // A weapon in a hand rides the hand's bone, which the second weapon's own writes name.
        if carried.in_hand.is_some() {
            unsafe { weapon.UpdateAttachmentTransforms(p3, p4, transform_type) };
            continue;
        }
        let bone = carried.socket.bone() as u32;
        for data in unsafe { guns_mut(weapon) } {
            if let Some(entry) = data
                .m_lBoneAttachementPerWeaponState
                .get_mut(WeaponState::E_WS_HOLSTERED as usize)
            {
                *entry = bone;
            }
        }
        unsafe { weapon.UpdateAttachmentTransforms(p3, p4, transform_type) };
        let shift = to_world.transform_vector3(carried.offset);
        if shift == Vec3::ZERO || !shift.is_finite() {
            continue;
        }
        for data in unsafe { guns_mut(weapon) } {
            let current = Mat4::from(data.m_WorldMatrixT1);
            // A placement that also set the previous matrix (the first, or one that skips its
            // render interpolation) left the two equal; the shift keeps them so.
            let copied = Mat4::from(data.m_WorldMatrixT0) == current;
            data.m_WorldMatrixT1 = Matrix4::from(translated(current, shift));
            if copied {
                data.m_WorldMatrixT0 = data.m_WorldMatrixT1;
            }
        }
    }
}

/// One carried weapon.
struct Carried {
    object: ObjectKey,
    strong: Strong,
    /// The socket it docks on, which is kept for it while a hand holds it.
    socket: SocketId,
    /// Its offset from the socket's bone, in the character's frame.
    offset: Vec3,
    /// The hand driving it, while it is out of its socket.
    in_hand: Option<Hand>,
}

impl Carried {
    fn weapon(&self) -> *mut WeaponBase {
        inventory_engine::weapon_of(&self.strong)
    }
}

static CARRY: Mutex<Vec<Carried>> = Mutex::new(Vec::new());
/// How many weapons are carried: the detours' check before any lock.
static COUNT: AtomicUsize = AtomicUsize::new(0);

fn push(carried: Carried) {
    let mut carry = CARRY.lock();
    carry.retain(|c| c.object != carried.object);
    carry.push(carried);
    COUNT.store(carry.len(), Ordering::Relaxed);
}

fn take(object: ObjectKey) -> Option<Carried> {
    let mut carry = CARRY.lock();
    let index = carry.iter().position(|c| c.object == object)?;
    let carried = carry.remove(index);
    COUNT.store(carry.len(), Ordering::Relaxed);
    Some(carried)
}

/// Rebuild the HUD's weapon list from the slots.
unsafe fn refresh_hud() {
    if let Some(hud) = unsafe { HUDUI::get() } {
        unsafe { hud.InitWeapons() };
    }
}

fn translated(matrix: Mat4, shift: Vec3) -> Mat4 {
    let mut out = matrix;
    out.w_axis += shift.extend(0.0);
    out
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

/// Skip every gun's render interpolation for one frame, so a gun that changed place does not
/// streak.
unsafe fn skip_interpolation(weapon: &mut WeaponBase) {
    let count = unsafe { gun::guns(weapon) }.count();
    for index in 0..count {
        if let Ok(index) = i32::try_from(index) {
            unsafe { weapon.SkipRenderInterpolationForOneFrame(index) };
        }
    }
}

#[detour(address = jc3gi::character::inventory::Inventory::UpdateWeaponVisibility_ADDRESS)]
fn update_weapon_visibility(inventory: *mut Inventory, visible: bool) {
    UPDATE_WEAPON_VISIBILITY
        .get()
        .unwrap()
        .call(inventory, visible);
    if COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    let local = ik::local_character();
    let ours = unsafe { inventory.as_ref() }
        .is_some_and(|i| local != 0 && i.m_Character as usize == local);
    if !ours {
        return;
    }
    for carried in CARRY.lock().iter() {
        // SAFETY: the set's strong reference keeps the weapon alive.
        unsafe { (*carried.weapon()).SetVisible(visible, false, false) };
    }
}
