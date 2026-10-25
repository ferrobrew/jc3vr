//! Magazine request admission and identity capture.
//!
//! Production admission stays closed until hook entry exclusion through removal is proven.
//! Execution and completion transitions remain available only to isolated mailbox tests.

use jc3gi::{
    character::character::Character,
    equipment::weapon::{WeaponData, WeaponState},
    game_world::GameWorld,
};
use parking_lot::Mutex;

use crate::controllers::{
    aim::gun,
    hands::late,
    weapons::{
        hold::armed,
        reload::{
            engine::DetachOutcome,
            extraction::{DetachRequest, MagazineId},
            loose::Retention,
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
    unsafe { resolve(character, id) }
}

pub(super) fn available() -> bool {
    let state = MAILBOX.lock();
    state.installed && state.accepting
}

pub(super) fn submit(request: DetachRequest) -> bool {
    let mut state = MAILBOX.lock();
    if !state.can_submit(request) {
        return false;
    }
    let Some(retention) = Retention::reserve(request.expected.gun, request.expected.spare) else {
        return false;
    };
    state.retention = Some(retention);
    state.submit(request)
}

pub(super) fn poll(request: DetachRequest) -> Option<DetachOutcome> {
    MAILBOX.lock().poll(request)
}

pub(super) fn cancel(request: DetachRequest) {
    MAILBOX.lock().cancel(request);
}

/// Close admission and cancel queued work before lifecycle preparation.
/// An installed callback cannot report readiness without a proven entry-exclusion adapter.
pub(crate) fn stop_and_drain() -> bool {
    MAILBOX.lock().stop_and_drain()
}

unsafe fn resolve(character: *mut Character, id: MagazineId) -> Option<Identity> {
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
    let (slot, shared) = owner
        .m_Inventory
        .m_Weapons
        .iter()
        .enumerate()
        .find(|(_, p)| p.px as usize == id.weapon && !p.px.is_null() && !p.pn.is_null())?;
    let weapon = unsafe { shared.px.as_ref() }?;
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
        || gun.m_Owner != shared.px
        || gun.m_MagazineUniqueTag == WeaponData::NO_MAGAZINE_UNIQUE_TAG
    {
        return None;
    }
    Some(Identity {
        character: character as usize,
        character_control: weapon.m_User[1] as usize,
        model: owner.m_AnimatedModel.m_InstanceData as usize,
        world,
        slot,
        weapon_control: shared.pn as usize,
        weapon_name: weapon.m_WeaponNameHash,
        gun: gun as *const WeaponData as usize,
        spare: gun.m_MagazineObject.px as usize,
        spare_control: gun.m_MagazineObject.pn as usize,
        tag: gun.m_MagazineUniqueTag,
    })
}

#[derive(Debug, Copy, Clone)]
enum Slot {
    Pending(DetachRequest),
    #[cfg(test)]
    Executing {
        request: DetachRequest,
        canceled: bool,
    },
    #[cfg(test)]
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

    #[cfg(not(test))]
    fn poll(&mut self, _: DetachRequest) -> Option<DetachOutcome> {
        None
    }

    #[cfg(test)]
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

    #[cfg(not(test))]
    fn cancel(&mut self, request: DetachRequest) {
        if matches!(self.slot, Some(Slot::Pending(actual)) if actual == request) {
            self.slot = None;
            self.retention = None;
        }
    }

    #[cfg(test)]
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

    #[cfg(not(test))]
    fn cancel_all(&mut self) {
        self.slot = None;
        self.retention = None;
    }

    #[cfg(test)]
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
        !self.installed && self.slot.is_none()
    }

    #[cfg(test)]
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

    #[cfg(test)]
    fn authorize(&self, request: DetachRequest, valid: bool) -> bool {
        valid
            && self.accepting
            && matches!(self.slot,
            Some(Slot::Executing { request: r, canceled: false }) if r == request)
    }

    #[cfg(test)]
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

static MAILBOX: Mutex<Mailbox> = Mutex::new(Mailbox::new());

#[cfg(test)]
mod retention_tests;
#[cfg(test)]
mod tests;
