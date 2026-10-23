//! The game's slots as a projection of the loadout table, made to match each input tick. The
//! design is the two-handed inventory's "The game's slots are a projection"
//! (`docs/mod/input/two-handed-inventory.md`), the engine side in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`.
//!
//! The table says where every weapon is; the reconcile makes the game agree, in three passes:
//!
//! - the slots ([`input_tick`]): each slot the inventory tracks holds the weapon the table's play
//!   projection puts there. A weapon that leaves its slot stays alive in the carry set, which keeps
//!   a strong reference to every table weapon off the slots
//!   ([`crate::controllers::weapons::inventory::carry`]), and goes out through
//!   [`Inventory::RemoveWeapon`], which takes it out without dropping it; one that comes in goes in
//!   through [`Inventory::AddWeapon`] without being equipped. A weapon in a slot that the table does
//!   not know is left where it is, since the game's word on ownership is final. The heavy slot
//!   stays the game's;
//! - the primary: the primary hand's weapon is wielded. Moving the primary unequips the old weapon
//!   and equips the new one ([`WeaponBase::OnUnequip`], then [`WeaponBase::OnEquip`]), with no act,
//!   which is what the draw clip's own equip message does. A draw, holster, or switch clip still
//!   playing is aborted first ([`EventIdSymbolTable::ACT_ABORT_WEAPON_PARTIAL`]), since its
//!   messages equip and unequip by slot when they fire;
//! - the hands: every other weapon a hand holds is armed there
//!   ([`crate::controllers::weapons::hold::armed`]), and a hand that no longer holds its armed
//!   weapon disarms it.
//!
//! Everything here runs on the input tick, on the main thread outside the world update.

use std::sync::atomic::{AtomicU64, Ordering};

use jc3gi::{
    animation::symbol_table::EventIdSymbolTable,
    character::{
        character::Character,
        inventory::{EquipMode, WeaponSlot},
    },
    equipment::weapon::{WeaponBase, WeaponState},
    ui::hud_ui::HUDUI,
};

use crate::{
    controllers::{
        aim::gun,
        frame::Hand,
        loadout::{LoadoutSnapshot, Place, Projection, SlotContent, Socket, StoreProjection, live},
        weapons::{
            engine,
            hold::armed,
            inventory::{
                carry,
                engine::{self as inventory_engine, ObjectKey, shared_view},
                traversal,
            },
        },
    },
    hooks::character::acts::resolve_act_id,
};

/// The reconcile's readout.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ReconcileStatus {
    /// Weapons taken out of their slots and put into them since the payload loaded.
    pub out_of_slots: u64,
    pub into_slots: u64,
    /// The primary's weapon equipped, a weapon unequipped with no primary, and clips aborted first.
    pub equips: u64,
    pub unequips: u64,
    pub aborts: u64,
    /// Weapons armed in a hand, and armed weapons made the primary's, wielded in the same hand.
    pub arms: u64,
    pub promotions: u64,
}

pub fn status() -> ReconcileStatus {
    ReconcileStatus {
        out_of_slots: OUT.load(Ordering::Relaxed),
        into_slots: IN.load(Ordering::Relaxed),
        equips: EQUIPS.load(Ordering::Relaxed),
        unequips: UNEQUIPS.load(Ordering::Relaxed),
        aborts: ABORTS.load(Ordering::Relaxed),
        arms: ARMS.load(Ordering::Relaxed),
        promotions: PROMOTIONS.load(Ordering::Relaxed),
    }
}

/// Make the game match the table as it is now: the slots, the primary, and the hands. Called each
/// input tick after the table's feed, and at once after the hands move a weapon, so the next
/// placement already follows.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn apply(owner: &mut Character) {
    let Some(snapshot) = live::snapshot_now() else {
        return;
    };
    if unsafe { slots(owner, &snapshot, &snapshot.projection) } {
        // The moves changed what the table publishes of the slots' weapons.
        unsafe { refresh_hud() };
    }
    unsafe { primary(owner, &snapshot) };
    unsafe { hands(owner, &snapshot) };
}

/// [`apply`] for the local player, when there is one.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn now() {
    if let Some(owner) = unsafe { engine::local_player() } {
        unsafe { apply(owner) };
    }
}

/// Before the game's own pickup puts a weapon into `slot`, take the slot's weapon out when the
/// table has it on the body, so the pickup does not drop or despawn it: it stays where the table
/// has it, and the next pass gives the slot to whichever weapon the projection prefers. Returns
/// whether the slot is free for the pickup.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn vacate(owner: &mut Character, slot: WeaponSlot) -> bool {
    let Some(snapshot) = live::snapshot_now() else {
        return false;
    };
    let Some(current) = (unsafe { owner.m_Inventory.m_Weapons[slot as usize].px.as_mut() }) else {
        return true;
    };
    let object = current as *mut WeaponBase as usize;
    let Some(socket) = home_of(&snapshot, object) else {
        return false;
    };
    let out = unsafe { take_out(owner, slot, current, socket) };
    if out {
        crate::controllers::weapons::inventory::count_carried_replaced();
    }
    out
}

/// Put the table back into the game's slots, as a store would write them: the table is about to
/// stop, so the game's own one-weapon model takes over with the best of what the player has in its
/// slots. The heavy slot stays the game's.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn project_back(owner: &mut Character) {
    let (Some(snapshot), Some(store)) = (live::snapshot_now(), live::store_projection_now()) else {
        return;
    };
    for hand in Hand::BOTH {
        let weapon = unsafe { armed::weapon_of(owner, hand) };
        unsafe { armed::disarm(owner, hand, weapon, true) };
    }
    if unsafe { slots(owner, &snapshot, &store.projection) } {
        unsafe { refresh_hud() };
    }
    tracing::info!(target: "controllers", "reconcile: put the table back into the game's slots");
}

/// Before the game stores its save or a mission caches the slots, give the tracked slots what the
/// store projection `store` puts there. In play the projection already keeps them so, since the two
/// differ only in the heavy slot, which stays the game's; this catches a move the hands made since
/// the last pass.
///
/// # Safety
///
/// On the game thread, outside the world update.
pub(crate) unsafe fn store(store: &StoreProjection) {
    let Some(owner) = (unsafe { engine::local_player() }) else {
        return;
    };
    let Some(snapshot) = live::snapshot_now() else {
        return;
    };
    if unsafe { slots(owner, &snapshot, &store.projection) } {
        unsafe { refresh_hud() };
    }
}

/// The slots the reconcile makes follow the table: the heavy slot and the consumables stay the
/// game's.
const SLOTS: [WeaponSlot; 3] = inventory_engine::TRACKED_SLOTS;

static OUT: AtomicU64 = AtomicU64::new(0);
static IN: AtomicU64 = AtomicU64::new(0);
static EQUIPS: AtomicU64 = AtomicU64::new(0);
static UNEQUIPS: AtomicU64 = AtomicU64::new(0);
static ABORTS: AtomicU64 = AtomicU64::new(0);
static ARMS: AtomicU64 = AtomicU64::new(0);
static PROMOTIONS: AtomicU64 = AtomicU64::new(0);

/// Make each tracked slot hold what `projection` puts there. Returns whether a slot changed.
unsafe fn slots(
    owner: &mut Character,
    snapshot: &LoadoutSnapshot,
    projection: &Projection,
) -> bool {
    let mut changed = false;
    for slot in SLOTS {
        let Some(fill) = projection.slots.iter().find(|f| f.slot == slot) else {
            continue;
        };
        let desired = match fill.content {
            SlotContent::Weapon { item, .. } => snapshot
                .items
                .iter()
                .find(|i| i.id == item)
                .and_then(|i| i.binding.address())
                .map(|a| a.get()),
            SlotContent::Empty => None,
            SlotContent::Game => continue,
        };
        let current = owner.m_Inventory.m_Weapons[slot as usize].px as usize;
        if (current != 0 && Some(current) == desired) || (current == 0 && desired.is_none()) {
            continue;
        }
        if current != 0 {
            // A weapon the table does not have on the body is the game's to keep there.
            let Some(socket) = home_of(snapshot, current) else {
                continue;
            };
            // SAFETY: the slot keeps the weapon alive.
            let weapon = unsafe { &mut *(current as *mut WeaponBase) };
            if !unsafe { take_out(owner, slot, weapon, socket) } {
                continue;
            }
            changed = true;
        }
        let Some(desired) = desired else {
            continue;
        };
        let home = home_of(snapshot, desired).unwrap_or(Socket::BackOuter);
        changed |= unsafe { put_in(owner, slot, desired, home) };
    }
    changed
}

/// Where the table has the object at `object` on the body, as the carry set keeps it while no slot
/// holds it: its socket, or for a hand's weapon the socket the body prefers for it. `None` for an
/// object the table does not have on the body.
fn home_of(snapshot: &LoadoutSnapshot, object: ObjectKey) -> Option<Socket> {
    let item = snapshot
        .items
        .iter()
        .find(|i| i.binding.address().is_some_and(|a| a.get() == object))?;
    match item.place {
        Place::Socket(socket) => Some(socket),
        Place::Hand(_) => {
            let weapon = item.kind.weapon()?;
            snapshot
                .body
                .preference(weapon)
                .next()
                .or(Some(Socket::BackOuter))
        }
        Place::World(_) => None,
    }
}

/// Take `weapon` out of `slot` into the carry set, on `socket`: unequipped first when it is
/// wielded, and disarmed when a hand has it armed, since the slot's removal does not stop its use.
/// Returns whether the slot is empty after.
unsafe fn take_out(
    owner: &mut Character,
    slot: WeaponSlot,
    weapon: &mut WeaponBase,
    socket: Socket,
) -> bool {
    let object = weapon as *mut WeaponBase as usize;
    if !unsafe { carry::keep(weapon, socket) } {
        return false;
    }
    if is_wielded(weapon) {
        unsafe { abort_clips(owner) };
        unsafe { weapon.OnUnequip() };
        UNEQUIPS.fetch_add(1, Ordering::Relaxed);
    }
    if let Some(hand) = armed::hand_of(object) {
        unsafe { armed::disarm(owner, hand, Some(&mut *weapon), true) };
    }
    if weapon.m_State != WeaponState::E_WS_HOLSTERED {
        carry::release(object);
        return false;
    }
    unsafe { owner.m_Inventory.RemoveWeapon(slot) };
    let empty = owner.m_Inventory.m_Weapons[slot as usize].px.is_null();
    if empty {
        OUT.fetch_add(1, Ordering::Relaxed);
        tracing::info!(target: "controllers", ?slot, "reconcile: took a weapon out of its slot");
    }
    empty
}

/// Put the table's weapon at `object`, which the carry set keeps, into the empty `slot` without
/// equipping it, or keep it on `home` when the slot does not take it. Returns whether the slot
/// holds it after.
unsafe fn put_in(owner: &mut Character, slot: WeaponSlot, object: ObjectKey, home: Socket) -> bool {
    if !owner.m_Inventory.m_Weapons[slot as usize].px.is_null() || !carry::contains(object) {
        return false;
    }
    if let Some(hand) = armed::hand_of(object) {
        // SAFETY: the carry set keeps the weapon alive.
        let weapon = unsafe { (object as *mut WeaponBase).as_mut() };
        unsafe { armed::disarm(owner, hand, weapon, true) };
    }
    let Some(strong) = carry::release(object) else {
        return false;
    };
    let view = shared_view(&strong);
    unsafe { owner.m_Inventory.AddWeapon(&view, EquipMode::E_DONT_EQUIP) };
    let in_slot = owner.m_Inventory.m_Weapons[slot as usize].px as usize == object;
    if !in_slot {
        // SAFETY: the strong reference keeps the weapon alive.
        let weapon = unsafe { &*inventory_engine::weapon_of(&strong) };
        unsafe { carry::keep(weapon, home) };
        return false;
    }
    IN.fetch_add(1, Ordering::Relaxed);
    tracing::info!(target: "controllers", ?slot, "reconcile: put a weapon into its slot");
    true
}

/// Make the primary hand's weapon the wielded one: unequip what the game wields and equip it, with
/// no act. With no weapon in the hands, unequip a weapon of the table's the game wields. A weapon
/// the game wields that the table does not have on the body (a grenade, the planted explosive, a
/// mounted gun) is left to the game.
unsafe fn primary(owner: &mut Character, snapshot: &LoadoutSnapshot) {
    let wielded = unsafe { gun::held_weapon(owner) }.map(|w| w as *const WeaponBase as usize);
    if wielded.is_some_and(|w| home_of(snapshot, w).is_none()) {
        return;
    }
    // Where the game's weapon updates skip the wielded weapon (a seat, the boost wingsuit), the
    // mod's update fires a gun in a hand: nothing is wielded, and the hands' weapons are armed.
    if unsafe { traversal::wield_skipped(owner) } {
        if let Some(current) = wielded {
            unsafe { abort_clips(owner) };
            // SAFETY: the table has it on the body, in a slot while wielded.
            let current = unsafe { &mut *(current as *mut WeaponBase) };
            unsafe { current.OnUnequip() };
            UNEQUIPS.fetch_add(1, Ordering::Relaxed);
            tracing::info!(target: "controllers", "reconcile: the game's update skips the wielded weapon here; unequipped it to arm it in its hand");
        }
        return;
    }
    let primary = snapshot
        .primary
        .and_then(|hand| hand_weapon(snapshot, hand));
    if wielded == primary {
        return;
    }
    let in_slot = |object: ObjectKey| {
        owner
            .m_Inventory
            .m_Weapons
            .iter()
            .any(|slot| slot.px as usize == object)
    };
    match (wielded, primary.filter(|&p| in_slot(p))) {
        (_, Some(target)) => {
            if !unsafe { owner.m_Inventory.GetAnimationAllowsDrawing() } {
                return;
            }
            unsafe { abort_clips(owner) };
            if let Some(current) = wielded {
                // SAFETY: the table has it on the body, in a slot while wielded.
                let current = unsafe { &mut *(current as *mut WeaponBase) };
                unsafe { current.OnUnequip() };
            }
            // SAFETY: the slot keeps the weapon alive.
            let weapon = unsafe { &mut *(target as *mut WeaponBase) };
            if let Some(hand) = armed::hand_of(target) {
                unsafe { armed::disarm(owner, hand, Some(&mut *weapon), true) };
                PROMOTIONS.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { weapon.OnEquip(owner) };
            EQUIPS.fetch_add(1, Ordering::Relaxed);
            tracing::info!(target: "controllers", "reconcile: the primary hand's weapon is wielded");
        }
        (Some(current), None) if primary.is_none() => {
            if !unsafe { owner.m_Inventory.GetAnimationAllowsHolstering() } {
                return;
            }
            unsafe { abort_clips(owner) };
            // SAFETY: as above.
            let current = unsafe { &mut *(current as *mut WeaponBase) };
            unsafe { current.OnUnequip() };
            UNEQUIPS.fetch_add(1, Ordering::Relaxed);
            tracing::info!(target: "controllers", "reconcile: no hand holds a weapon; unequipped the wielded one");
        }
        _ => {}
    }
}

/// Arm each hand's weapon that the game does not wield, and disarm a hand's weapon it no longer
/// holds there or that the game now wields.
unsafe fn hands(owner: &mut Character, snapshot: &LoadoutSnapshot) {
    let wielded = unsafe { gun::held_weapon(owner) }.map(|w| w as *const WeaponBase as usize);
    for hand in Hand::BOTH {
        let want = hand_weapon(snapshot, hand).filter(|&w| Some(w) != wielded);
        let current = armed::in_hand(hand);
        if current.is_some() && current != want {
            let weapon = unsafe { armed::weapon_of(owner, hand) };
            let stop = current != wielded;
            unsafe { armed::disarm(owner, hand, weapon, stop) };
        }
        let Some(object) = want.filter(|&w| Some(w) != armed::in_hand(hand)) else {
            continue;
        };
        let alive = carry::contains(object)
            || owner
                .m_Inventory
                .m_Weapons
                .iter()
                .any(|slot| slot.px as usize == object);
        if !alive {
            continue;
        }
        // SAFETY: the slot, or the carry set, keeps the weapon alive.
        let weapon = unsafe { &mut *(object as *mut WeaponBase) };
        let heavy = unsafe { inventory_engine::slot_of(weapon) } == Some(WeaponSlot::E_SLOT_HEAVY);
        if heavy || weapon.m_State != WeaponState::E_WS_HOLSTERED {
            continue;
        }
        match unsafe { armed::arm(owner, weapon, hand) } {
            Ok(()) => {
                ARMS.fetch_add(1, Ordering::Relaxed);
                tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    "reconcile: armed the weapon in the {} hand",
                    hand.label()
                );
            }
            Err(refusal) => {
                tracing::debug!(target: "controllers", %refusal, "reconcile: the hand's weapon was not armed");
            }
        }
    }
}

/// The weapon the table holds in `hand`, by its object.
fn hand_weapon(snapshot: &LoadoutSnapshot, hand: Hand) -> Option<ObjectKey> {
    snapshot
        .items
        .iter()
        .find(|i| i.place == Place::Hand(hand) && i.kind.weapon().is_some())
        .and_then(|i| i.binding.address())
        .map(|a| a.get())
}

fn is_wielded(weapon: &WeaponBase) -> bool {
    matches!(
        weapon.m_State,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    )
}

/// Abort a draw, holster, or switch clip still playing, whose messages would equip or unequip by
/// slot when they fire.
unsafe fn abort_clips(owner: &mut Character) {
    if !unsafe { owner.IsDrawingOrHolsteringWeapon() } {
        return;
    }
    let Some(act) = resolve_act_id(EventIdSymbolTable::ACT_ABORT_WEAPON_PARTIAL) else {
        return;
    };
    let act = act as u32;
    unsafe { owner.QueueAct(&act) };
    ABORTS.fetch_add(1, Ordering::Relaxed);
}

/// Rebuild the HUD's weapon list from the slots.
unsafe fn refresh_hud() {
    if let Some(hud) = unsafe { HUDUI::get() } {
        unsafe { hud.InitWeapons() };
    }
}
