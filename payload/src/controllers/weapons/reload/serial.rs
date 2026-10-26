//! Bounded magazine requests consumed by the native serial post-physics character callback.
//!
//! Input publishes identities only. The callback resolves the current owned weapon and spare again
//! before detaching, and completion delivers the retained prop through the ordinary magazine flow.

use std::time::Instant;

use jc3gi::{
    character::character::Character,
    equipment::weapon::{WeaponBase, WeaponData, WeaponState},
    game::GameObjectUpdateContext,
    game_world::GameWorld,
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        aim::gun,
        frame::{self, Button, Hand},
        hands::late,
        loadout::live,
        mode, scheme,
        weapons::{
            capabilities::{Capabilities, GunHand, gun_owners_now},
            hold::{armed, refs::Strong},
            inventory::engine::strong_to,
            reload::{
                engine::{self, DetachOutcome},
                extraction::{DetachKind, DetachRequest, MagazineId},
                loose::Retention,
            },
        },
    },
};

/// Object identities sampled while the magazine is still inserted. These values confer no access.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Identity {
    character: usize,
    character_control: usize,
    model: usize,
    world: usize,
    slot: usize,
    weapon_control: usize,
    weapon_name: u32,
    gun: usize,
    spare: usize,
    spare_control: usize,
    tag: i32,
}

impl Identity {
    pub(super) fn has_spare(self) -> bool {
        self.spare != 0 && self.spare_control != 0
    }

    /// The weapon and gun remain the same after their spare has been dropped and replaced.
    pub(super) fn same_gun(self, other: Self) -> bool {
        Self {
            spare: other.spare,
            spare_control: other.spare_control,
            ..self
        } == other
    }
}

#[cfg(test)]
impl Identity {
    pub(super) const fn test() -> Self {
        Self {
            character: 1,
            character_control: 2,
            model: 3,
            world: 4,
            slot: 0,
            weapon_control: 5,
            weapon_name: 6,
            gun: 7,
            spare: 8,
            spare_control: 9,
            tag: 10,
        }
    }
}

/// Input-side snapshot. A weapon absent from the live inventory is not dereferenced.
///
/// # Safety
///
/// Called on the game input thread while inventory and weapon ownership are stable.
pub(super) unsafe fn capture(id: MagazineId) -> Option<Identity> {
    let character = unsafe { Character::GetLocalPlayerCharacter() };
    unsafe { resolve(character, id) }.map(|(identity, _)| identity)
}

/// Runtime evidence from the native consumer, copied without exposing engine objects.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct SerialStatus {
    pub installed: bool,
    pub accepting: bool,
    pub pending: [bool; 2],
    pub executing: [bool; 2],
    pub completed: [bool; 2],
    pub callbacks: u64,
    pub submitted: u64,
    pub executed: u64,
    pub refused: u64,
    pub canceled: u64,
    pub last_refusal: Option<Refusal>,
}

impl SerialStatus {
    pub(super) const fn new() -> Self {
        Self {
            installed: false,
            accepting: false,
            pending: [false; 2],
            executing: [false; 2],
            completed: [false; 2],
            callbacks: 0,
            submitted: 0,
            executed: 0,
            refused: 0,
            canceled: 0,
            last_refusal: None,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Refusal {
    Admission,
    Retention,
    Paused,
    Permission,
    Identity,
    NoThrow,
}

pub fn status() -> SerialStatus {
    let state = MAILBOX.lock();
    SerialStatus {
        installed: state.hands.iter().all(|h| h.installed),
        accepting: state.hands.iter().all(|h| h.accepting),
        pending: state
            .hands
            .each_ref()
            .map(|h| matches!(h.slot, Some(Slot::Pending(_)))),
        executing: state
            .hands
            .each_ref()
            .map(|h| matches!(h.slot, Some(Slot::Executing { .. }))),
        completed: state
            .hands
            .each_ref()
            .map(|h| matches!(h.slot, Some(Slot::Complete { .. }))),
        ..state.status
    }
}

/// Called by hook installation only after the character library is enabled.
pub(crate) fn set_installed(installed: bool) {
    for hand in &mut MAILBOX.lock().hands {
        hand.installed = installed;
        hand.accepting = installed;
        if !installed {
            hand.cancel_all();
        }
    }
}

pub(super) fn available() -> bool {
    MAILBOX
        .lock()
        .hands
        .iter()
        .all(|h| h.installed && h.accepting)
}

pub(super) fn submit(request: DetachRequest) -> bool {
    submit_with_retention(request, true)
}

pub(super) fn submit_with_retention(request: DetachRequest, retain: bool) -> bool {
    let mut state = MAILBOX.lock();
    let hand = request.id.holder.index();
    if !state.hands[hand].can_submit(request) {
        state.refuse(Refusal::Admission);
        return false;
    }
    let retention = if retain {
        let Some(retention) = Retention::reserve(request.expected.gun, request.expected.spare)
        else {
            state.refuse(Refusal::Retention);
            return false;
        };
        Some(retention)
    } else {
        None
    };
    state.hands[hand].retention = retention;
    let submitted = state.hands[hand].submit(request);
    state.status.submitted += u64::from(submitted);
    submitted
}

pub(super) fn poll(request: DetachRequest) -> Option<DetachOutcome> {
    MAILBOX.lock().hands[request.id.holder.index()].poll(request)
}

pub(super) fn cancel(request: DetachRequest) {
    let mut state = MAILBOX.lock();
    state.hands[request.id.holder.index()].cancel(request);
    state.status.canceled += 1;
}

/// Close admission and cancel pending work. Hook removal remains the hook owner's responsibility.
pub(crate) fn stop_and_drain() -> bool {
    let mut state = MAILBOX.lock();
    let mut drained = true;
    for hand in &mut state.hands {
        drained &= hand.stop_and_drain();
    }
    drained
}

/// Consume at most one request per gun hand before the native message poll.
///
/// # Safety
///
/// Called only at `Character::UpdatePassPostPhysics_Serial` entry with its native arguments.
pub(crate) unsafe fn consume(character: *mut Character, context: *mut GameObjectUpdateContext) {
    if character.is_null() || unsafe { Character::GetLocalPlayerCharacter() } != character {
        return;
    }
    let paused = unsafe { context.as_ref() }.is_none_or(|context| context.m_Paused);
    MAILBOX.lock().status.callbacks += 1;
    for hand in Hand::BOTH {
        let work = {
            let mut state = MAILBOX.lock();
            let queue = &mut state.hands[hand.index()];
            queue
                .begin()
                .map(|request| (request, queue.retention.take()))
        };
        let Some((request, retention)) = work else {
            continue;
        };
        let resolved = if paused {
            None
        } else {
            unsafe { resolve(character, request.id) }
        };
        let refusal = if paused {
            Some(Refusal::Paused)
        } else if resolved
            .as_ref()
            .is_none_or(|(identity, _)| *identity != request.expected || !identity.has_spare())
        {
            Some(Refusal::Identity)
        } else if !permitted(request, retention.is_some()) {
            Some(Refusal::Permission)
        } else {
            None
        };
        let authorized = MAILBOX.lock().hands[hand.index()].authorize(request, refusal.is_none());
        let outcome = if authorized {
            // SAFETY: the native serial phase owns execution; resolve found the current inventory
            // entry and its exact gun and spare, rather than borrowing any queued address.
            unsafe { engine::detach_serial(request.id.weapon, request.id.gun, retention.as_ref()) }
        } else {
            DetachOutcome::NoThrow
        };
        drop(resolved);
        let mut state = MAILBOX.lock();
        if let Some(refusal) = refusal.or_else(|| {
            (authorized && outcome == DetachOutcome::NoThrow).then_some(Refusal::NoThrow)
        }) {
            state.refuse(refusal);
        }
        state.status.executed += u64::from(authorized);
        let queue = &mut state.hands[hand.index()];
        queue.retention = retention;
        queue.complete(request, outcome);
    }
}

fn permitted(request: DetachRequest, retain: bool) -> bool {
    let pull = request.kind == DetachKind::Pull;
    if crate::is_shutting_down()
        || !scheme::is_controllers()
        || !Capabilities::of(mode::current().mode).reload
        || !Config::lock_query(|c| {
            let reload = &c.controllers.weapons.reload;
            reload.enabled
                && (!retain || reload.loose.enabled)
                && (!pull || reload.extraction.enabled)
                && (request.kind != DetachKind::Belt || reload.belt.enabled)
        })
    {
        return false;
    }
    let tracked = frame::with_latest(|frame| {
        frame.focused
            && frame.is_fresh_at(Instant::now())
            && (!pull
                || frame
                    .hand(request.id.holder.other())
                    .buttons
                    .contains(Button::SqueezePress))
            && [request.id.holder, request.id.holder.other()]
                .into_iter()
                .all(|hand| {
                    let tracked = frame.hand(hand);
                    (!pull && hand != request.id.holder)
                        || (tracked.active && tracked.grip.is_tracked())
                })
    })
    .unwrap_or(false);
    if !tracked {
        return false;
    }
    let owner = gun_owners_now()[request.id.holder.index()];
    owner.is_some_and(|owner| {
        owner.weapon == request.id.weapon
            && matches!(owner.role, GunHand::Grip | GunHand::NativePair(_))
    }) && (!pull
        || (live::in_hand(request.id.holder.other()).is_none()
            && armed::in_hand(request.id.holder.other()).is_none()))
}

unsafe fn resolve(character: *mut Character, id: MagazineId) -> Option<(Identity, Strong)> {
    if character.is_null() || unsafe { Character::GetLocalPlayerCharacter() } != character {
        return None;
    }
    let world = {
        let world = unsafe { GameWorld::get() }?;
        if unsafe { world.IsTeleporting() } {
            return None;
        }
        world as *mut GameWorld as usize
    };
    let owner = unsafe { character.as_ref() }?;
    if !owner.m_IsLocalCharacter || owner.m_Inventory.m_Character != character {
        return None;
    }
    let inventory = owner
        .m_Inventory
        .m_Weapons
        .iter()
        .enumerate()
        .find(|(_, p)| p.px as usize == id.weapon && !p.px.is_null() && !p.pn.is_null());
    let (slot, weapon, weapon_control) = if let Some((slot, shared)) = inventory {
        (slot, unsafe { shared.px.as_ref() }?, shared.pn as usize)
    } else {
        let weapon = unsafe { armed::weapon_of(owner, id.holder) }?;
        if weapon as *mut WeaponBase as usize != id.weapon {
            return None;
        }
        // The existing carry set owns this non-slot weapon throughout the character update.
        (usize::MAX, &*weapon, 0)
    };
    if !std::ptr::eq(weapon.m_User[0].cast::<Character>(), character) || weapon.m_User[1].is_null()
    {
        return None;
    }
    let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
    let selected = unsafe { (&raw const owner.m_SelectedWeaponSlot).cast::<i32>().read() };
    let wielded = selected == slot as i32
        && (state == WeaponState::E_WS_WIELDING as i32
            || state == WeaponState::E_WS_DUAL_WIELDING as i32);
    let armed_here =
        state == WeaponState::E_WS_HOLSTERED as i32 && armed::in_hand(id.holder) == Some(id.weapon);
    if !wielded && !armed_here {
        return None;
    }
    let gun = unsafe { gun::guns(weapon) }.nth(id.gun).flatten()?;
    let bone = gun
        .m_lBoneAttachementPerWeaponState
        .get(usize::try_from(state).ok()?)?;
    if late::attach_hand(*bone) != Some(id.holder)
        || gun.m_Owner as usize != id.weapon
        || gun.m_MagazineUniqueTag == WeaponData::NO_MAGAZINE_UNIQUE_TAG
    {
        return None;
    }
    let strong = unsafe { strong_to(weapon) }?;
    let weapon_control = if weapon_control == 0 {
        strong.control_block() as usize
    } else {
        weapon_control
    };
    Some((
        Identity {
            character: character as usize,
            character_control: weapon.m_User[1] as usize,
            model: owner.m_AnimatedModel.m_InstanceData as usize,
            world,
            slot,
            weapon_control,
            weapon_name: weapon.m_WeaponNameHash,
            gun: gun as *const WeaponData as usize,
            spare: gun.m_MagazineObject.px as usize,
            spare_control: gun.m_MagazineObject.pn as usize,
            tag: gun.m_MagazineUniqueTag,
        },
        strong,
    ))
}

#[derive(Debug, Copy, Clone)]
enum Slot {
    Pending(DetachRequest),
    Executing {
        request: DetachRequest,
        canceled: bool,
    },
    Complete {
        request: DetachRequest,
        outcome: DetachOutcome,
    },
}

struct Mailbox<L = Retention> {
    installed: bool,
    accepting: bool,
    high_water: u64,
    slot: Option<Slot>,
    retention: Option<L>,
}

impl<L> Mailbox<L> {
    const fn new() -> Self {
        Self {
            installed: false,
            accepting: false,
            high_water: 0,
            slot: None,
            retention: None,
        }
    }

    fn can_submit(&self, request: DetachRequest) -> bool {
        self.installed
            && self.accepting
            && self.slot.is_none()
            && request.generation > self.high_water
    }

    fn submit(&mut self, request: DetachRequest) -> bool {
        if !self.can_submit(request) {
            return false;
        }
        self.high_water = request.generation;
        self.slot = Some(Slot::Pending(request));
        true
    }

    fn poll(&mut self, request: DetachRequest) -> Option<DetachOutcome> {
        let Some(Slot::Complete {
            request: actual,
            outcome,
        }) = self.slot
        else {
            return None;
        };
        if actual != request {
            return None;
        }
        self.slot = None;
        self.retention = None;
        Some(outcome)
    }

    fn cancel(&mut self, request: DetachRequest) {
        match self.slot {
            Some(Slot::Pending(r)) | Some(Slot::Complete { request: r, .. }) if r == request => {
                self.slot = None;
                self.retention = None;
            }
            Some(Slot::Executing { request: r, .. }) if r == request => {
                self.slot = Some(Slot::Executing {
                    request,
                    canceled: true,
                });
            }
            _ => {}
        }
    }

    fn cancel_all(&mut self) {
        self.slot = match self.slot {
            Some(Slot::Executing { request, .. }) => Some(Slot::Executing {
                request,
                canceled: true,
            }),
            _ => None,
        };
        if self.slot.is_none() {
            self.retention = None;
        }
    }

    fn stop_and_drain(&mut self) -> bool {
        self.accepting = false;
        self.cancel_all();
        self.slot.is_none()
    }

    fn begin(&mut self) -> Option<DetachRequest> {
        if !self.installed || !self.accepting {
            return None;
        }
        let Some(Slot::Pending(request)) = self.slot else {
            return None;
        };
        self.slot = Some(Slot::Executing {
            request,
            canceled: false,
        });
        Some(request)
    }

    fn authorize(&self, request: DetachRequest, valid: bool) -> bool {
        valid
            && self.accepting
            && matches!(self.slot,
            Some(Slot::Executing { request: r, canceled: false }) if r == request)
    }

    fn complete(&mut self, request: DetachRequest, outcome: DetachOutcome) {
        if let Some(Slot::Executing {
            request: actual,
            canceled,
        }) = self.slot
            && actual == request
        {
            // Cancellation never reverses a detach or removes its world prop and recorded rounds.
            self.slot =
                (!canceled && self.accepting).then_some(Slot::Complete { request, outcome });
            if self.slot.is_none() || !matches!(outcome, DetachOutcome::Retained(_)) {
                self.retention = None;
            }
        }
    }
}

struct Queues {
    hands: [Mailbox; 2],
    status: SerialStatus,
}

impl Queues {
    fn refuse(&mut self, refusal: Refusal) {
        self.status.refused += 1;
        self.status.last_refusal = Some(refusal);
        tracing::debug!(target: "controllers", ?refusal, "reload: serial magazine request refused");
    }
}

static MAILBOX: Mutex<Queues> = Mutex::new(Queues {
    hands: [Mailbox::new(), Mailbox::new()],
    status: SerialStatus::new(),
});

#[cfg(test)]
mod retention_tests;
#[cfg(test)]
mod tests;
