//! Taking a weapon off a rack, or out of a locker. The design is in
//! `docs/mod/input/physical-weapons.md`, the engine side in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`.
//!
//! A rack's weapon is placed by its weapon placement rule in the rack state, which keeps no body in
//! the physics world, so the hands' reach cannot find it. The game offers it to the player through
//! the interaction system instead: among the player's valid interaction entries
//! ([`InteractionUserProxy::m_ValidEntries`]), either the weapon's own pickup interaction (a rack
//! that uses the ordinary pickup), or a node standing for the rack's placement rule
//! ([`WeaponPlacementRule`]), whose spawned weapon it is. [`targets`] walks those entries as a
//! swimming character's pickup does, and the weapon handling offers each weapon to a free hand
//! ([`crate::controllers::weapons::hold::world`]). A weapon an entry offers is taken in the rack
//! state or lying in the world (a locker's weapon is offered the same way out of the rack state,
//! presumably set down at a spot by its placement rule); one a character holds, one already being
//! picked up, and a mounted gun, which a placement rule also spawns, are left out.
//!
//! A grab of one takes it ([`take`]), found again among the entries so that it is known alive:
//!
//! - a weapon the rack hands over goes straight into the grabbing hand, as a weapon off the ground
//!   does, after the call that starts the game's own pickup of it ([`WeaponBase::PickupStart`]);
//! - a rack with unlimited stock hands out a copy instead: starting the pickup asks the inventory
//!   for a new weapon of the same kind, which spawns a few frames later and is a give the table
//!   takes into the hand left expecting it, without the game's equip, so the other hand keeps what
//!   it holds. The hand is left pending for it, and the rack's own weapon is hidden from the grab until the copy is
//!   in the hand; the copy then slides into the hand from where the rack's weapon was, so it is
//!   seen to leave the rack, and the rack's weapon fades back in a moment later, restocked.

pub(crate) mod paired;

use std::{
    fmt,
    time::{Duration, Instant},
};

use glam::Vec3;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponState},
    game::GameObject,
    interaction::InteractionGraphEntry,
    physics::PhysicsGameObject,
    weapon_placement_rule::WeaponPlacementRule,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    loadout::slot_label,
    weapons::{
        dual_wield,
        engine::{self, slot_from_raw},
        hold::{
            self, Cause, act,
            pickup::{self, Refusal, TakeOptions},
            refs::Strong,
            steal,
            world::{TargetKind, WorldTarget},
        },
        inventory::engine::slot_of,
    },
};

/// Why a take off a rack did not happen.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RackRefusal {
    /// The weapon is no longer offered on its rack.
    Gone,
    /// A copy is already on its way to a hand.
    CopyPending,
    /// The pickup that puts the weapon in the hand refused it.
    Pickup(Refusal),
}

impl fmt::Display for RackRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RackRefusal::Gone => write!(f, "take off a rack: the weapon is no longer on its rack"),
            RackRefusal::CopyPending => {
                write!(f, "take off a rack: a copy is already on its way to a hand")
            }
            RackRefusal::Pickup(refusal) => write!(f, "take off a rack: {refusal}"),
        }
    }
}

impl std::error::Error for RackRefusal {}

/// The rack weapons the player's interaction entries offer now, with their points carried into the
/// cockpit frame by `to_cockpit`.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn targets(
    owner: &Character,
    to_cockpit: impl Fn(Vec3) -> Vec3,
) -> Vec<WorldTarget> {
    let mut out: Vec<WorldTarget> = Vec::new();
    unsafe {
        walk(owner, |weapon, alive| {
            if paired::targets(owner, weapon, alive, &to_cockpit, &mut out) {
                return false;
            }
            let address = weapon as *const WeaponBase as usize;
            if out.iter().any(|t| t.weapon == address) {
                return false;
            }
            let Some(slot) = rack_slot(weapon) else {
                return false;
            };
            let Some((points, gun)) = steal::gun_points(weapon) else {
                return false;
            };
            out.push(WorldTarget {
                kind: TargetKind::Rack { weapon: address },
                weapon: address,
                slot,
                points: points.map(&to_cockpit),
                gun,
            });
            false
        })
    };
    out
}

/// Take `target`'s weapon off its rack into `hand`.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn take(
    hand: Hand,
    target: &WorldTarget,
    options: TakeOptions,
) -> Result<WeaponSlot, RackRefusal> {
    let owner =
        unsafe { engine::local_player() }.ok_or(RackRefusal::Pickup(Refusal::NoCharacter))?;
    // Find the weapon again among the entries, and hold it alive through the take.
    let mut found = None;
    unsafe {
        walk(owner, |weapon, strong| {
            if weapon as *const WeaponBase as usize == target.weapon {
                found = Some(strong);
                return true;
            }
            false
        })
    };
    let Some(alive) = found else {
        return Err(RackRefusal::Gone);
    };
    // SAFETY: the strong reference keeps the weapon alive.
    let weapon = unsafe { &mut *(target.weapon as *mut WeaponBase) };
    if !offered_state(weapon) {
        return Err(RackRefusal::Gone);
    }
    if let TargetKind::RackMember {
        weapon: source,
        generation,
        member,
    } = target.kind
    {
        if source != target.weapon || weapon.m_State != WeaponState::E_WS_WEAPON_RACK {
            return Err(RackRefusal::Gone);
        }
        return unsafe {
            paired::take(
                owner,
                hand,
                paired::Member {
                    source,
                    generation,
                    index: member,
                },
                options,
            )
        };
    }
    // A copy is asked of a rack with unlimited stock; anything else hands its own weapon over.
    let restocks = weapon.m_InfinitePickup && weapon.m_State == WeaponState::E_WS_WEAPON_RACK;
    if !restocks {
        let slot = unsafe { pickup::take(hand, weapon, options) }.map_err(RackRefusal::Pickup)?;
        RACK_TAKES.lock().takes += 1;
        tracing::info!(
            target: "controllers",
            hand = hand.label(),
            slot = slot_label(slot),
            "hold: took the weapon off a rack into the {} hand",
            hand.label()
        );
        return Ok(slot);
    }
    if COPY.lock().is_some() {
        return Err(RackRefusal::CopyPending);
    }
    let slot = rack_slot(weapon).ok_or(RackRefusal::Pickup(Refusal::UnknownSlot))?;
    if !unsafe { owner.m_Inventory.GetAnimationAllowsPickup() } {
        return Err(RackRefusal::Pickup(Refusal::AnimationForbids));
    }
    let from = unsafe { pickup::gun_pose(weapon) };
    // The copy is a give, which the table takes into the hand left expecting it
    // ([`crate::controllers::weapons::inventory::gives`]); the slot's weapon keeps its place.
    hold::begin(hand, Cause::Rack, None);
    unsafe { weapon.PickupStart(pickup::activator(owner)) };
    unsafe { weapon.SetVisible(false, false, false) };
    *COPY.lock() = Some(Copy {
        display: alive,
        display_weapon: target.weapon,
        hand,
        from,
        slide_s: options.slide_s,
        asked_at: Instant::now(),
        arrived_at: None,
    });
    RACK_TAKES.lock().copies += 1;
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        slot = slot_label(slot),
        "hold: asked a rack for a copy for the {} hand",
        hand.label()
    );
    Ok(slot)
}

/// A weapon the game wielded came into `hand` on a rack's grab: when a copy was asked for that
/// hand, slide it in from where the rack's weapon was. A copy of a pistol is one pistol while
/// pistols are single.
pub(crate) fn arrived(weapon: usize, hand: Hand) {
    let mut copy = COPY.lock();
    let Some(pending) = copy
        .as_mut()
        .filter(|c| c.hand == hand && c.arrived_at.is_none())
    else {
        return;
    };
    pending.arrived_at = Some(Instant::now());
    // SAFETY: the weapon the game wields this input tick is live.
    if let Some(copy) = unsafe { (weapon as *mut WeaponBase).as_mut() } {
        unsafe { dual_wield::single::rack_copy_arrived(copy) };
    }
    if let Some(from) = pending.from {
        act::slide_in(weapon, hand, from, pending.slide_s);
    }
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        "hold: the rack's copy is in the {} hand",
        hand.label()
    );
}

/// Show the rack's weapon again once its copy has been in the hand a moment, or when the copy
/// never came. Called every input tick.
pub(crate) fn tick() {
    paired::tick();
    let mut copy = COPY.lock();
    let Some(pending) = copy.as_ref() else {
        return;
    };
    let due = match pending.arrived_at {
        Some(at) => at.elapsed() >= RESTOCK_AFTER,
        None => pending.asked_at.elapsed() >= COPY_WAIT,
    };
    if !due {
        return;
    }
    let pending = copy.take();
    drop(copy);
    if let Some(pending) = pending {
        if pending.arrived_at.is_none() {
            tracing::info!(target: "controllers", "hold: the rack's copy never came");
            hold::refused("take off a rack: the rack's copy never came");
        }
        pending.restore();
    }
}

/// Show the rack's weapon again and forget the copy. Called when the weapon handling stands down.
pub(crate) fn stand_down() {
    paired::clear();
    let pending = COPY.lock().take();
    if let Some(pending) = pending {
        pending.restore();
    }
}

/// The takes off racks since the payload loaded: weapons handed over, and copies asked for.
pub(crate) fn counts() -> (u64, u64) {
    let counts = RACK_TAKES.lock();
    (counts.takes, counts.copies)
}

/// How long a rack's weapon stays hidden after its copy is in the hand.
const RESTOCK_AFTER: Duration = Duration::from_millis(800);
/// How long a copy is waited for before the rack's weapon is shown again.
const COPY_WAIT: Duration = Duration::from_secs(3);

/// A copy a rack with unlimited stock is spawning for a hand.
struct Copy {
    /// The rack's own weapon, hidden until the copy is in the hand, held alive meanwhile.
    display: Strong,
    display_weapon: usize,
    hand: Hand,
    /// Where the rack's weapon was, for the copy's slide.
    from: Option<Pose>,
    slide_s: f32,
    asked_at: Instant,
    arrived_at: Option<Instant>,
}

impl Copy {
    /// Fade the rack's weapon back in, when it is still on its rack.
    fn restore(self) {
        // SAFETY: the strong reference keeps the weapon alive; called on the main thread outside
        // the world update.
        let weapon = unsafe { &mut *(self.display_weapon as *mut WeaponBase) };
        if weapon.m_State == WeaponState::E_WS_WEAPON_RACK {
            unsafe { weapon.SetVisible(true, true, false) };
        }
        drop(self.display);
    }
}

struct Counts {
    takes: u64,
    copies: u64,
}

static COPY: Mutex<Option<Copy>> = Mutex::new(None);
static RACK_TAKES: Mutex<Counts> = Mutex::new(Counts {
    takes: 0,
    copies: 0,
});

/// Walk the weapons `owner`'s valid interaction entries offer that sit on a rack, with a strong
/// reference holding each alive, until `visit` returns true. A weapon whose own pickup interaction
/// is the entry is offered directly, and a rack's placement rule offers the weapon it spawned.
unsafe fn walk(owner: &Character, mut visit: impl FnMut(&WeaponBase, Strong) -> bool) {
    let entries = &owner.m_InteractionUserProxy.m_ValidEntries;
    let (mut at, end) = (entries.m_Begin, entries.m_End);
    if at.is_null() || end < at {
        return;
    }
    let mut seen = 0;
    while at < end && seen < MAX_ENTRIES {
        // SAFETY: the pointer is inside the proxy's live entry array.
        let entry = unsafe { *at };
        at = unsafe { at.add(1) };
        seen += 1;
        let Some(strong) = (unsafe { offered(entry) }) else {
            continue;
        };
        let weapon = unsafe { weapon_of(strong.object()) };
        if let Some(weapon) = weapon.filter(|w| offered_state(w))
            && visit(weapon, strong)
        {
            return;
        }
    }
}

/// The slot of the first weapon `owner`'s valid interaction entries offer, as a swimming
/// character's pickup takes it, with whether the table would take that weapon.
///
/// # Safety
///
/// `owner` must be the live local player character.
pub(crate) unsafe fn first_offered(
    owner: &Character,
    takes: impl Fn(&WeaponBase) -> bool,
) -> Option<WeaponSlot> {
    let mut found = None;
    unsafe {
        walk(owner, |weapon, _| {
            found = takes(weapon).then(|| slot_of(weapon)).flatten();
            true
        })
    };
    found
}

/// The most entries a walk visits, a bound against a corrupt array.
const MAX_ENTRIES: usize = 256;

/// Whether an offered weapon in `weapon`'s state may be taken: any but one a character holds,
/// wields, or carries, and one already being picked up.
fn offered_state(weapon: &WeaponBase) -> bool {
    // Read as the raw value, so that a state the enum does not name is not misread.
    let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
    ![
        WeaponState::E_WS_WIELDING,
        WeaponState::E_WS_DUAL_WIELDING,
        WeaponState::E_WS_HOLSTERED,
        WeaponState::E_WS_HOLSTERED_INVISIBLE,
        WeaponState::E_WS_PICKING_UP,
        WeaponState::E_WS_WINGSUIT_WEAPON,
        WeaponState::E_WS_MOUNTED,
        WeaponState::E_WS_TURRET,
        WeaponState::E_WS_VEHICLE,
    ]
    .into_iter()
    .any(|s| s as i32 == state)
}

/// The weapon `entry` offers, held alive: its context's own weapon, or the weapon its entry node's
/// placement rule spawned.
unsafe fn offered(entry: *mut InteractionGraphEntry) -> Option<Strong> {
    let entry = unsafe { entry.as_ref() }?;
    let graph = unsafe { entry.m_Graph.as_mut() }?;
    if let Some(context) = unsafe { graph.m_Context.as_ref() }
        && let Some(parent) = unsafe { Strong::lock(&context.m_Parent) }
        && unsafe { (*parent.object()).IsType(WeaponBase::TYPE_ID()) }
    {
        return Some(parent);
    }
    let node = unsafe { graph.GetNodeByIndex(entry.m_EntryNodeIndex).as_ref() }?;
    let object = unsafe { Strong::lock(&node.m_GameObject) }?;
    if !unsafe { (*object.object()).IsType(WeaponPlacementRule::TYPE_ID()) } {
        return None;
    }
    // A rule's `CGameObject` subobject is at its start.
    let rule = object.object() as *const WeaponPlacementRule;
    let spawned = unsafe {
        Strong::filled(|out| {
            (*rule).GetSpawnedObject(out, WeaponBase::TYPE_ID(), 0);
        })
    }?;
    unsafe { (*spawned.object()).IsType(WeaponBase::TYPE_ID()) }.then_some(spawned)
}

/// The weapon whose `CGameObject` subobject is `object`.
unsafe fn weapon_of<'a>(object: *mut GameObject) -> Option<&'a WeaponBase> {
    let address =
        (object as usize).checked_sub(PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize)?;
    unsafe { (address as *const WeaponBase).as_ref() }
}

/// The slot a rack weapon goes into, for the slots a hand holds.
fn rack_slot(weapon: &WeaponBase) -> Option<WeaponSlot> {
    // Read as the raw value: an unset slot is out of the enum's range.
    let slot = slot_from_raw(unsafe { (&raw const weapon.m_EquipSlot).cast::<i32>().read() })?;
    matches!(
        slot,
        WeaponSlot::E_SLOT_DUAL_WIELD
            | WeaponSlot::E_SLOT_TWO_HANDED
            | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
    )
    .then_some(slot)
}
