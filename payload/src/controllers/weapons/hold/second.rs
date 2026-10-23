//! A second weapon in the other hand: the design's "two weapons, one per hand".
//!
//! The loadout table says what each hand holds, and the game wields the primary hand's weapon
//! ([`crate::controllers::weapons::inventory::reconcile`]). The second weapon is the other hand's:
//! armed there ([`crate::controllers::weapons::hold::armed`]), it stays in the holstered state,
//! rides the hand's attach bone, and has the player as its user, and the per-hand tick fires it
//! from its hand's trigger ([`crate::controllers::weapons::hold::tick`]). This module is the grabs'
//! view of it: which hand holds it and where it came from ([`hand`], [`source`]), the takes that
//! put a long gun of another slot or a carried weapon in the free hand ([`take`],
//! [`take_carried`]), the put-back and the fall that take it out ([`put_back`], [`let_fall`]), its
//! grip's release ([`release_tick`]), and its magazine's pips ([`pips`]). Each move is an event for
//! the table, and the reconcile then arms or disarms the weapon at once.
//!
//! Its magazine is its own, and the true reload reloads it as it does the gun in the other hand,
//! while the HUD's readout stays the wielded weapon's. When the other hand empties, the second
//! weapon's hand is the primary, so the reconcile makes it the wielded weapon in the same hand.

use std::fmt;

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponState},
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    loadout::{ObjectAddress, Place, Socket, live},
    weapons::{
        dual_wield::single,
        engine::{self, HeldWeapon},
        heft::HeftClass,
        hold::{
            self, armed,
            dock::ReleaseWatch,
            equip::{self, Thrown},
            pips, tick,
        },
        holster::{Wielded, slot_label},
        inventory::{self, carry, engine as inventory_engine, reconcile},
    },
};

/// Why a second weapon was not taken, put back, or dropped.
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
    Carried(Socket),
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
    let held = held();
    let tick = tick::status();
    let promotions = reconcile::status().promotions;
    let state = STATE.lock();
    SecondStatus {
        held: held.map(|h| (h.hand, h.slot)),
        source: held.map(|h| h.source),
        trigger: held.is_some_and(|h| tick.fire[h.hand.index()]),
        updates: held.map_or(0, |h| tick.updates[h.hand.index()]),
        drops: state.drops,
        promotions,
        last_refusal: state.last_refusal,
        magazine: state.magazine,
    }
}

/// The hand holding the second weapon.
pub(crate) fn hand() -> Option<Hand> {
    held().map(|h| h.hand)
}

/// The second weapon's slot.
pub(crate) fn slot() -> Option<WeaponSlot> {
    held().map(|h| h.slot)
}

/// Where the second weapon came from.
pub(crate) fn source() -> Option<Source> {
    held().map(|h| h.source)
}

/// The second weapon, when `owner`'s inventory or the carry set holds it.
///
/// # Safety
///
/// `owner` must be the live local player character.
pub(crate) unsafe fn weapon(owner: &Character) -> Option<&'static WeaponBase> {
    let held = held()?;
    unsafe { armed::weapon_of(owner, held.hand) }.map(|w| &*w)
}

/// The second weapon as the weapon handling sees a weapon in the hands: every gun in its hand.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn held_weapon(owner: &Character) -> Option<HeldWeapon> {
    let held = held()?;
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
    taken(hand, result)
}

/// Take the carried weapon `object` off its socket into `hand` as the second weapon.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn take_carried(hand: Hand, object: usize) -> Result<(), SecondRefusal> {
    let result = unsafe { take_object(hand, object) };
    taken(hand, result)
}

/// Put the second weapon back onto the body: onto `socket` when it is given, otherwise onto the
/// socket it came from.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn put_back(socket: Option<Socket>) {
    let Some(held) = held() else {
        return;
    };
    let Some(object) = ObjectAddress::from_address(held.weapon) else {
        return;
    };
    let whereabouts = match (socket, held.source) {
        (Some(socket), _) | (None, Source::Carried(socket)) => live::Whereabouts::Carried(socket),
        (None, Source::Slot) => live::Whereabouts::Slot,
    };
    live::depart(held.hand, object, whereabouts);
    unsafe { reconcile::now() };
    tracing::info!(target: "controllers", "hold: the second weapon is back on the body");
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
    let held = held().ok_or(SecondRefusal::NothingHeld)?;
    let address = held.weapon;
    let weapon = unsafe { armed::weapon_of(owner, held.hand) }.ok_or(SecondRefusal::NotDone)?;
    unsafe { armed::disarm(owner, held.hand, Some(&mut *weapon), true) };
    if let Some(object) = ObjectAddress::from_address(address) {
        live::depart(held.hand, object, live::Whereabouts::Away);
    }
    // A carried weapon's strong reference is kept until the drop is done.
    let strong = match held.source {
        Source::Carried(_) => carry::release(address),
        Source::Slot => None,
    };
    if weapon.m_State != WeaponState::E_WS_HOLSTERED {
        return Err(SecondRefusal::NotHolstered);
    }
    let thrown = unsafe { equip::drop_loose(weapon, max_speed_mps) };
    if held.source == Source::Slot {
        unsafe { owner.m_Inventory.DropWeapon(held.slot, false) };
    }
    std::mem::drop(strong);
    unsafe { reconcile::apply(owner) };
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

/// Fold in one input tick of the grips, by [`Hand::index`]: returns the second weapon's hand when
/// its grip opened on it, once the grip had closed on it after it came into the hand.
pub(crate) fn release_tick(grips: [bool; 2]) -> Option<Hand> {
    let held = held();
    let holder = held.map(|h| h.hand);
    let mut state = STATE.lock();
    let weapon = held.map(|h| h.weapon);
    if weapon != state.last {
        state.last = weapon;
        state.release.disarm();
        if let Some(hand) = holder.filter(|h| grips[h.index()]) {
            state.release.arm(hand);
        }
    }
    state.release.latch_press(holder, grips);
    state.release.observe(holder, grips)
}

/// The carry set let its weapons go: nothing to do, since the second weapon is the table's and the
/// armed state's, which the spawn reset's removal updates.
pub(crate) fn carry_released() {
    STATE.lock().release.disarm();
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
        let capacity = unsafe { single::magazine_size(weapon) };
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

/// Stop: disarm the weapons in the hands, left on the body where the game has them. Called when
/// the weapon handling stands down.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn stand_down() {
    let Some(owner) = (unsafe { engine::local_player() }) else {
        for hand in Hand::BOTH {
            armed::forget(hand);
        }
        return;
    };
    for hand in Hand::BOTH {
        let weapon = unsafe { armed::weapon_of(owner, hand) };
        unsafe { armed::disarm(owner, hand, weapon, true) };
    }
    STATE.lock().release.disarm();
}

/// The second weapon as the grabs see it.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Held {
    weapon: usize,
    slot: WeaponSlot,
    hand: Hand,
    source: Source,
}

struct State {
    last_refusal: Option<SecondRefusal>,
    magazine: Option<(i32, i32)>,
    release: ReleaseWatch,
    /// The second weapon at the last grip tick, to arm the grip watch when it changes.
    last: Option<usize>,
    drops: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    last_refusal: None,
    magazine: None,
    release: ReleaseWatch::new(),
    last: None,
    drops: 0,
});

/// The second weapon: the weapon armed in a hand, the first hand's when both have one.
fn held() -> Option<Held> {
    let (hand, weapon) = Hand::BOTH
        .into_iter()
        .find_map(|hand| Some((hand, armed::in_hand(hand)?)))?;
    // SAFETY: an armed weapon is kept alive by its slot or the carry set while it is armed.
    let object = unsafe { (weapon as *const WeaponBase).as_ref() }?;
    let slot = unsafe { inventory_engine::slot_of(object) }?;
    let source = match carry::home(weapon) {
        Some(socket) if carry::contains(weapon) => Source::Carried(socket),
        _ => Source::Slot,
    };
    Some(Held {
        weapon,
        slot,
        hand,
        source,
    })
}

/// Record a take's result, or note the refusal.
fn taken(hand: Hand, result: Result<(), SecondRefusal>) -> Result<(), SecondRefusal> {
    match result {
        Ok(()) => {
            tracing::info!(
                target: "controllers",
                hand = hand.label(),
                slot = slot().map_or("unknown", slot_label),
                carried = matches!(source(), Some(Source::Carried(_))),
                "hold: the second weapon is in the {} hand",
                hand.label()
            );
            Ok(())
        }
        Err(refusal) => {
            STATE.lock().last_refusal = Some(refusal);
            hold::refused(refusal);
            Err(refusal)
        }
    }
}

unsafe fn take_slot(hand: Hand, slot: WeaponSlot) -> Result<(), SecondRefusal> {
    if !equip::handles(slot) {
        return Err(SecondRefusal::Unsupported);
    }
    let owner = unsafe { engine::local_player() }.ok_or(SecondRefusal::NoCharacter)?;
    let weapon = owner
        .m_Inventory
        .m_Weapons
        .get(slot as usize)
        .and_then(|s| unsafe { s.px.as_ref() })
        .ok_or(SecondRefusal::EmptySlot)?;
    unsafe { take_object(hand, weapon as *const WeaponBase as usize) }
}

/// Take the weapon at `object`, on the body, into `hand`: a draw off its socket for the table,
/// which the reconcile then arms in the hand.
unsafe fn take_object(hand: Hand, object: usize) -> Result<(), SecondRefusal> {
    if held().is_some() {
        return Err(SecondRefusal::Occupied);
    }
    if !matches!(live::place_of(object), Some(Place::Socket(_))) {
        return Err(SecondRefusal::NotHolstered);
    }
    // SAFETY: the table has it on the body, kept alive by its slot or the carry set.
    let table = unsafe { inventory::held_of(object) }.ok_or(SecondRefusal::Unsupported)?;
    if !live::arrive(hand, table) {
        return Err(SecondRefusal::NotDone);
    }
    unsafe { reconcile::now() };
    if armed::in_hand(hand) != Some(object) && live::in_hand(hand) != Some(object) {
        return Err(SecondRefusal::NotDone);
    }
    Ok(())
}
