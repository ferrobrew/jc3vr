//! Which hand holds the wielded weapon, and the attach bones the mod owns to put it there. The
//! design is in `docs/mod/input/physical-weapons.md`, the engine side in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`.
//!
//! The game holds every weapon but the dual-wield pair in the right hand. Here any weapon rides
//! the hand that drew it at a holster, took it off the ground, or was handed it ([`holder`]): its
//! guns ride that hand's attach bones ([`layout`]), and everything downstream that derives a gun's
//! hand from its attach bone (the gun aim, the shot's hand, the rigid grip, the recoil, the heft,
//! the haptics, the reload, and the render-time late update) follows. The grenade, the planted
//! explosive, and the beacon are left to the game. The dual-wield pair keeps its own layout ([`crate::controllers::weapons::dual_wield::attach`]), one pistol per hand.
//!
//! The game rewrites every gun's attach-bone table at the end of the character's
//! pose-finalization pass ([`WeaponBase::UpdatePostFinalizePose`]), and the character's state
//! tasks move single guns between bones earlier in the same pass, so a table written once does not
//! stay. The tables are therefore written at three points, from the layouts the input tick
//! publishes:
//!
//! - before each placement of the inventory's weapons
//!   ([`Inventory::UpdateAttachmentTransforms`]), so the guns are placed on the mod's bones
//!   whatever the state tasks wrote; a gun whose hand changed since its last placement skips its
//!   render interpolation for that placement
//!   ([`WeaponBase::SkipRenderInterpolationForOneFrame`]), so it moves between hands (or between
//!   hand and hip) in one frame rather than streaking;
//! - after the end-of-pass rewrite ([`Inventory::UpdateInventoryPostFinalizePose`]), so every
//!   reader between two passes (the input tick, the gun aim, the shot's hand, and the render-time
//!   late update) sees the mod's bones;
//! - at the weapon's own per-tick update ([`before_update`], from the gun aim's detour), just
//!   before it fires.
//!
//! Both the wielding and the dual-wielding entries are written, and only a hand attach bone of the
//! other hand is moved, to its counterpart: the game's own moves within a hand, and a gun it puts
//! off the hands, are kept. Nothing changes the weapon's state.
//!
//! The placement detour also runs the rigid grip around the original
//! ([`crate::controllers::hands::grip`]), for every weapon the local player holds.
//!
//! The detours run on the simulation's worker threads for every character, so they compare the
//! inventory's weapons against the owned weapon's address before taking any lock, and they never
//! touch the VR state.
//!
//! A weapon grabbed off the ground goes into the grabbing hand at once ([`pickup`]). A long gun can
//! be drawn and holstered without the game's acts, sliding between its dock and the hand, and let
//! go of to fall with the hand's motion ([`act`], [`dock`], and [`equip`]). The free hand can hold
//! a second weapon, a slot's or one the VR inventory carries beyond the slots, which the mod fires
//! ([`second`]); the weapons beyond the slots are the VR inventory's
//! ([`crate::controllers::weapons::inventory`]). A free hand can also take a gun out of a
//! living character's hands ([`steal`]) or off a rack ([`rack`]), through the volumes the take places on weapons that have
//! no body in the world ([`take`] and [`world`]).

pub mod act;
pub mod dock;
pub mod equip;
pub mod holder;
pub mod layout;
pub mod pickup;
pub mod pips;
pub mod rack;
pub(crate) mod refs;
pub mod second;
pub mod steal;
pub mod take;
pub mod world;

use std::{
    sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    time::Instant,
};

use detours_macro::detour;
use jc3gi::{
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    equipment::weapon::{WeaponBase, WeaponState},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    hands::{grip, late},
    weapons::{
        dual_wield,
        engine::slot_from_raw,
        hold::holder::{Cause, Change, Holder, Pending},
        inventory,
    },
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_ATTACHMENT_TRANSFORMS_BINDER)
        .with_static_binder(&UPDATE_INVENTORY_POST_FINALIZE_POSE_BINDER)
        .with_hook_library(second::hook_library())
}

/// The settings the holding hand acts on.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Settings {
    /// The hand a weapon the game wields on its own goes to.
    pub fallback: Hand,
}

/// The hold's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HoldStatus {
    /// Whether the mod owns the wielded weapon's attach bones.
    pub owned: bool,
    /// The hand holding the wielded weapon.
    pub holder: Option<Hand>,
    /// What brought the wielded weapon into the hands, or `None` for the game's own draw.
    pub origin: Option<Cause>,
    /// A hand waiting for the game to wield a weapon.
    pub pending: Option<Pending>,
    /// Handovers, direct pickups, and pickups left to the game's use-item action since the payload
    /// loaded.
    pub handovers: u64,
    pub pickups: u64,
    pub pickup_fallbacks: u64,
    /// Why the last direct pickup did not happen.
    pub last_pickup_refusal: Option<String>,
    /// Table writes and snapped placements since the payload loaded.
    pub writes: u64,
    pub snaps: u64,
    /// The takes of weapons that have no body in the world.
    pub take: take::TakeStatus,
}

pub fn status() -> HoldStatus {
    // Read before the tick's lock: the status rows take one lock at a time.
    let take = take::status();
    let tick = TICK.lock();
    HoldStatus {
        owned: OWNED.load(Ordering::Relaxed) != 0,
        holder: tick.holder.hand(),
        origin: tick.holder.origin(),
        pending: tick.holder.pending(),
        handovers: tick.handovers,
        pickups: tick.pickups,
        pickup_fallbacks: tick.pickup_fallbacks,
        last_pickup_refusal: tick.last_pickup_refusal.clone(),
        writes: WRITES.load(Ordering::Relaxed),
        snaps: SNAPS.load(Ordering::Relaxed),
        take,
    }
}

/// Follow the local player's wielded weapon for one input tick and publish which hand holds it.
/// The mod owns the weapon's attach bones while `settings` holds and the weapon is not the
/// dual-wield pair the pistols' own layout owns (`pair_owned`). Returns the holding hand.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(
    owner: &Character,
    settings: Option<Settings>,
    pair_owned: bool,
) -> Option<Hand> {
    // The grenade, the planted explosive, and the beacon are held as the game holds them.
    let weapon = unsafe { gun::held_weapon(owner) }.filter(|w| {
        // Read as the raw value: an unset slot is out of the enum's range.
        let slot = unsafe { (&raw const w.m_EquipSlot).cast::<i32>().read() };
        !matches!(
            slot_from_raw(slot),
            Some(
                WeaponSlot::E_SLOT_HAND_GRENADE
                    | WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE
                    | WeaponSlot::E_SLOT_BEACON
            )
        )
    });
    let address = weapon.map(|w| w as *const WeaponBase as usize);
    let mut tick = TICK.lock();
    let now = Instant::now();
    let dt = tick
        .last
        .map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
    tick.last = Some(now);
    let fallback = settings.map_or(Hand::Right, |s| s.fallback);
    let change = tick.holder.observe(address, fallback, dt);
    if settings.is_some() {
        took(change);
    }
    if let (
        Some(Change::Took {
            hand,
            cause: Some(Cause::Rack),
        }),
        Some(address),
    ) = (change, address)
    {
        rack::arrived(address, hand);
    }
    let owns = settings.is_some() && !pair_owned;
    ENABLED.store(settings.is_some(), Ordering::Relaxed);
    let holder = tick.holder.hand().filter(|_| settings.is_some());
    drop(tick);
    unsafe { act::tick(owner, address, dt) };
    match (address, holder) {
        (Some(address), Some(hand)) if owns => {
            HOLDER.store(hand_code(Some(hand)), Ordering::Relaxed);
            OWNED.store(address, Ordering::Relaxed);
        }
        _ => clear(),
    }
    holder
}

/// What brought the wielded weapon into the hands, or `None` for the game's own draw.
pub(crate) fn origin() -> Option<Cause> {
    TICK.lock().holder.origin()
}

/// Leave `hand` pending for the next weapon the game wields, or for `weapon` when it is named.
pub(crate) fn begin(hand: Hand, cause: Cause, weapon: Option<usize>) {
    TICK.lock().holder.begin(hand, cause, weapon);
}

/// The weapon at `weapon` came into the hands by a call the mod made (an equip or a pickup): follow
/// it at once, rather than at the next input tick, so that the next placement already puts it in
/// its hand. Only while the hold is on, and never for the dual-wield pair, whose own layout owns
/// its bones.
pub(crate) fn adopt(weapon: usize, fallback: Hand) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let mut tick = TICK.lock();
    let change = tick.holder.observe(Some(weapon), fallback, 0.0);
    took(change);
    if let Some(hand) = tick.holder.hand() {
        HOLDER.store(hand_code(Some(hand)), Ordering::Relaxed);
        OWNED.store(weapon, Ordering::Relaxed);
    }
}

/// Hand the wielded weapon to `hand`. Returns whether it moved.
pub(crate) fn hand_over(hand: Hand) -> bool {
    let mut tick = TICK.lock();
    if !tick.holder.hand_over(hand) {
        return false;
    }
    tick.handovers += 1;
    act::arm(hand);
    if OWNED.load(Ordering::Relaxed) != 0 {
        HOLDER.store(hand_code(Some(hand)), Ordering::Relaxed);
    }
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        "hold: handed the weapon to the {} hand",
        hand.label()
    );
    true
}

/// Stop owning the attach bones; the game's end-of-pass rewrite restores its own layout.
pub(crate) fn clear() {
    OWNED.store(0, Ordering::Relaxed);
    HOLDER.store(0, Ordering::Relaxed);
}

/// The hand holding `weapon`, while the mod owns its attach bones.
pub(crate) fn owned_holder(weapon: usize) -> Option<Hand> {
    (weapon != 0 && OWNED.load(Ordering::Relaxed) == weapon)
        .then(holder_hand)
        .flatten()
}

/// Write the holding hand's bones into `weapon`'s attach-bone table, when `weapon` is the owned
/// weapon and wielded. With `snap`, a gun whose hand changed since its last placement skips its
/// render interpolation.
///
/// # Safety
///
/// `weapon` must be live, on the thread updating its holder.
pub(crate) unsafe fn apply(weapon: &mut WeaponBase, snap: bool) {
    let address = weapon as *mut WeaponBase as usize;
    if OWNED.load(Ordering::Relaxed) != address || !is_wielded(weapon.m_State) {
        return;
    }
    let Some(holder) = holder_hand() else {
        return;
    };
    let state = weapon.m_State as usize;
    let mut hands = [None; gun::MAX_GUNS];
    let list = &weapon.m_lWeaponData;
    let count = if list.m_Begin.is_null() || list.m_End < list.m_Begin {
        0
    } else {
        unsafe { list.m_End.offset_from(list.m_Begin) as usize }
    };
    let mut wrote = false;
    for (index, hand) in hands.iter_mut().enumerate().take(count) {
        // SAFETY: the entry is inside the weapon's live gun list.
        let Some(data) = (unsafe { (*list.m_Begin.add(index)).as_mut() }) else {
            continue;
        };
        for wielding in [WeaponState::E_WS_WIELDING, WeaponState::E_WS_DUAL_WIELDING] {
            if let Some(entry) = data
                .m_lBoneAttachementPerWeaponState
                .get_mut(wielding as usize)
                && let Some(bone) = layout::placed_bone(*entry, holder, index, count)
            {
                *entry = bone;
                wrote = true;
            }
        }
        *hand = data
            .m_lBoneAttachementPerWeaponState
            .get(state)
            .and_then(|&bone| late::attach_hand(bone));
    }
    if wrote {
        WRITES.fetch_add(1, Ordering::Relaxed);
    }
    if !snap {
        return;
    }
    let previous = {
        let mut placed = PLACED.lock();
        let previous = placed.filter(|p| p.weapon == address).map(|p| p.hands);
        *placed = Some(Placed {
            weapon: address,
            hands,
        });
        previous
    };
    let Some(previous) = previous else {
        return;
    };
    for (index, (was, now)) in previous.into_iter().zip(hands).enumerate() {
        if was.is_some()
            && now.is_some()
            && was != now
            && let Ok(index) = i32::try_from(index)
        {
            unsafe { weapon.SkipRenderInterpolationForOneFrame(index) };
            SNAPS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Write the owned layouts into the local player's weapon at its per-tick update, before it
/// fires: the shot's hand and the gun aim read the attach bones, which a state task may have moved
/// since the last write.
///
/// # Safety
///
/// `weapon` must be live, called from its per-tick update before the original runs.
pub(crate) unsafe fn before_update(weapon: &mut WeaponBase) {
    if weapon.m_IsUserLocalPlayer {
        unsafe { apply(weapon, false) };
    }
}

/// A direct pickup happened, or was left to the game's use-item action because of `refusal`.
pub(crate) fn record_pickup(refusal: Option<String>) {
    let mut tick = TICK.lock();
    match refusal {
        None => tick.pickups += 1,
        Some(refusal) => {
            tick.pickup_fallbacks += 1;
            tick.last_pickup_refusal = Some(refusal.clone());
            drop(tick);
            refused(refusal);
        }
    }
}

/// Note why the last thing the hands tried with a weapon (a pickup, a take, a draw, a holster, a
/// drop, or a second weapon) did not happen, for the readout.
pub(crate) fn refused(why: impl ToString) {
    *LAST_REFUSAL.lock() = Some(why.to_string());
}

/// Why the last thing the hands tried with a weapon did not happen.
pub fn last_refusal() -> Option<String> {
    LAST_REFUSAL.lock().clone()
}

/// Log a weapon coming into a hand, and arm the grip watch when a grab put it there.
fn took(change: Option<Change>) {
    let Some(Change::Took { hand, cause }) = change else {
        return;
    };
    if cause.is_some() {
        act::arm(hand);
    }
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        cause = cause.map_or("the game's draw", Cause::label),
        "hold: the weapon is in the {} hand",
        hand.label()
    );
}

/// The input tick's side.
struct Tick {
    holder: Holder,
    last: Option<Instant>,
    handovers: u64,
    pickups: u64,
    pickup_fallbacks: u64,
    last_pickup_refusal: Option<String>,
}

/// The hand each gun of the owned weapon was last placed in.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Placed {
    weapon: usize,
    hands: [Option<Hand>; gun::MAX_GUNS],
}

static TICK: Mutex<Tick> = Mutex::new(Tick {
    holder: Holder::new(),
    last: None,
    handovers: 0,
    pickups: 0,
    pickup_fallbacks: 0,
    last_pickup_refusal: None,
});
static PLACED: Mutex<Option<Placed>> = Mutex::new(None);
static LAST_REFUSAL: Mutex<Option<String>> = Mutex::new(None);
/// The owned weapon's address, or zero: the detours' check before any lock.
static OWNED: AtomicUsize = AtomicUsize::new(0);
/// The holding hand ([`hand_code`]).
static HOLDER: AtomicU8 = AtomicU8::new(0);
/// Whether the last input tick had the hold on.
static ENABLED: AtomicBool = AtomicBool::new(false);
static WRITES: AtomicU64 = AtomicU64::new(0);
static SNAPS: AtomicU64 = AtomicU64::new(0);

const fn hand_code(hand: Option<Hand>) -> u8 {
    match hand {
        None => 0,
        Some(Hand::Left) => 1,
        Some(Hand::Right) => 2,
    }
}

fn holder_hand() -> Option<Hand> {
    match HOLDER.load(Ordering::Relaxed) {
        1 => Some(Hand::Left),
        2 => Some(Hand::Right),
        _ => None,
    }
}

const fn is_wielded(state: WeaponState) -> bool {
    matches!(
        state,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    )
}

/// The owned weapon, when `inventory` holds it.
unsafe fn owned_in(inventory: *mut Inventory) -> Option<&'static mut WeaponBase> {
    let owned = OWNED.load(Ordering::Relaxed);
    if owned == 0 {
        return None;
    }
    let inventory = unsafe { inventory.as_ref() }?;
    inventory
        .m_Weapons
        .iter()
        .find(|slot| slot.px as usize == owned)
        .and_then(|slot| unsafe { slot.px.as_mut() })
}

#[detour(address = jc3gi::character::inventory::Inventory::UpdateAttachmentTransforms_ADDRESS)]
fn update_attachment_transforms(
    inventory: *mut Inventory,
    character: *const Character,
    p3: f32,
    p4: f32,
    transform_type: u32,
) {
    unsafe { dual_wield::attach::apply_owned(inventory, true) };
    if let Some(weapon) = unsafe { owned_in(inventory) } {
        unsafe { apply(weapon, true) };
    }
    unsafe { second::apply_owned(inventory) };
    unsafe { dual_wield::single::apply_home(inventory) };
    // The rigid grip pins the attach bones after the layouts above have chosen them.
    let placement = unsafe { grip::before_placement(inventory, character) };
    UPDATE_ATTACHMENT_TRANSFORMS
        .get()
        .unwrap()
        .call(inventory, character, p3, p4, transform_type);
    // No inventory places the carried weapons; they follow the slots' weapons.
    unsafe { inventory::carry::place(inventory, character, p3, p4, transform_type) };
    if let Some(placement) = placement {
        unsafe { grip::after_placement(character, placement) };
    }
}

#[detour(address = jc3gi::character::inventory::Inventory::UpdateInventoryPostFinalizePose_ADDRESS)]
fn update_inventory_post_finalize_pose(inventory: *mut Inventory) {
    UPDATE_INVENTORY_POST_FINALIZE_POSE
        .get()
        .unwrap()
        .call(inventory);
    unsafe { dual_wield::attach::apply_owned(inventory, false) };
    if let Some(weapon) = unsafe { owned_in(inventory) } {
        unsafe { apply(weapon, false) };
    }
    unsafe { second::apply_owned(inventory) };
    unsafe { dual_wield::single::apply_home(inventory) };
    // No inventory ends the pass for the carried weapons.
    unsafe { inventory::carry::end_pass(inventory) };
    // The guns are placed for the pass, so another character's gun is where it is drawn.
    unsafe { steal::observe(inventory) };
}
