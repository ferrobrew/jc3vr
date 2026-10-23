//! Which hand holds the wielded weapon, and the attach bones the mod owns to put it there. The
//! design is in `docs/mod/input/physical-weapons.md`, the engine side in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`.
//!
//! The game holds every weapon but the dual-wield pair in the right hand. Here any weapon rides
//! the hand the loadout table holds it in ([`crate::controllers::loadout::live`]): the hand that
//! drew it at a holster, took it off the ground, or was handed it, or for a weapon the game wields
//! on its own a free hand, the dominant first. A grab that starts a draw or a take tells the table
//! which hand expects the weapon ([`begin`]), a call of the mod's that wields one lands it at once
//! ([`adopt`]), and a handover moves it ([`hand_over`]). Its guns ride that hand's attach bones
//! ([`layout`]), and everything downstream that derives a gun's hand from its attach bone (the gun
//! aim, the shot's hand, the rigid grip, the recoil, the heft, the haptics, the reload, and the
//! render-time late update) follows. The grenade, the planted explosive, and the beacon are left
//! to the game. The dual-wield pair keeps its own layout
//! ([`crate::controllers::weapons::dual_wield::attach`]), one pistol per hand. The table holds the
//! hands only while the VR inventory runs it; without it the game's own layout stands.
//!
//! The game rewrites every gun's attach-bone table at the end of the character's
//! pose-finalization pass ([`WeaponBase::UpdatePostFinalizePose`]), and the character's state
//! tasks move single guns between bones earlier in the same pass, so a table written once does not
//! stay. The tables are therefore written at three points, from the hands the table publishes:
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
//! inventory's weapons against the table's published hands before taking any lock, and they never
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
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
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
    loadout::{ObjectAddress, TakeSource, live},
    weapons::{dual_wield, engine::slot_from_raw, inventory},
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_ATTACHMENT_TRANSFORMS_BINDER)
        .with_static_binder(&UPDATE_INVENTORY_POST_FINALIZE_POSE_BINDER)
        .with_hook_library(second::hook_library())
}

/// What a grab that brings a weapon into a hand is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Cause {
    /// A holster grab's draw.
    Draw,
    /// A weapon taken off the ground.
    Pickup,
    /// A gun taken out of a living character's hands.
    Steal,
    /// A weapon taken off a rack, or the copy a rack hands out.
    Rack,
}

impl Cause {
    pub const fn label(self) -> &'static str {
        match self {
            Cause::Draw => "draw",
            Cause::Pickup => "pickup",
            Cause::Steal => "taken from a character",
            Cause::Rack => "rack",
        }
    }

    /// What the grab leaves its hand expecting of the table.
    const fn expect(self) -> live::Expect {
        match self {
            Cause::Draw => live::Expect::Draw,
            Cause::Pickup => live::Expect::Take(TakeSource::World),
            Cause::Steal => live::Expect::Take(TakeSource::Character),
            Cause::Rack => live::Expect::Take(TakeSource::Rack),
        }
    }

    /// The grab an expectation of the table's came from, when one of the hold's did.
    const fn of(expect: live::Expect) -> Option<Cause> {
        match expect {
            live::Expect::Draw => Some(Cause::Draw),
            live::Expect::Take(TakeSource::World) => Some(Cause::Pickup),
            live::Expect::Take(TakeSource::Character) => Some(Cause::Steal),
            live::Expect::Take(TakeSource::Rack | TakeSource::RackCopy) => Some(Cause::Rack),
            live::Expect::Take(
                TakeSource::Locker
                | TakeSource::Give
                | TakeSource::Snatch
                | TakeSource::Pouch
                | TakeSource::Catch,
            ) => None,
        }
    }
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
    /// A hand expecting the game to wield a weapon, and the grab that left it so.
    pub pending: Option<(Hand, Cause)>,
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
    let pending = live::pending().and_then(|p| Some((p.hand, Cause::of(p.expect)?)));
    let wielded = WIELDED.load(Ordering::Relaxed);
    let holder = owned_holder(wielded);
    let origin = holder.and_then(live::origin).and_then(Cause::of);
    let tick = TICK.lock();
    HoldStatus {
        owned: holder.is_some(),
        holder,
        origin,
        pending,
        handovers: tick.handovers,
        pickups: tick.pickups,
        pickup_fallbacks: tick.pickup_fallbacks,
        last_pickup_refusal: tick.last_pickup_refusal.clone(),
        writes: WRITES.load(Ordering::Relaxed),
        snaps: SNAPS.load(Ordering::Relaxed),
        take,
    }
}

/// The weapon the game wields in the hands, as the hold follows it: the grenade, the planted
/// explosive, and the beacon are held as the game holds them.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn wielded(owner: &Character) -> Option<usize> {
    unsafe { gun::held_weapon(owner) }
        .filter(|w| {
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
        })
        .map(|w| w as *const WeaponBase as usize)
}

/// Follow the local player's wielded weapon (`wielded`, from [`wielded`]) for one input tick, after
/// the table's feed has landed it in a hand (`arrival`, when it came into one this tick), and
/// publish whether the mod owns its attach bones: while `enabled`, and the weapon is not the
/// dual-wield pair the pistols' own layout owns. Returns the holding hand.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(
    owner: &Character,
    wielded: Option<usize>,
    enabled: bool,
    arrival: Option<live::Arrival>,
) -> Option<Hand> {
    let mut tick = TICK.lock();
    let now = Instant::now();
    let dt = tick
        .last
        .map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
    tick.last = Some(now);
    drop(tick);
    ENABLED.store(enabled, Ordering::Relaxed);
    WIELDED.store(wielded.unwrap_or(0), Ordering::Relaxed);
    if enabled && let Some(arrival) = arrival {
        took(arrival);
        if let (Some(live::Expect::Take(TakeSource::Rack | TakeSource::RackCopy)), Some(address)) =
            (arrival.expect, wielded)
        {
            rack::arrived(address, arrival.hand);
        }
    }
    unsafe { act::tick(owner, wielded, dt) };
    wielded.and_then(owned_holder)
}

/// Leave `hand` expecting the next weapon the game wields, or the one at `weapon` when it is
/// named.
pub(crate) fn begin(hand: Hand, cause: Cause, weapon: Option<usize>) {
    live::expect(
        hand,
        cause.expect(),
        weapon.and_then(ObjectAddress::from_address),
    );
}

/// The weapon at `weapon` came into the hands by a call the mod made (an equip or a pickup): land
/// it in the table at once, rather than at the next input tick, so that the next placement already
/// puts it in its hand. Only while the hold is on, and never for the dual-wield pair, whose own
/// layout owns its bones.
///
/// # Safety
///
/// `weapon` must be the live weapon the game now wields, on the main thread outside the world
/// update.
pub(crate) unsafe fn adopt(weapon: usize) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    WIELDED.store(weapon, Ordering::Relaxed);
    // SAFETY: forwarded from the caller.
    let Some(held) = (unsafe { inventory::held_of(weapon) }) else {
        return;
    };
    if let Some(arrival) = live::adopt(held) {
        took(arrival);
    }
}

/// Hand the wielded weapon to `hand`. Returns whether it moved.
pub(crate) fn hand_over(hand: Hand) -> bool {
    let wielded = WIELDED.load(Ordering::Relaxed);
    let Some(object) = ObjectAddress::from_address(wielded) else {
        return false;
    };
    if !live::hand_over(hand, object) {
        return false;
    }
    TICK.lock().handovers += 1;
    act::arm(hand);
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
    ENABLED.store(false, Ordering::Relaxed);
    WIELDED.store(0, Ordering::Relaxed);
}

/// The hand holding `weapon`, while the mod owns its attach bones.
pub(crate) fn owned_holder(weapon: usize) -> Option<Hand> {
    let owned = ENABLED.load(Ordering::Relaxed) && !dual_wield::attach::owns(weapon);
    owned.then(|| live::hand_of(weapon)).flatten()
}

/// Write the holding hand's bones into `weapon`'s attach-bone table, when the table holds `weapon`
/// in a hand, the mod owns its bones, and it is wielded. With `snap`, a gun whose hand changed since its last placement skips its
/// render interpolation.
///
/// # Safety
///
/// `weapon` must be live, on the thread updating its holder.
pub(crate) unsafe fn apply(weapon: &mut WeaponBase, snap: bool) {
    let address = weapon as *mut WeaponBase as usize;
    if !is_wielded(weapon.m_State) {
        return;
    }
    let Some(holder) = owned_holder(address) else {
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
fn took(arrival: live::Arrival) {
    if arrival.expect.is_some() {
        act::arm(arrival.hand);
    }
    tracing::info!(
        target: "controllers",
        hand = arrival.hand.label(),
        cause = arrival.expect.map_or("the game's draw", live::Expect::label),
        "hold: the weapon is in the {} hand",
        arrival.hand.label()
    );
}

/// The input tick's side.
struct Tick {
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
    last: None,
    handovers: 0,
    pickups: 0,
    pickup_fallbacks: 0,
    last_pickup_refusal: None,
});
static PLACED: Mutex<Option<Placed>> = Mutex::new(None);
static LAST_REFUSAL: Mutex<Option<String>> = Mutex::new(None);
/// Whether the last input tick had the hold on.
static ENABLED: AtomicBool = AtomicBool::new(false);
/// The weapon the game wielded at the last input tick, or zero.
static WIELDED: AtomicUsize = AtomicUsize::new(0);
static WRITES: AtomicU64 = AtomicU64::new(0);
static SNAPS: AtomicU64 = AtomicU64::new(0);

const fn is_wielded(state: WeaponState) -> bool {
    matches!(
        state,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    )
}

/// The weapons of `inventory` the table holds in a hand.
unsafe fn held_in(inventory: *mut Inventory) -> impl Iterator<Item = &'static mut WeaponBase> {
    let hands = Hand::BOTH.map(live::in_hand);
    let weapons = if hands == [None; 2] {
        None
    } else {
        unsafe { inventory.as_ref() }
    };
    weapons
        .into_iter()
        .flat_map(|inventory| inventory.m_Weapons.iter())
        .filter(move |slot| hands.contains(&Some(slot.px as usize)))
        .filter_map(|slot| unsafe { slot.px.as_mut() })
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
    for weapon in unsafe { held_in(inventory) } {
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
    for weapon in unsafe { held_in(inventory) } {
        unsafe { apply(weapon, false) };
    }
    unsafe { second::apply_owned(inventory) };
    unsafe { dual_wield::single::apply_home(inventory) };
    // No inventory ends the pass for the carried weapons.
    unsafe { inventory::carry::end_pass(inventory) };
    // The guns are placed for the pass, so another character's gun is where it is drawn.
    unsafe { steal::observe(inventory) };
}
