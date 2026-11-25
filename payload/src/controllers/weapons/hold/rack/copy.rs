//! A rack with unlimited stock hands out copies. The design is in
//! `docs/mod/input/physical-weapons.md`, "Taking a weapon off a rack", the engine side in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`, "Racks".
//!
//! Starting the pickup of a rack weapon with [`WeaponBase::m_InfinitePickup`] asks the player's
//! inventory for a copy of it ([`WeaponBase::PickupStart`], which gives it through
//! [`Inventory::GivePlayerWeapon`](jc3gi::character::inventory::Inventory::GivePlayerWeapon)) and
//! tells the rack's placement rule that its weapon was picked up; the rack's own weapon stays. A
//! take of one starts that pickup once ([`request`]), inside the rack-member give fence
//! ([`crate::controllers::weapons::inventory::gives`]), so the copy arrives a few frames later
//! without the give's equip, extra ammunition, or replacement drop, and is staged rather than
//! adopted into the loadout: it leaves the inventory and is placed where the rack's weapon hangs,
//! as one standalone weapon ([`staged`]). Meanwhile the rack's weapon is hidden, held alive, and it
//! fades back in a moment after the copy left, restocked.
//!
//! From there the copy is delivered as any weapon its source gave up: a hand in reach takes it at
//! once through the direct pickup, sliding from the rack, and for the grapple's snatch it drops
//! into the world where it hangs, with its bodies, and waits for the line ([`collect`]). One copy
//! is on its way at a time.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use jc3gi::{
    character::character::Character,
    equipment::weapon::{WeaponBase, WeaponState},
    types::math::Matrix4,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    loadout::{Delivery, TakeSource},
    weapons::{
        engine::{self, character_object, slot_of},
        hold::{
            self, SNAP_S,
            pickup::{self, TakeOptions},
            rack::paired::{self, Member},
            refs::Strong,
            source::{Freed, SourceRefusal},
        },
        inventory::{
            engine::{strong_to, weapon_of},
            gives,
        },
    },
};

/// Ask the rack weapon `display` for a copy, to be delivered to `hand` as `delivery` says: start
/// its pickup inside the rack-member give fence and hide it until the copy has left. Returns the
/// copy's member identity, for the snatch to collect it by.
///
/// # Safety
///
/// `owner` must be the live local player and `display` a live rack weapon with unlimited stock,
/// on the game thread before the world update.
pub(crate) unsafe fn request(
    owner: &Character,
    hand: Hand,
    delivery: Delivery,
    display: &mut WeaponBase,
) -> Result<Member, SourceRefusal> {
    if gives::members_closed() {
        return Err(SourceRefusal::CopiesClosed);
    }
    let slot = unsafe { slot_of(display) }.ok_or(SourceRefusal::Gone)?;
    if PENDING.lock().is_some()
        || gives::member_pending()
        || owner.m_Inventory.m_WeaponPendingSpawn[slot as usize]
    {
        return Err(SourceRefusal::CopyPending);
    }
    let gun = unsafe { engine::first_gun_matrix(display) }.ok_or(SourceRefusal::Gone)?;
    let alive = unsafe { strong_to(display) }.ok_or(SourceRefusal::Gone)?;
    let generation = paired::next_generation().ok_or(SourceRefusal::Gone)?;
    let member = Member {
        source: display as *mut WeaponBase as usize,
        generation,
        index: 0,
    };
    *PENDING.lock() = Some(Pending {
        member,
        display: alive,
        hand,
        delivery,
        gun,
        asked_at: Instant::now(),
        stage: Stage::Waiting,
    });
    gives::within_member_start(member, || unsafe {
        display.PickupStart(character_object(owner))
    });
    unsafe { display.SetVisible(false, false, false) };
    COPIES.fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        delivery = delivery.label(),
        "hold: asked a rack for a copy for the {} hand",
        hand.label()
    );
    Ok(member)
}

/// Whether `member` is a copy a rack is spawning.
pub(crate) fn owns(member: Member) -> bool {
    PENDING.lock().as_ref().is_some_and(|p| p.member == member)
}

/// Whether the copy `member` is still waited for.
pub(crate) fn waiting(member: Member) -> bool {
    PENDING
        .lock()
        .as_ref()
        .is_some_and(|p| p.member == member && matches!(p.stage, Stage::Waiting))
}

/// The copy `member` arrived, initialized and out of the inventory: place it where the rack's
/// weapon hangs, and deliver it. A copy nobody waits for any more is despawned.
///
/// # Safety
///
/// `actor` owns a live weapon on the game thread before the world update.
pub(crate) unsafe fn staged(member: Member, actor: Strong) {
    let weapon = unsafe { &mut *weapon_of(&actor) };
    let (hand, delivery, gun) = {
        let pending = PENDING.lock();
        match pending.as_ref() {
            Some(p) if p.member == member && matches!(p.stage, Stage::Waiting) => {
                (p.hand, p.delivery, p.gun)
            }
            _ => {
                drop(pending);
                unsafe { weapon.Despawn() };
                return;
            }
        }
    };
    unsafe { engine::place_guns(weapon, gun) };
    // It is a weapon of its own now: a take of it must not ask for another copy.
    weapon.m_InfinitePickup = false;
    unsafe { weapon.SetState(WeaponState::E_WS_DROPPED, true) };
    let stage = match delivery {
        Delivery::Local => {
            unsafe { deliver(hand, weapon) };
            Stage::Delivered { at: Instant::now() }
        }
        Delivery::Remote => {
            unsafe { weapon.SetVisible(true, true, false) };
            // Into the world where it hangs, with its bodies, for the line to pull in.
            unsafe { weapon.OnDrop() };
            Stage::Freed(actor)
        }
    };
    if let Some(pending) = PENDING.lock().as_mut().filter(|p| p.member == member) {
        pending.stage = stage;
    }
}

/// The copy `member` the snatch waits for: `None` while it has not arrived, else the copy lying in
/// the world, or why it never will.
pub(crate) fn collect(member: Member) -> Option<Result<Freed, SourceRefusal>> {
    let mut pending = PENDING.lock();
    let Some(p) = pending.as_mut().filter(|p| p.member == member) else {
        return Some(Err(SourceRefusal::Gone));
    };
    match std::mem::replace(&mut p.stage, Stage::Delivered { at: Instant::now() }) {
        Stage::Freed(actor) => Some(Ok(Freed {
            weapon: weapon_of(&actor),
            alive: actor,
            source: TakeSource::RackCopy,
        })),
        Stage::Waiting => {
            p.stage = Stage::Waiting;
            None
        }
        Stage::Failed => {
            p.stage = Stage::Failed;
            Some(Err(SourceRefusal::Gone))
        }
        Stage::Delivered { at } => {
            p.stage = Stage::Delivered { at };
            Some(Err(SourceRefusal::Gone))
        }
    }
}

/// Show the rack's weapon again a moment after its copy left, or when the copy never came. Called
/// every input tick.
pub(crate) fn tick() {
    let mut pending = PENDING.lock();
    let Some(p) = pending.as_mut() else {
        return;
    };
    let done = match p.stage {
        Stage::Waiting if p.asked_at.elapsed() >= COPY_WAIT => {
            tracing::info!(target: "controllers", "hold: the rack's copy never came");
            hold::refused("take off a rack: the rack's copy never came");
            p.stage = Stage::Failed;
            false
        }
        Stage::Waiting => false,
        Stage::Delivered { at } => at.elapsed() >= RESTOCK_AFTER,
        // A copy the snatch never collected lies in the world where it hangs.
        Stage::Freed(_) | Stage::Failed => p.asked_at.elapsed() >= 2 * COPY_WAIT,
    };
    if done && let Some(p) = pending.take() {
        drop(pending);
        p.restore();
    }
}

/// Show the rack's weapon again and forget the copy; one that arrives later is despawned. Called
/// when the weapon handling stands down.
pub(crate) fn clear() {
    let pending = PENDING.lock().take();
    if let Some(p) = pending {
        p.restore();
    }
}

/// The copies asked of racks since the payload loaded.
pub(crate) fn count() -> u64 {
    COPIES.load(Ordering::Relaxed)
}

/// How long a rack's weapon stays hidden after its copy left it.
const RESTOCK_AFTER: Duration = Duration::from_millis(800);
/// How long a copy is waited for.
pub(crate) const COPY_WAIT: Duration = Duration::from_secs(3);

/// A copy a rack with unlimited stock is spawning.
struct Pending {
    member: Member,
    /// The rack's own weapon, hidden until the copy has left, held alive meanwhile.
    display: Strong,
    hand: Hand,
    delivery: Delivery,
    /// The rack weapon's first gun's world matrix, where the copy is placed.
    gun: Matrix4,
    asked_at: Instant,
    stage: Stage,
}

enum Stage {
    /// The copy has not arrived.
    Waiting,
    /// The copy lies in the world at the rack, held alive for the snatch.
    Freed(Strong),
    /// The copy left the rack: into a hand, or to the snatch.
    Delivered { at: Instant },
    /// The copy never came.
    Failed,
}

impl Pending {
    /// Fade the rack's weapon back in, when it is still on its rack or where its rule set it down.
    fn restore(self) {
        // SAFETY: the strong reference keeps the weapon alive; called on the game thread before
        // the world update.
        let weapon = unsafe { &mut *weapon_of(&self.display) };
        if matches!(
            weapon.m_State,
            WeaponState::E_WS_WEAPON_RACK | WeaponState::E_WS_DROPPED
        ) {
            unsafe { weapon.SetVisible(true, true, false) };
        }
    }
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static COPIES: AtomicU64 = AtomicU64::new(0);

/// Take the staged copy `weapon` into `hand` at once, sliding from the rack, as the hand's grab
/// asked. A take the hands can no longer make leaves the copy lying at the rack.
///
/// # Safety
///
/// `weapon` must be held alive, on the game thread before the world update.
unsafe fn deliver(hand: Hand, weapon: &mut WeaponBase) {
    let Some(owner) = (unsafe { engine::local_player() }) else {
        unsafe { weapon.Despawn() };
        return;
    };
    let options = TakeOptions {
        slide_s: SNAP_S,
        source: TakeSource::RackCopy,
        delivery: Delivery::Local,
    };
    let taken = unsafe { pickup::preflight(owner, weapon, hand, true) }
        .and_then(|admitted| unsafe { pickup::deliver(owner, hand, weapon, admitted, options) });
    match taken {
        Ok(_) => tracing::info!(
            target: "controllers",
            hand = hand.label(),
            "hold: the rack's copy is in the {} hand",
            hand.label()
        ),
        Err(refusal) => {
            tracing::info!(target: "controllers", %refusal, "hold: the rack's copy came, but the hand cannot take it; it drops at the rack");
            hold::refused(refusal);
            unsafe { weapon.SetVisible(true, true, false) };
            unsafe { weapon.OnDrop() };
        }
    }
}
