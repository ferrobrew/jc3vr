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
//!   ([`Inventory::UpdateAttachmentTransforms`], from the hold's detour), where the loadout table
//!   has it ([`crate::controllers::loadout::live::place_of`]): on a socket, through the weapon's
//!   placement virtual ([`WeaponBase::UpdateAttachmentTransforms`]) with its guns' holstered
//!   attach-bone entries naming the socket's bone, and then moved by the socket's offset in the
//!   character's frame; in a hand, on the hand's bone that the second weapon writes ([`place`]);
//! - it shows or hides the carried weapons the table has on the body whenever the inventory's
//!   weapons are ([`Inventory::UpdateWeaponVisibility`], detoured);
//! - it ends each pose-finalization pass for them as the game ends it for the slots' weapons
//!   ([`end_pass`], from the hold's detour of [`Inventory::UpdateInventoryPostFinalizePose`]):
//!   a skip of the render interpolation lasts until then, and no inventory clears it for a carried
//!   weapon, so without it the weapon would be drawn at its newest placement every frame, a tick
//!   ahead of the body it rides.
//!
//! The loadout table says where each weapon is, and the slots follow it
//! ([`crate::controllers::weapons::inventory::reconcile`]): a table weapon the reconcile takes out
//! of its slot comes into the set ([`keep`]), and one it puts into a slot leaves it ([`release`]).
//! So the set holds every table weapon on the body that no slot holds, and which socket each sits
//! on is the table's. A slot's weapon the table has on a socket other than the one the game docks
//! its slot on is placed there too ([`place_slots`] and [`place`]).
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
    character::{character::Character, inventory::Inventory},
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
    loadout::{Place, Socket, live},
    weapons::{
        dual_wield::single,
        hold::refs::Strong,
        inventory::{
            engine::{self as inventory_engine, ObjectKey, Seen, kind_of, strong_to},
            lost,
            persist::Record,
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
    /// The weapon is not one the inventory carries.
    Unsupported,
    /// The weapon did not end up holstered on the player, or no longer lives.
    NotDone,
}

impl fmt::Display for CarryRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            CarryRefusal::Unsupported => "the weapon is not one the inventory carries",
            CarryRefusal::NotDone => "the weapon is not holstered on the player afterwards",
        };
        write!(f, "carry: {why}")
    }
}

impl std::error::Error for CarryRefusal {}

/// The carried weapons and their sockets, as the inventory's tick reads them.
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
            let socket = match live::place_of(c.object) {
                Some(Place::Socket(socket)) => socket,
                _ => c.socket,
            };
            Some((
                Seen {
                    object: c.object,
                    kind,
                    magazine: weapon.m_MagazineAmmo,
                },
                socket,
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

/// Take `object` out of the carry set, handing over its strong reference.
pub(crate) fn release(object: ObjectKey) -> Option<Strong> {
    take(object).map(|c| c.strong)
}

/// Keep `weapon`, a table weapon on the body the reconcile takes out of its slot, alive in the set,
/// on `socket` (where it sits, or for a hand's weapon where it last sat). Returns whether it is
/// kept.
///
/// # Safety
///
/// `weapon` must be live, on the main thread outside the world update.
pub(crate) unsafe fn keep(weapon: &WeaponBase, socket: Socket) -> bool {
    let object = weapon as *const WeaponBase as ObjectKey;
    if contains(object) {
        return true;
    }
    let Some(strong) = (unsafe { strong_to(weapon) }) else {
        return false;
    };
    push(Carried {
        object,
        strong,
        socket,
    });
    true
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

/// Set each socket's offset from its bone, in the character's frame, from the settings.
pub(crate) fn set_offsets(offset_of: impl Fn(Socket) -> Vec3) {
    *OFFSETS.lock() = Socket::ALL.map(&offset_of);
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
) -> Result<ObjectKey, CarryRefusal> {
    let pointer = inventory_engine::weapon_of(&strong);
    // SAFETY: the strong reference keeps the weapon alive.
    let weapon = unsafe { &mut *pointer };
    if unsafe { inventory_engine::slot_of(weapon) }
        .is_none_or(|slot| !inventory_engine::tracks(slot))
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

/// The carried weapons as the save sidecar records them: each one's type, magazine, and socket, the
/// one the loadout table has it on, or for a weapon in a hand the one it came into the set on.
pub(crate) fn records() -> Vec<Record> {
    CARRY
        .lock()
        .iter()
        .filter_map(|c| {
            // SAFETY: the set's strong reference keeps the weapon alive.
            let weapon = unsafe { &*c.weapon() };
            let kind = unsafe { kind_of(weapon) }?;
            let socket = match live::place_of(c.object) {
                Some(Place::Socket(socket)) => socket,
                _ => c.socket,
            };
            Some(Record {
                tag_hash: kind.tag_hash,
                magazine: weapon.m_MagazineAmmo,
                socket,
                single: single::is(c.object),
            })
        })
        .collect()
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
    let offsets = *OFFSETS.lock();
    // The inventory placed its own weapons; those the table has on another socket move by its
    // offset, as a carried weapon does.
    for (weapon, socket) in unsafe { off_dock(inventory) } {
        unsafe { shift(weapon, to_world.transform_vector3(offsets[socket.index()])) };
    }
    if COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    let carry = CARRY.lock();
    for carried in carry.iter() {
        // SAFETY: the set's strong reference keeps the weapon alive.
        let weapon = unsafe { &mut *carried.weapon() };
        if weapon.m_State != WeaponState::E_WS_HOLSTERED {
            continue;
        }
        let socket = match live::place_of(carried.object) {
            Some(Place::Socket(socket)) => socket,
            // A weapon in a hand rides the hand's bone, which its arming writes.
            Some(Place::Hand(_)) => {
                unsafe { weapon.UpdateAttachmentTransforms(p3, p4, transform_type) };
                continue;
            }
            // A lost weapon brought to the feet rides the body for the tick before it falls.
            Some(Place::World(_)) if lost::falling(carried.object) => carried.socket,
            Some(Place::World(_)) | None => continue,
        };
        unsafe { write_bone(weapon, socket) };
        unsafe { weapon.UpdateAttachmentTransforms(p3, p4, transform_type) };
        unsafe { shift(weapon, to_world.transform_vector3(offsets[socket.index()])) };
    }
}

/// Write the socket's bone into the holstered entries of each of `inventory`'s weapons the table
/// has on a socket other than its slot's own dock, when `inventory` is the local player's, so the
/// inventory's own placement puts it there. Called at the hold's write points, before the
/// inventory's placement and after the game's rewrite of the tables at the end of each pass.
///
/// # Safety
///
/// `inventory` must be null or live, on the thread updating its owner.
pub(crate) unsafe fn place_slots(inventory: *mut Inventory) {
    let Some(inventory) = (unsafe { inventory.as_ref() }) else {
        return;
    };
    let local = ik::local_character();
    if local == 0 || inventory.m_Character as usize != local {
        return;
    }
    for (weapon, socket) in unsafe { off_dock(inventory) } {
        unsafe { write_bone(weapon, socket) };
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
    /// The socket it sat on when it came into the set, kept for it while a hand holds it.
    socket: Socket,
}

impl Carried {
    fn weapon(&self) -> *mut WeaponBase {
        inventory_engine::weapon_of(&self.strong)
    }
}

static CARRY: Mutex<Vec<Carried>> = Mutex::new(Vec::new());
/// Each socket's offset from its bone, in the character's frame, by [`Socket::index`].
static OFFSETS: Mutex<[Vec3; Socket::COUNT]> = Mutex::new([Vec3::ZERO; Socket::COUNT]);
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

/// Whether the game's own placement puts the slot's weapon `weapon` on `socket`: the socket it
/// docks the weapon's slot on. The game docks the pistol slot's first gun on the right hip, so a
/// single pistol, whose first gun is all it shows ([`single`]), is the game's on that hip alone,
/// while the game's pair takes both.
unsafe fn at_own_dock(weapon: &WeaponBase, socket: Socket) -> bool {
    let Some(slot) = (unsafe { inventory_engine::slot_of(weapon) }) else {
        return false;
    };
    match socket {
        Socket::Hip(side) if single::is(weapon as *const WeaponBase as usize) => {
            side == Hand::Right
        }
        _ => socket.is_native(slot),
    }
}

/// The weapons of `inventory`'s slots the table has on a socket other than their slot's own dock.
unsafe fn off_dock(inventory: &Inventory) -> Vec<(&'static mut WeaponBase, Socket)> {
    inventory
        .m_Weapons
        .iter()
        .filter_map(|slot| {
            let weapon = unsafe { slot.px.as_mut() }?;
            if weapon.m_State != WeaponState::E_WS_HOLSTERED {
                return None;
            }
            let Some(Place::Socket(socket)) = live::place_of(weapon as *const WeaponBase as usize)
            else {
                return None;
            };
            (!unsafe { at_own_dock(weapon, socket) }).then_some((weapon, socket))
        })
        .collect()
}

/// Write `socket`'s bone into `weapon`'s guns' holstered entries.
unsafe fn write_bone(weapon: &mut WeaponBase, socket: Socket) {
    let bone = sockets::bone(socket) as u32;
    for data in unsafe { guns_mut(weapon) } {
        if let Some(entry) = data
            .m_lBoneAttachementPerWeaponState
            .get_mut(WeaponState::E_WS_HOLSTERED as usize)
        {
            *entry = bone;
        }
    }
}

/// Move `weapon`'s guns by `shift`, in the world, after their placement.
unsafe fn shift(weapon: &mut WeaponBase, shift: Vec3) {
    if shift == Vec3::ZERO || !shift.is_finite() {
        return;
    }
    for data in unsafe { guns_mut(weapon) } {
        let current = Mat4::from(data.m_WorldMatrixT1);
        // A placement that also set the previous matrix (the first, or one that skips its render
        // interpolation) left the two equal; the shift keeps them so.
        let copied = Mat4::from(data.m_WorldMatrixT0) == current;
        data.m_WorldMatrixT1 = Matrix4::from(translated(current, shift));
        if copied {
            data.m_WorldMatrixT0 = data.m_WorldMatrixT1;
        }
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
        if live::place_of(carried.object).is_none() {
            continue;
        }
        // SAFETY: the set's strong reference keeps the weapon alive.
        unsafe { (*carried.weapon()).SetVisible(visible, false, false) };
    }
}
