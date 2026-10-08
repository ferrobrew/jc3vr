//! A second weapon in the other hand, fired by the mod: the design's "two weapons, one per hand".
//!
//! The game wields one weapon, the selected slot's. While it is in one hand, the free hand can hold
//! a second weapon there: a long gun of another slot taken off its shoulder ([`take`]), or any
//! weapon the VR inventory carries beyond the slots, taken off its socket ([`take_carried`]). The
//! second weapon stays in the holstered state, so the game's inventory, selection, HUD, and saves
//! see a slot's weapon as holstered, and its bodies stay out of the physics world:
//!
//! - its guns' holstered entries in the attach-bone table name the hand's attach bone, written at
//!   the same three points as the held weapon's ([`apply`], from the hold's detours), and its
//!   holstered scale ([`WeaponData::m_HolsteredScaleValue`]) is one while it is in the hand, both
//!   restored when it goes back. A slot's weapon is placed by the inventory, and a carried one by
//!   the carry set ([`crate::controllers::weapons::inventory::carry`]);
//! - it is given a user ([`WeaponBase::StartUsing`]), so its fire code runs as the player's, and
//!   loses it when it goes back ([`WeaponBase::StopUsing`], which leaves a holstered weapon
//!   holstered);
//! - after the inventory's own weapon updates ([`Inventory::UpdateWeapons_Parallel`] and
//!   [`Inventory::UpdateWeapons_Serial`]), the mod updates it the same way, through its
//!   [`WeaponBase::UpdateFromUserPostSim_Parallel`] and `_Serial` virtuals, with the hand's trigger
//!   as the fire input while the animation allows shooting
//!   ([`Inventory::GetAnimationAllowsShooting`]), the magazine holds a round, and the true reload
//!   does not hold the gun, and the gun's own sight line's aim point.
//!
//! The rigid grip, the gun aim, the per-shot seam, and the render-time late update treat it as a
//! held weapon ([`is`]). Its magazine is its own, shown in pips over its hand ([`pips`]); the true
//! reload turns to it when it runs lower than the wielded weapon
//! ([`crate::controllers::weapons::reload::focus`]), while the HUD's readout and the game's reload
//! act stay the selected weapon's. It goes back to the body at a grab of its holster or its socket
//! by its hand, falls when its grip opens elsewhere ([`let_fall`]), becomes the wielded weapon when the
//! other hand has been empty a moment ([`promote_tick`]), and is let go of (left holstered) when
//! the game wields it, takes it from its slot, or the weapon handling stands down.
//!
//! The detours run on the simulation's worker threads for every character, so they compare against
//! the second weapon's address before taking any lock. A carried second weapon is in no slot;
//! the carry set keeps it alive while it is the second weapon, and the input tick changes either
//! only outside the world update.

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
    equipment::weapon::{WeaponBase, WeaponData, WeaponInputs, WeaponState},
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    hands::ik,
    weapons::{
        engine::{self, HeldWeapon, slot_from_raw},
        heft::HeftClass,
        hold::{
            self,
            dock::ReleaseWatch,
            equip::{self, Thrown},
            holder::Cause,
            layout, pips,
        },
        holster::{Wielded, slot_label},
        inventory::{carry, decide::AloneWatch, sockets::SocketId},
        reload,
    },
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_WEAPONS_PARALLEL_BINDER)
        .with_static_binder(&UPDATE_WEAPONS_SERIAL_BINDER)
}

/// Why a second weapon was not taken, put back, dropped, or made the wielded weapon.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SecondRefusal {
    NoCharacter,
    /// The slot is not a long gun's.
    Unsupported,
    EmptySlot,
    /// The weapon is not on the body, or is in use.
    NotHolstered,
    /// A second weapon is already in the hands.
    Occupied,
    /// No second weapon is held.
    NothingHeld,
    /// The call ran, but the weapon is not where it should be afterwards.
    NotDone,
}

impl fmt::Display for SecondRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            SecondRefusal::NoCharacter => "there is no local player",
            SecondRefusal::Unsupported => "the slot is not a long gun's",
            SecondRefusal::EmptySlot => "the slot holds no weapon",
            SecondRefusal::NotHolstered => "the weapon is not on the body",
            SecondRefusal::Occupied => "a second weapon is already in the hands",
            SecondRefusal::NothingHeld => "no second weapon is held",
            SecondRefusal::NotDone => "the weapon is not where it should be afterwards",
        };
        write!(f, "second weapon: {why}")
    }
}

impl std::error::Error for SecondRefusal {}

/// Where the second weapon belongs when it is not in a hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Source {
    /// Its inventory slot, on its holster.
    Slot,
    /// A socket of the body, carried beyond the slots.
    Carried(SocketId),
}

/// The second weapon's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct SecondStatus {
    /// The hand holding it and its slot.
    pub held: Option<(Hand, WeaponSlot)>,
    /// Where it belongs, while it is held.
    pub source: Option<Source>,
    /// Whether its hand's trigger was pulled at the last input tick.
    pub trigger: bool,
    /// The updates the mod gave it since the payload loaded.
    pub updates: u64,
    /// Second weapons dropped from the hand and made the wielded weapon since the payload loaded.
    pub drops: u64,
    pub promotions: u64,
    pub last_refusal: Option<SecondRefusal>,
    /// Its magazine and the magazine's size as of the last input tick.
    pub magazine: Option<(i32, i32)>,
}

pub fn status() -> SecondStatus {
    let state = STATE.lock();
    SecondStatus {
        held: state.held.map(|h| (h.hand, h.slot)),
        source: state.held.map(|h| h.source),
        trigger: TRIGGER.load(Ordering::Relaxed),
        updates: UPDATES.load(Ordering::Relaxed),
        drops: state.drops,
        promotions: state.promotions,
        last_refusal: state.last_refusal,
        magazine: state.magazine,
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

/// The second weapon, when `owner`'s inventory or the carry set holds it.
///
/// # Safety
///
/// `owner` must be the live local player character.
pub(crate) unsafe fn weapon(owner: &Character) -> Option<&'static WeaponBase> {
    let address = ADDRESS.load(Ordering::Relaxed);
    if address == 0 {
        return None;
    }
    let in_slot = owner
        .m_Inventory
        .m_Weapons
        .iter()
        .find(|slot| slot.px as usize == address)
        .and_then(|slot| unsafe { slot.px.as_ref() });
    in_slot.or_else(|| unsafe { carried_weapon() })
}

/// The second weapon when it is a carried one, which the carry set keeps alive.
///
/// # Safety
///
/// On the main thread outside the world update, or inside the world update on a thread placing
/// or updating the local player's weapons.
pub(crate) unsafe fn carried_weapon() -> Option<&'static WeaponBase> {
    if !CARRIED.load(Ordering::Relaxed) {
        return None;
    }
    unsafe { (ADDRESS.load(Ordering::Relaxed) as *const WeaponBase).as_ref() }
}

/// The slot of the second weapon.
pub(crate) fn slot() -> Option<WeaponSlot> {
    STATE.lock().held.map(|h| h.slot)
}

/// Where the second weapon belongs.
pub(crate) fn source() -> Option<Source> {
    STATE.lock().held.map(|h| h.source)
}

/// The second weapon as the weapon handling sees a weapon in the hands: every gun in its hand.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn held_weapon(owner: &Character) -> Option<HeldWeapon> {
    let held = STATE.lock().held?;
    let weapon = unsafe { self::weapon(owner) }?;
    // Read as the raw value: an unset or unknown class is out of the enum's range.
    let class = unsafe { (&raw const weapon.m_Class).cast::<i32>().read() };
    Some(HeldWeapon {
        address: held.weapon,
        wielded: Wielded {
            slot: held.slot,
            hands: Hand::BOTH.map(|h| h == held.hand),
        },
        holder: Some(held.hand),
        two_handed: false,
        class: HeftClass::of(class, false),
    })
}

/// Take `slot`'s holstered long gun into `hand` as the second weapon.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn take(hand: Hand, slot: WeaponSlot) -> Result<(), SecondRefusal> {
    let result = unsafe { take_slot(hand, slot) };
    unsafe { taken(hand, result) }
}

/// Take the carried weapon `object` off its socket into `hand` as the second weapon.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn take_carried(hand: Hand, object: usize) -> Result<(), SecondRefusal> {
    let result = unsafe { take_from_carry(hand, object) };
    unsafe { taken(hand, result) }
}

/// Put the second weapon back where it belongs: its holster, or its socket (or `socket`, when it
/// is a carried weapon that should dock there from now on).
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn put_back(socket: Option<SocketId>) {
    let Some(owner) = (unsafe { engine::local_player() }) else {
        forget();
        return;
    };
    if let Some(socket) = socket
        && let Some(held) = STATE.lock().held
        && matches!(held.source, Source::Carried(_))
    {
        carry::set_in_hand(held.weapon, Some(held.hand), Some(socket));
    }
    unsafe { release(owner, true) };
}

/// Let the second weapon fall from its hand with the hand's motion, the linear part capped at
/// `max_speed_mps`. A slot's weapon leaves its slot; a carried one leaves the carry set. Either
/// then lies in the world, where the VR inventory keeps it while near.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn let_fall(max_speed_mps: f32) -> Result<Thrown, SecondRefusal> {
    let owner = unsafe { engine::local_player() }.ok_or(SecondRefusal::NoCharacter)?;
    let held = STATE.lock().held.ok_or(SecondRefusal::NothingHeld)?;
    let address = held.weapon;
    unsafe { release(owner, true) };
    // A carried weapon's strong reference is kept until the drop is done.
    let strong = match held.source {
        Source::Carried(_) => Some(carry::release(address).ok_or(SecondRefusal::NotDone)?),
        Source::Slot => None,
    };
    let weapon = match held.source {
        Source::Slot => unsafe { weapon_in(&owner.m_Inventory, held.slot) }
            .filter(|w| *w as *const WeaponBase as usize == address)
            .ok_or(SecondRefusal::NotDone)?,
        // SAFETY: the strong reference keeps the weapon alive.
        Source::Carried(_) => unsafe { &mut *(address as *mut WeaponBase) },
    };
    if weapon.m_State != WeaponState::E_WS_HOLSTERED {
        return Err(SecondRefusal::NotHolstered);
    }
    let thrown = unsafe { equip::drop_loose(weapon, max_speed_mps) };
    if held.source == Source::Slot {
        unsafe { owner.m_Inventory.DropWeapon(held.slot, false) };
    }
    std::mem::drop(strong);
    STATE.lock().drops += 1;
    tracing::info!(
        target: "controllers",
        hand = held.hand.label(),
        speed_mps = thrown.linear.length(),
        set_by_the_mod = thrown.set,
        "hold: dropped the second weapon from the {} hand",
        held.hand.label()
    );
    Ok(thrown)
}

/// Make the second weapon the wielded weapon once the hands have held nothing else for
/// `after_s`: a slot's weapon is equipped in place, and a carried one goes into its slot (the
/// slot's weapon taking its socket). It stays in the same hand.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn promote_tick(owner: &mut Character, wielded: Option<usize>, after_s: f32) {
    let now = Instant::now();
    let (held, due) = {
        let mut state = STATE.lock();
        let dt = state
            .last_tick
            .map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
        state.last_tick = Some(now);
        let held = state.held;
        let due = state
            .alone
            .observe(held.is_some(), wielded.is_some(), dt, after_s);
        (held, due)
    };
    let Some(held) = held.filter(|_| due) else {
        return;
    };
    let result = unsafe { promote(owner, held) };
    let mut state = STATE.lock();
    match result {
        Ok(()) => {
            state.promotions += 1;
            std::mem::drop(state);
            tracing::info!(
                target: "controllers",
                hand = held.hand.label(),
                slot = slot_label(held.slot),
                "hold: the second weapon in the {} hand is now the wielded weapon",
                held.hand.label()
            );
        }
        Err(refusal) => {
            state.last_refusal = Some(refusal);
            std::mem::drop(state);
            hold::refused(refusal);
        }
    }
}

/// Fold in one input tick of the grips, by [`Hand::index`]: returns the second weapon's hand when
/// its grip opened on it after a grab took it.
pub(crate) fn release_tick(grips: [bool; 2]) -> Option<Hand> {
    let holder = hand();
    let mut state = STATE.lock();
    if let Some(armed) = state.arm_pending.take()
        && holder == Some(armed)
        && grips[armed.index()]
    {
        state.release.arm(armed);
    }
    state.release.observe(holder, grips)
}

/// Check the second weapon is still in its slot or the carry set, holstered, and used by `owner`;
/// let it go otherwise (left as the game has it).
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn validate(owner: &mut Character) {
    let Some(held) = STATE.lock().held else {
        return;
    };
    let Some(weapon) = (unsafe { live(owner, &held) }) else {
        // The weapon left its slot or the carry set: nothing to restore on it.
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

/// The carry set is letting its weapons go: a carried second weapon is forgotten at once, without
/// touching it, since the carry set's reference was what kept it alive. Called from the spawn
/// system's reset, on whatever thread runs it, and when carrying is switched off.
pub(crate) fn carry_released() {
    if CARRIED.load(Ordering::Relaxed) {
        forget();
        tracing::info!(target: "controllers", "hold: the carried second weapon was let go of with the carry set");
    }
}

/// Show the second weapon's magazine over its hand, or take it away when there is none or `shown`
/// is off.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn show_magazine(owner: Option<&Character>, shown: bool) {
    let readout = owner.and_then(|owner| {
        let hand = hand()?;
        let weapon = unsafe { self::weapon(owner) }?;
        let capacity = unsafe { weapon.GetMagazineSize() };
        Some((
            hand,
            weapon.m_MagazineAmmo,
            capacity,
            weapon.m_WeaponIsWithoutMagazine,
        ))
    });
    STATE.lock().magazine = readout.map(|(_, magazine, capacity, _)| (magazine, capacity));
    pips::submit(readout.filter(|_| shown));
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

/// Write the second weapon's hand into its holstered entries, when `inventory` holds it or it is a
/// carried weapon and `inventory` is the local player's.
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
    source: Source,
    /// Each gun's holstered bone and holstered scale before it came into the hand.
    saved: [Option<(u32, f32)>; gun::MAX_GUNS],
}

struct State {
    held: Option<Held>,
    last_refusal: Option<SecondRefusal>,
    magazine: Option<(i32, i32)>,
    /// Watches for the second weapon held with nothing else wielded.
    alone: AloneWatch,
    last_tick: Option<Instant>,
    release: ReleaseWatch,
    arm_pending: Option<Hand>,
    drops: u64,
    promotions: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    held: None,
    last_refusal: None,
    magazine: None,
    alone: AloneWatch::new(),
    last_tick: None,
    release: ReleaseWatch::new(),
    arm_pending: None,
    drops: 0,
    promotions: 0,
});
/// The second weapon's address, or zero: the detours' check before any lock.
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
/// Whether the second weapon is a carried one, in no slot.
static CARRIED: AtomicBool = AtomicBool::new(false);
/// The hand holding it: one for the left, two for the right.
static HAND: AtomicU8 = AtomicU8::new(0);
static TRIGGER: AtomicBool = AtomicBool::new(false);
static TARGET: Mutex<Option<(Vec3, Instant)>> = Mutex::new(None);
static UPDATES: AtomicU64 = AtomicU64::new(0);

/// How long the aim update's point stays current for the second weapon's update.
const MAX_TARGET_AGE: Duration = Duration::from_millis(250);

/// Record a take's result: publish the weapon, or note the refusal.
unsafe fn taken(hand: Hand, result: Result<Held, SecondRefusal>) -> Result<(), SecondRefusal> {
    let mut state = STATE.lock();
    match result {
        Ok(held) => {
            state.held = Some(held);
            state.alone = AloneWatch::new();
            state.arm_pending = Some(hand);
            HAND.store(
                match hand {
                    Hand::Left => 1,
                    Hand::Right => 2,
                },
                Ordering::Relaxed,
            );
            CARRIED.store(matches!(held.source, Source::Carried(_)), Ordering::Relaxed);
            ADDRESS.store(held.weapon, Ordering::Relaxed);
            std::mem::drop(state);
            tracing::info!(
                target: "controllers",
                hand = hand.label(),
                slot = slot_label(held.slot),
                carried = matches!(held.source, Source::Carried(_)),
                "hold: the second weapon is in the {} hand",
                hand.label()
            );
            Ok(())
        }
        Err(refusal) => {
            state.last_refusal = Some(refusal);
            std::mem::drop(state);
            hold::refused(refusal);
            Err(refusal)
        }
    }
}

unsafe fn take_slot(hand: Hand, slot: WeaponSlot) -> Result<Held, SecondRefusal> {
    if STATE.lock().held.is_some() {
        return Err(SecondRefusal::Occupied);
    }
    if !equip::handles(slot) {
        return Err(SecondRefusal::Unsupported);
    }
    let owner = unsafe { engine::local_player() }.ok_or(SecondRefusal::NoCharacter)?;
    let weapon = unsafe { weapon_in(&owner.m_Inventory, slot) }.ok_or(SecondRefusal::EmptySlot)?;
    unsafe { into_hand(owner, weapon, hand, slot, Source::Slot) }
}

unsafe fn take_from_carry(hand: Hand, object: usize) -> Result<Held, SecondRefusal> {
    if STATE.lock().held.is_some() {
        return Err(SecondRefusal::Occupied);
    }
    let socket = carry::home(object).ok_or(SecondRefusal::NotHolstered)?;
    let owner = unsafe { engine::local_player() }.ok_or(SecondRefusal::NoCharacter)?;
    // SAFETY: the carry set keeps the weapon alive.
    let weapon = unsafe { &mut *(object as *mut WeaponBase) };
    // Read as the raw value: an unset slot is out of the enum's range.
    let slot = slot_from_raw(unsafe { (&raw const weapon.m_EquipSlot).cast::<i32>().read() })
        .ok_or(SecondRefusal::Unsupported)?;
    let held = unsafe { into_hand(owner, weapon, hand, slot, Source::Carried(socket)) }?;
    carry::set_in_hand(object, Some(hand), None);
    Ok(held)
}

/// Put the holstered `weapon` in `hand`: its holstered entries name the hand's bone, its
/// holstered scale is one, and the character uses it.
unsafe fn into_hand(
    owner: &mut Character,
    weapon: &mut WeaponBase,
    hand: Hand,
    slot: WeaponSlot,
    source: Source,
) -> Result<Held, SecondRefusal> {
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
        source,
        saved,
    })
}

/// The second weapon as it is now: its slot's, or the carry set's.
unsafe fn live(owner: &Character, held: &Held) -> Option<&'static mut WeaponBase> {
    match held.source {
        Source::Slot => unsafe { weapon_in(&owner.m_Inventory, held.slot) }
            .filter(|w| *w as *const WeaponBase as usize == held.weapon),
        // SAFETY: the carry set keeps a weapon it holds alive.
        Source::Carried(_) if carry::contains(held.weapon) => unsafe {
            (held.weapon as *mut WeaponBase).as_mut()
        },
        Source::Carried(_) => None,
    }
}

/// Restore the second weapon's table and scale, stop using it when `stop` and it is still the
/// player's, put a carried one back on its socket, and forget it.
unsafe fn release(owner: &mut Character, stop: bool) {
    let Some(held) = STATE.lock().held else {
        return;
    };
    forget();
    let Some(weapon) = (unsafe { live(owner, &held) }) else {
        return;
    };
    unsafe { restore(weapon, &held) };
    let used = std::ptr::eq(weapon.m_User[0].cast::<Character>(), owner);
    if stop && used && weapon.m_State == WeaponState::E_WS_HOLSTERED {
        unsafe { weapon.StopUsing() };
    }
    if matches!(held.source, Source::Carried(_)) {
        carry::set_in_hand(held.weapon, None, None);
    }
    tracing::info!(target: "controllers", "hold: the second weapon is back on the body");
}

/// Put back the guns' holstered entries and scales the hand took over.
unsafe fn restore(weapon: &mut WeaponBase, held: &Held) {
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
}

/// Make `held` the wielded weapon, in the hand it is in.
unsafe fn promote(owner: &mut Character, held: Held) -> Result<(), SecondRefusal> {
    if !unsafe { owner.m_Inventory.GetAnimationAllowsDrawing() } {
        return Err(SecondRefusal::NotDone);
    }
    let weapon = unsafe { live(owner, &held) }.ok_or(SecondRefusal::NothingHeld)?;
    unsafe { restore(weapon, &held) };
    forget();
    if matches!(held.source, Source::Carried(_)) {
        carry::set_in_hand(held.weapon, None, None);
    }
    hold::begin(held.hand, Cause::Draw, Some(held.weapon));
    match held.source {
        Source::Slot => unsafe { weapon.OnEquip(owner) },
        Source::Carried(_) => {
            unsafe { carry::promote(owner, held.weapon) }.map_err(|_| SecondRefusal::NotDone)?;
        }
    }
    let wielded = unsafe { gun::held_weapon(owner) }
        .is_some_and(|w| w as *const WeaponBase as usize == held.weapon);
    if !wielded {
        return Err(SecondRefusal::NotDone);
    }
    hold::adopt(held.weapon, held.hand);
    Ok(())
}

fn forget() {
    let mut state = STATE.lock();
    state.held = None;
    state.alone = AloneWatch::new();
    state.release.disarm();
    state.arm_pending = None;
    std::mem::drop(state);
    ADDRESS.store(0, Ordering::Relaxed);
    CARRIED.store(false, Ordering::Relaxed);
    HAND.store(0, Ordering::Relaxed);
    TRIGGER.store(false, Ordering::Relaxed);
}

/// The weapon in `inventory`'s `slot`.
unsafe fn weapon_in(inventory: &Inventory, slot: WeaponSlot) -> Option<&'static mut WeaponBase> {
    unsafe { inventory.m_Weapons.get(slot as usize)?.px.as_mut() }
}

/// The weapon's guns, to write.
unsafe fn guns_mut(weapon: &mut WeaponBase) -> impl Iterator<Item = &'static mut WeaponData> {
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

/// The second weapon of `inventory`, when the inventory is the local player's and holds it, or it
/// is a carried weapon.
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
    if CARRIED.load(Ordering::Relaxed) {
        // SAFETY: the carry set keeps a carried second weapon alive while it is the second weapon.
        return unsafe { (address as *mut WeaponBase).as_mut() };
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
    let address = weapon as *const WeaponBase as usize;
    let held_by_reload = hand().is_some_and(|h| reload::holds_fire(address, h));
    let fire = loaded
        && !held_by_reload
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
