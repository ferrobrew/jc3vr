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
//!   ([`Inventory::UpdateWeaponVisibility`], detoured);
//! - it ends each pose-finalization pass for them as the game ends it for the slots' weapons
//!   ([`end_pass`], from the hold's detour of [`Inventory::UpdateInventoryPostFinalizePose`]):
//!   a skip of the render interpolation lasts until then, and no inventory clears it for a carried
//!   weapon, so without it the weapon would be drawn at its newest placement every frame, a tick
//!   ahead of the body it rides.
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
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    character::{
        character::Character,
        inventory::{EquipMode, Inventory, WeaponSlot},
    },
    equipment::weapon::{ModelAttachementWeaponComponent, WeaponBase, WeaponState},
    game::GameObject,
    types::math::Matrix4,
    ui::hud_ui::HUDUI,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    hands::ik,
    loadout::Socket,
    weapons::{
        dual_wield::single,
        hold::{refs::Strong, steal},
        holster::slot_label,
        inventory::{
            engine::{self as inventory_engine, kind_of, shared_view, strong_to},
            persist::Record,
            registry::{ObjectKey, Seen},
            sockets,
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
pub(crate) unsafe fn seen() -> Vec<(Seen, Socket)> {
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

/// The carried weapons' objects.
pub(crate) fn objects() -> Vec<ObjectKey> {
    CARRY.lock().iter().map(|c| c.object).collect()
}

/// Whether `object` is carried.
pub(crate) fn contains(object: ObjectKey) -> bool {
    CARRY.lock().iter().any(|c| c.object == object)
}

/// The socket `object` docks on, when it is carried.
pub(crate) fn home(object: ObjectKey) -> Option<Socket> {
    CARRY
        .lock()
        .iter()
        .find(|c| c.object == object)
        .map(|c| c.socket)
}

/// Mark the carried `object` as driven in `hand`, or back on its socket with `None`; with `socket`,
/// it docks there from now on. Returns whether it is carried.
pub(crate) fn set_in_hand(object: ObjectKey, hand: Option<Hand>, socket: Option<Socket>) -> bool {
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
pub(crate) fn taken() -> Vec<Socket> {
    CARRY.lock().iter().map(|c| c.socket).collect()
}

/// The carried weapon on `socket`, not counting one a hand holds.
pub(crate) fn on_socket(socket: Socket) -> Option<ObjectKey> {
    CARRY
        .lock()
        .iter()
        .find(|c| c.socket == socket && c.in_hand.is_none())
        .map(|c| c.object)
}

/// Hide the carried weapons and take their grab volumes away while a mission holds the player's
/// weapons, or show them again with `false`. Returns whether that changed anything.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn set_stashed(stashed: bool) -> bool {
    if STASHED.swap(stashed, Ordering::Relaxed) == stashed {
        return false;
    }
    for carried in CARRY.lock().iter() {
        // SAFETY: the set's strong reference keeps the weapon alive.
        let weapon = unsafe { &mut *carried.weapon() };
        if weapon.m_State == WeaponState::E_WS_HOLSTERED {
            unsafe { weapon.SetVisible(!stashed, !stashed, false) };
        }
    }
    true
}

/// Whether the carried weapons are stashed for a mission.
pub(crate) fn stashed() -> bool {
    STASHED.load(Ordering::Relaxed)
}

/// How many weapons are carried.
pub(crate) fn count() -> usize {
    COUNT.load(Ordering::Relaxed)
}

/// Set every carried weapon's offset from its socket's bone, in the character's frame, from the
/// settings.
pub(crate) fn set_offsets(offset_of: impl Fn(Socket) -> Vec3) {
    for carried in CARRY.lock().iter_mut() {
        carried.offset = offset_of(carried.socket);
    }
}

/// The grip of each carried weapon on its socket, in the world: where a hand closes on it.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn grips() -> Vec<(Socket, Vec3)> {
    if stashed() {
        return Vec::new();
    }
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
    socket: Socket,
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

/// Take the weapon `strong` keeps alive, which no inventory holds, into the carry set on `socket`:
/// the weapon's pickup ([`WeaponBase::OnPickup`]) makes the player its holder and holsters it, and
/// the HUD's weapon list, which the pickup shows the weapon in, is rebuilt from the slots.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn carry_loose(
    owner: &mut Character,
    strong: Strong,
    socket: Socket,
    offset: Vec3,
) -> Result<ObjectKey, CarryRefusal> {
    let pointer = inventory_engine::weapon_of(&strong);
    // SAFETY: the strong reference keeps the weapon alive.
    let weapon = unsafe { &mut *pointer };
    if unsafe { inventory_engine::slot_of(weapon) }
        .is_none_or(|slot| !inventory_engine::TRACKED_SLOTS.contains(&slot))
    {
        return Err(CarryRefusal::Unsupported);
    }
    let holder = inventory_engine::player_object(owner) as *mut GameObject;
    unsafe { weapon.OnPickup(holder) };
    if weapon.m_State != WeaponState::E_WS_HOLSTERED
        || weapon.m_Holder.px as usize != inventory_engine::player_object(owner)
    {
        return Err(CarryRefusal::NotDone);
    }
    unsafe { skip_interpolation(weapon) };
    let object = pointer as ObjectKey;
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
        "inventory: took a weapon into the carry set on the {}",
        socket.label()
    );
    Ok(object)
}

/// The carried weapons as the save sidecar records them: each one's type, magazine, and socket.
pub(crate) fn records() -> Vec<Record> {
    CARRY
        .lock()
        .iter()
        .filter_map(|c| {
            // SAFETY: the set's strong reference keeps the weapon alive.
            let weapon = unsafe { &*c.weapon() };
            let kind = unsafe { kind_of(weapon) }?;
            Some(Record {
                tag_hash: kind.tag_hash,
                magazine: weapon.m_MagazineAmmo,
                socket: c.socket,
                single: single::is(c.object),
            })
        })
        .collect()
}

/// Move the carried weapon `object` into its slot and equip it: the slot's occupant comes into the
/// carry set first, on the socket the weapon leaves (or its own hip, for two single pistols), so
/// nothing is dropped, and the inventory's add puts the weapon in and equips it
/// ([`EquipMode::E_EQUIP`]). Returns the occupant that took a socket, if any.
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
    unsafe { into_slot(owner, object, EquipMode::E_EQUIP) }
}

/// Move the carried weapon `object` into its empty slot without equipping it: it stays holstered
/// on the body, and the HUD and the weapon wheel list it. Refused when the slot holds a weapon.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn promote_unequipped(
    owner: &mut Character,
    object: ObjectKey,
) -> Result<(), CarryRefusal> {
    // SAFETY: a carried weapon is kept alive by the set; one that is not is refused below.
    let slot = contains(object)
        .then(|| unsafe { inventory_engine::slot_of(&*(object as *const WeaponBase)) })
        .flatten()
        .ok_or(CarryRefusal::NotCarried)?;
    if !owner.m_Inventory.m_Weapons[slot as usize].px.is_null() {
        return Err(CarryRefusal::NotDone);
    }
    unsafe { into_slot(owner, object, EquipMode::E_DONT_EQUIP) }.map(|_| ())
}

/// Move the carried weapon `object` into its slot with `mode`, the slot's occupant coming into
/// the set first: on the socket the weapon leaves, or, when both are single pistols, on the
/// occupant's own hip ([`single::promote_socket`]).
unsafe fn into_slot(
    owner: &mut Character,
    object: ObjectKey,
    mode: EquipMode,
) -> Result<Option<ObjectKey>, CarryRefusal> {
    let carried = take(object).ok_or(CarryRefusal::NotCarried)?;
    // SAFETY: the set's strong reference keeps the weapon alive.
    let weapon = unsafe { &mut *carried.weapon() };
    let Some(slot) = (unsafe { inventory_engine::slot_of(weapon) }) else {
        push(carried);
        return Err(CarryRefusal::Unsupported);
    };
    let occupant_object = owner.m_Inventory.m_Weapons[slot as usize].px as ObjectKey;
    let occupant = if occupant_object == 0 {
        single::entered_slot(object, carried.socket);
        None
    } else {
        let taken = taken();
        let socket = single::promote_socket(object, carried.socket, occupant_object, |s| {
            !taken.contains(&s)
        })
        .unwrap_or(carried.socket);
        let offset = if socket == carried.socket {
            carried.offset
        } else {
            Vec3::ZERO
        };
        match unsafe { carry_from_slot(owner, slot, socket, offset) } {
            Ok(occupant) => Some(occupant),
            Err(refusal) => {
                push(carried);
                return Err(refusal);
            }
        }
    };
    let view = shared_view(&carried.strong);
    unsafe { owner.m_Inventory.AddWeapon(&view, mode) };
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
    STASHED.store(false, Ordering::Relaxed);
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
    STASHED.store(false, Ordering::Relaxed);
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
        let bone = sockets::bone(carried.socket) as u32;
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

/// End the pose-finalization pass for the carried weapons of `inventory`, when it is the local
/// player's: clear each gun's and each model part's skip of the render interpolation, as
/// [`WeaponBase::UpdatePostFinalizePose`] does for the slots' weapons. A carried weapon came into
/// the set with its interpolation skipped, so that it does not streak from the slot's bone to the
/// socket, and the skip holds until cleared.
///
/// # Safety
///
/// `inventory` must be null or live, on the thread running the character's pass.
pub(crate) unsafe fn end_pass(inventory: *mut Inventory) {
    if COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    let local = ik::local_character();
    let ours = unsafe { inventory.as_ref() }
        .is_some_and(|i| local != 0 && i.m_Character as usize == local);
    if !ours {
        return;
    }
    let carry = CARRY.lock();
    for carried in carry.iter() {
        // SAFETY: the set's strong reference keeps the weapon alive.
        let weapon = unsafe { &mut *carried.weapon() };
        for data in unsafe { guns_mut(weapon) } {
            data.m_SkipRenderInterpolation = false;
        }
        for component in unsafe { weapon.m_WeaponComponents.as_slice() } {
            let Some(component) = (unsafe { component.px.as_mut() }) else {
                continue;
            };
            // SAFETY: the component belongs to the live weapon; the test also takes the barrel
            // recoil and spin parts, which derive from the model part.
            if unsafe { component.IsType(ModelAttachementWeaponComponent::TYPE_ID()) } {
                let part =
                    unsafe { &mut *(component as *mut _ as *mut ModelAttachementWeaponComponent) };
                part.m_SkipRenderInterpolation = false;
            }
        }
    }
}

/// One carried weapon.
struct Carried {
    object: ObjectKey,
    strong: Strong,
    /// The socket it docks on, which is kept for it while a hand holds it.
    socket: Socket,
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
/// Whether the carried weapons are stashed for a mission.
static STASHED: AtomicBool = AtomicBool::new(false);

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
    let visible = visible && !stashed();
    for carried in CARRY.lock().iter() {
        // SAFETY: the set's strong reference keeps the weapon alive.
        unsafe { (*carried.weapon()).SetVisible(visible, false, false) };
    }
}
