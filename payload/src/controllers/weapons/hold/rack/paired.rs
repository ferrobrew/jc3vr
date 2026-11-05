//! Independent members of a native paired weapon rack.
//!
//! Unlimited racks initialize an actor for each member. Finite racks retain the original actor
//! and initialize only one additional actor, with ammunition moved only on an accepted take.

mod finite;

use std::time::{Duration, Instant};

use glam::Mat4;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{ModelAttachementWeaponComponent, WeaponBase, WeaponData, WeaponState},
    graphics_engine::model_instance::ModelInstanceFlags,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    loadout::{live, take::TakeRoute},
    weapons::{
        dual_wield::single::{self, magazine},
        engine::{character_object, place_guns, slot_of},
        hold::{
            pickup::{self, Refusal},
            rack::{copy, offered_state},
            refs::Strong,
            source::SourceRefusal,
            world::{TargetKind, WorldTarget},
        },
        inventory::{engine, gives},
    },
};

/// One member of one observed physical source. A generation never identifies a replacement rack.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub source: usize,
    pub generation: u64,
    pub index: u8,
}

/// The native give being requested for one member, with that member's magazine allocation.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct CopyRequest {
    pub member: Member,
    pub ammo: i32,
}

/// Add a qualifying native pair's separate gun targets. Returns whether it owns this source.
///
/// # Safety
///
/// The source is live on the game thread before the world update.
pub(crate) unsafe fn targets(
    owner: &Character,
    weapon: &WeaponBase,
    alive: Strong,
    to_cockpit: &impl Fn(glam::Vec3) -> glam::Vec3,
    out: &mut Vec<WorldTarget>,
) -> bool {
    if !weapon.m_InfinitePickup {
        return unsafe { finite::targets(owner, weapon, alive, to_cockpit, out) };
    }
    if !unsafe { qualifies(weapon) } {
        return false;
    }
    let callback_busy = gives::member_pending();
    let address = weapon as *const WeaponBase as usize;
    let now = Instant::now();
    let mut state = STATE.lock();
    let at = match state.sources.iter().position(|s| s.address == address) {
        Some(at) => at,
        None if state.sources.len() < MAX_SOURCES => {
            let Some(generation) = state.generation.checked_add(1) else {
                return false;
            };
            state.generation = generation;
            state.sources.push(Source {
                address,
                generation,
                alive,
                seen: now,
                members: [Stage::Empty, Stage::Empty],
            });
            state.sources.len() - 1
        }
        None => return true,
    };
    let source = &mut state.sources[at];
    source.seen = now;
    for index in 0..2 {
        if matches!(source.members[index], Stage::Taken) {
            unsafe { hide_member(source, index) };
        }
        let member = source.member(index);
        if !matches!(source.members[index], Stage::Ready(_)) {
            continue;
        }
        let Some(gun) = (unsafe { gun(weapon, index) }) else {
            continue;
        };
        let Some((points, pose)) = gun_target(gun) else {
            continue;
        };
        out.push(WorldTarget {
            kind: TargetKind::RackMember {
                weapon: address,
                generation: member.generation,
                member: member.index,
            },
            weapon: address,
            slot: WeaponSlot::E_SLOT_DUAL_WIELD,
            points: points.map(to_cockpit),
            gun: pose,
        });
    }
    let available = !state.sources.iter().any(|s| {
        s.members
            .iter()
            .any(|m| matches!(m, Stage::Waiting { .. } | Stage::Taking))
    });
    let request = if available
        && !callback_busy
        && !owner.m_Inventory.m_WeaponPendingSpawn[WeaponSlot::E_SLOT_DUAL_WIELD as usize]
    {
        let source = &mut state.sources[at];
        let index = source
            .members
            .iter()
            .position(|m| matches!(m, Stage::Empty));
        index.map(|index| {
            let member = source.member(index);
            source.members[index] = Stage::Waiting { at: now };
            member
        })
    } else {
        None
    };
    drop(state);
    if let Some(member) = request {
        // PickupStart supplies the native ammo amount; the give detour divides it between members.
        gives::within_member_start(member, || unsafe {
            (*(address as *mut WeaponBase)).PickupStart(character_object(owner));
        });
        tracing::info!(target: "controllers", source = address, generation = member.generation,
            member = member.index, "rack: requested a native source member");
    }
    true
}

/// Split the native rack's allocation, not a fabricated actor's ammunition snapshot.
pub(crate) fn copy_request(member: Member, ammo: i32) -> CopyRequest {
    // A long gun's copy is one weapon, and takes the whole allocation.
    if copy::owns(member) {
        return CopyRequest { member, ammo };
    }
    if finite::owns(member) {
        return CopyRequest { member, ammo: 0 };
    }
    let dealt = magazine::split(ammo, ammo);
    CopyRequest {
        member,
        ammo: if member.index == 0 {
            dealt.slot
        } else {
            dealt.twin
        },
    }
}

/// Whether an unowned finite source must keep its single-member designation.
pub(crate) fn retains_single(object: usize) -> bool {
    finite::retains_single(object)
}

/// Whether an arrival still belongs to a waiting member of the same source generation.
pub(crate) fn waiting(member: Member) -> bool {
    if copy::owns(member) {
        return copy::waiting(member);
    }
    if finite::owns(member) {
        return finite::waiting(member);
    }
    let state = STATE.lock();
    find(&state, member)
        .is_some_and(|s| matches!(s.members[member.index as usize], Stage::Waiting { .. }))
}

/// Keep a native-initialized actor at its source, outside the player's inventory and loadout.
///
/// # Safety
///
/// `actor` owns a live weapon on the game thread before the world update.
pub(crate) unsafe fn staged(member: Member, actor: Strong) {
    if copy::owns(member) {
        unsafe { copy::staged(member, actor) };
        return;
    }
    if finite::owns(member) {
        unsafe { finite::staged(member, actor) };
        return;
    }
    let weapon = unsafe { &mut *engine::weapon_of(&actor) };
    if !two_guns(weapon)
        || unsafe { gun(weapon, 0) }.is_none()
        || unsafe { gun(weapon, 1) }.is_none()
    {
        unsafe { weapon.Despawn() };
        return;
    }
    unsafe { weapon.SetState(WeaponState::E_WS_WEAPON_RACK, true) };
    unsafe { weapon.SetVisible(false, false, false) };
    let mut state = STATE.lock();
    let Some(source) = find_mut(&mut state, member) else {
        drop(state);
        unsafe { weapon.Despawn() };
        return;
    };
    if !matches!(source.members[member.index as usize], Stage::Waiting { .. }) {
        drop(state);
        unsafe { weapon.Despawn() };
        return;
    }
    if let Some(display) = unsafe {
        gun(
            &*(source.address as *const WeaponBase),
            member.index as usize,
        )
    } {
        unsafe { place_guns(weapon, display.m_WorldMatrixT1) };
    }
    source.members[member.index as usize].supply(actor);
    tracing::info!(target: "controllers", source = member.source, generation = member.generation,
        member = member.index, "rack: the initialized member waits at its source");
}

/// A member claimed from its source for a take: a standalone single pistol, out of the rack state,
/// held alive. The take settles it ([`Claim::settle`]): accepted, the member has left its source;
/// refused, the source has it back as before the claim, except that a finite pair's last pistol
/// whose rack already heard its pickup stays a loose single pistol.
pub(crate) struct Claim {
    member: Member,
    actor: Strong,
    kind: ClaimKind,
}

/// What settling a claim undoes or completes.
pub(super) enum ClaimKind {
    /// A staged member of a rack with unlimited stock.
    Unlimited,
    /// A finite pair's staged copy, with the original's ammunition before the split.
    FiniteFirst { magazine: i32, stored: i32 },
    /// A finite pair's original actor, the last pistol: whether its rack heard its pickup.
    FiniteFinal { started: bool },
}

impl Claim {
    pub(super) fn new(member: Member, actor: Strong, kind: ClaimKind) -> Claim {
        Claim {
            member,
            actor,
            kind,
        }
    }

    /// The claimed pistol.
    pub(crate) fn weapon(&self) -> *mut WeaponBase {
        engine::weapon_of(&self.actor)
    }

    /// Settle the claim after its take: `accepted` when the pistol went into the hand.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    pub(crate) unsafe fn settle(self, accepted: bool) {
        let Claim {
            member,
            actor,
            kind,
        } = self;
        match kind {
            ClaimKind::Unlimited => unsafe { settle_unlimited(member, actor, accepted) },
            kind => unsafe { finite::settle(member, actor, kind, accepted) },
        }
    }
}

/// Claim exactly the cached source member for a take into `hand`, never its neighbour or a
/// replacement source, once the take is admitted. Returns the claim and the admitted take.
///
/// # Safety
///
/// The local player and source are live on the game thread before the world update.
pub(crate) unsafe fn claim(
    owner: &Character,
    hand: Hand,
    member: Member,
) -> Result<(Claim, (WeaponSlot, TakeRoute)), SourceRefusal> {
    if finite::owns(member) {
        return unsafe { finite::claim(owner, hand, member) };
    }
    let actor = {
        let mut state = STATE.lock();
        let source = find_mut(&mut state, member).ok_or(SourceRefusal::Gone)?;
        let stage = &mut source.members[member.index as usize];
        stage.take().ok_or(SourceRefusal::CopyPending)?
    };
    let weapon = unsafe { &mut *engine::weapon_of(&actor) };
    // A staged actor is not in the single-pistol inventory watch until the take makes it owned.
    let admitted =
        unsafe { designate_member(weapon) }.and_then(|()| unsafe { admit(owner, weapon, hand) });
    let admitted = match admitted {
        Ok(admitted) => admitted,
        Err(refusal) => {
            if let Some(source) = find_mut(&mut STATE.lock(), member) {
                source.members[member.index as usize].complete(actor, false);
            }
            return Err(SourceRefusal::Take(refusal));
        }
    };
    // The native give already initialized ownership and ammunition; do not request a copy.
    weapon.m_InfinitePickup = false;
    unsafe { weapon.SetState(WeaponState::E_WS_DROPPED, true) };
    Ok((Claim::new(member, actor, ClaimKind::Unlimited), admitted))
}

/// The take of a claimed member into `hand`, admitted as every take is, and only through the table,
/// which holds a single pistol in a hand of its own.
///
/// # Safety
///
/// The owner and weapon are live on the game thread before the world update.
pub(super) unsafe fn admit(
    owner: &Character,
    weapon: &WeaponBase,
    hand: Hand,
) -> Result<(WeaponSlot, TakeRoute), Refusal> {
    let admitted = unsafe { pickup::preflight(owner, weapon, hand, true) }?;
    match admitted.1 {
        TakeRoute::Table { .. } => Ok(admitted),
        TakeRoute::Game(_) => Err(Refusal::NotTaken),
    }
}

/// Settle an unlimited rack's claimed member: hide its gun at the source when it left, or put the
/// staged actor back.
unsafe fn settle_unlimited(member: Member, actor: Strong, accepted: bool) {
    let weapon = unsafe { &mut *engine::weapon_of(&actor) };
    let mut state = STATE.lock();
    let Some(source) = find_mut(&mut state, member) else {
        return;
    };
    if accepted {
        // Hide only the member that left; the other native source gun remains visible.
        unsafe { hide_member(source, member.index as usize) };
    } else {
        unsafe { weapon.SetState(WeaponState::E_WS_WEAPON_RACK, true) };
        unsafe { weapon.SetVisible(false, false, false) };
    }
    source.members[member.index as usize].complete(actor, accepted);
}

/// Retire sources no longer offered, and make failed native requests non-repeating.
pub(crate) fn tick() {
    finite::tick();
    let mut state = STATE.lock();
    let mut retired = Vec::new();
    let mut at = 0;
    while at < state.sources.len() {
        let source = &mut state.sources[at];
        if source.seen.elapsed() >= SOURCE_GRACE {
            retired.push(state.sources.swap_remove(at));
            continue;
        }
        for member in &mut source.members {
            if matches!(member, Stage::Waiting { at } if at.elapsed() >= COPY_WAIT) {
                *member = Stage::Failed;
                tracing::warn!(target: "controllers", "rack: a source member did not arrive; it is not requested again");
            }
        }
        at += 1;
    }
    drop(state);
    for source in retired {
        unsafe { retire(source) };
    }
}

/// Forget the generation before a reset, stand-down, or character replacement.
pub(crate) fn clear() {
    finite::clear();
    let sources = std::mem::take(&mut STATE.lock().sources);
    for source in sources {
        unsafe { retire(source) };
    }
}

unsafe fn designate_member(weapon: &mut WeaponBase) -> Result<(), Refusal> {
    (single::is(weapon as *const WeaponBase as usize)
        || unsafe { single::designate(weapon, "a paired rack member") })
    .then_some(())
    .ok_or(Refusal::NotTaken)
}

const MAX_SOURCES: usize = 8;
const SOURCE_GRACE: Duration = Duration::from_secs(3);
const COPY_WAIT: Duration = Duration::from_secs(3);

struct Source {
    address: usize,
    generation: u64,
    alive: Strong,
    seen: Instant,
    members: [Stage; 2],
}

impl Source {
    fn member(&self, index: usize) -> Member {
        Member {
            source: self.address,
            generation: self.generation,
            index: index as u8,
        }
    }
}

enum Stage<T = Strong> {
    Empty,
    Waiting { at: Instant },
    Ready(T),
    Taking,
    Taken,
    Failed,
}

impl<T> Stage<T> {
    fn supply(&mut self, actor: T) -> bool {
        if !matches!(self, Self::Waiting { .. }) {
            return false;
        }
        *self = Self::Ready(actor);
        true
    }

    fn take(&mut self) -> Option<T> {
        if !matches!(self, Self::Ready(_)) {
            return None;
        }
        match std::mem::replace(self, Self::Taking) {
            Self::Ready(actor) => Some(actor),
            _ => unreachable!(),
        }
    }

    fn complete(&mut self, actor: T, accepted: bool) {
        *self = if accepted {
            Self::Taken
        } else {
            Self::Ready(actor)
        };
    }
}

struct State {
    generation: u64,
    sources: Vec<Source>,
}

static STATE: Mutex<State> = Mutex::new(State {
    generation: 0,
    sources: Vec::new(),
});

pub(crate) fn next_generation() -> Option<u64> {
    let mut state = STATE.lock();
    state.generation = state.generation.checked_add(1)?;
    Some(state.generation)
}

fn find(state: &State, member: Member) -> Option<&Source> {
    (member.index < 2).then_some(())?;
    state
        .sources
        .iter()
        .find(|s| s.address == member.source && s.generation == member.generation)
}

fn find_mut(state: &mut State, member: Member) -> Option<&mut Source> {
    (member.index < 2).then_some(())?;
    state
        .sources
        .iter_mut()
        .find(|s| s.address == member.source && s.generation == member.generation)
}

unsafe fn qualifies(weapon: &WeaponBase) -> bool {
    if !single::active()
        || !live::running()
        || live::resetting()
        || live::mission_holds()
        || weapon.m_State != WeaponState::E_WS_WEAPON_RACK
        || !offered_state(weapon)
        || unsafe { slot_of(weapon) } != Some(WeaponSlot::E_SLOT_DUAL_WIELD)
    {
        return false;
    }
    two_guns(weapon) && unsafe { gun(weapon, 0) }.is_some() && unsafe { gun(weapon, 1) }.is_some()
}

fn two_guns(weapon: &WeaponBase) -> bool {
    let list = &weapon.m_lWeaponData;
    !list.m_Begin.is_null()
        && (list.m_End as usize).checked_sub(list.m_Begin as usize)
            == Some(2 * std::mem::size_of::<*mut WeaponData>())
}

unsafe fn gun(weapon: &WeaponBase, index: usize) -> Option<&WeaponData> {
    if index >= 2 || !two_guns(weapon) {
        return None;
    }
    unsafe { (*weapon.m_lWeaponData.m_Begin.add(index)).as_ref() }
}

fn gun_target(gun: &WeaponData) -> Option<([glam::Vec3; 2], Pose)> {
    let matrix = Mat4::from(gun.m_WorldMatrixT1);
    if !matrix.is_finite() {
        return None;
    }
    let (_, orientation, position) = matrix.to_scale_rotation_translation();
    if !orientation.is_finite() || !orientation.is_normalized() {
        return None;
    }
    // The native gun's own origin is also where the visible member is placed, not its neighbour.
    Some((
        [position; 2],
        Pose {
            position,
            orientation,
        },
    ))
}

unsafe fn hide_member(source: &Source, index: usize) {
    let weapon = unsafe { &mut *(source.address as *mut WeaponBase) };
    let Some(gun) = (unsafe { gun(weapon, index) }) else {
        return;
    };
    if let Some(model) = unsafe { gun.m_BaseModelInstance.as_mut() } {
        model
            .m_Flags
            .remove(ModelInstanceFlags::VISIBLE | ModelInstanceFlags::FLAG_ENABLED);
    }
    for component in unsafe { weapon.m_WeaponComponents.as_slice() } {
        let Some(component) = (unsafe { component.px.as_mut() }) else {
            continue;
        };
        if unsafe { component.IsType(ModelAttachementWeaponComponent::TYPE_ID()) } {
            let part = unsafe { &*(component as *mut _ as *const ModelAttachementWeaponComponent) };
            if part.m_WeaponDataIndex as usize == index {
                unsafe { component.SetVisible(false, false) };
            }
        }
    }
}

unsafe fn retire(source: Source) {
    let weapon = unsafe { &mut *(source.address as *mut WeaponBase) };
    if weapon.m_State == WeaponState::E_WS_WEAPON_RACK {
        unsafe { weapon.SetVisible(true, true, false) };
    }
    for member in source.members {
        if let Stage::Ready(actor) = member {
            unsafe { (*engine::weapon_of(&actor)).Despawn() };
        }
    }
    drop(source.alive);
}

#[cfg(test)]
mod tests {
    use std::mem::MaybeUninit;

    use jc3gi::equipment::weapon::WeaponClass;

    use super::*;
    use crate::controllers::weapons::dual_wield::single::set;

    #[test]
    fn a_full_production_single_set_refuses_before_take_and_restores_ready() {
        let added: Vec<_> = (1..=set::CAPACITY)
            .map(|index| 0x7200_0000 + index)
            .collect();
        for &object in &added {
            assert_eq!(set::insert(object), Ok(true));
        }
        let mut storage = Box::new(MaybeUninit::<WeaponBase>::zeroed());
        // SAFETY: the mapped enums receive valid values before the initialized weapon is borrowed.
        let weapon = unsafe {
            (&raw mut (*storage.as_mut_ptr()).m_Class).write(WeaponClass::E_WEAPON_CLASS_NONE);
            (&raw mut (*storage.as_mut_ptr()).m_EquipSlot).write(WeaponSlot::E_SLOT_DUAL_WIELD);
            let weapon = storage.assume_init_mut();
            let begin = std::ptr::NonNull::<*mut WeaponData>::dangling().as_ptr();
            weapon.m_lWeaponData.m_Begin = begin;
            weapon.m_lWeaponData.m_End = begin.wrapping_add(2);
            weapon
        };
        let mut stage = Stage::Ready(17);
        let actor = stage.take().unwrap();
        let mut taken = false;
        let result = unsafe { designate_member(weapon) }.map(|()| taken = true);
        stage.complete(actor, result.is_ok());
        assert_eq!(result, Err(Refusal::NotTaken));
        assert!(!taken);
        assert!(matches!(stage, Stage::Ready(17)));
        assert!(!single::is(weapon as *const WeaponBase as usize));
        for object in added {
            assert!(set::remove(object));
        }
    }

    #[test]
    fn native_pair_allocation_is_conserved_for_both_grab_orders() {
        for ammo in 0..65 {
            let member = Member {
                source: 17,
                generation: 4,
                index: 0,
            };
            let first = copy_request(member, ammo);
            let second = copy_request(Member { index: 1, ..member }, ammo);
            assert_eq!(first.ammo + second.ammo, ammo);
            assert_eq!(second.member.source, first.member.source);
            assert_eq!(second.member.generation, first.member.generation);
        }
    }

    #[test]
    fn each_native_member_is_acquired_only_by_its_explicit_take() {
        for first in [0, 1] {
            let mut stages = [
                Stage::Waiting { at: Instant::now() },
                Stage::Waiting { at: Instant::now() },
            ];
            assert!(stages[0].supply(101));
            assert!(stages[1].supply(202));
            let other = 1 - first;
            let actor = stages[first].take().unwrap();
            assert!(stages[first].take().is_none());
            assert!(matches!(stages[other], Stage::Ready(_)));
            stages[first].complete(actor, false);
            assert!(
                matches!(stages[first], Stage::Ready(_)),
                "a full hand leaves the member at its source"
            );
            let actor = stages[first].take().unwrap();
            stages[first].complete(actor, true);
            assert!(
                stages[first].take().is_none(),
                "a taken member cannot be taken twice"
            );
            assert!(
                matches!(stages[other], Stage::Ready(_)),
                "the second member is never auto-adopted"
            );
            assert!(
                !stages[first].supply(303),
                "a consumed member is never restaged while its source lives"
            );
        }
    }

    #[test]
    fn canceled_or_failed_native_requests_do_not_accept_arrivals() {
        let mut failed = Stage::Failed;
        assert!(!failed.supply(101));
        assert!(failed.take().is_none());
        let mut empty = Stage::Empty;
        assert!(!empty.supply(202));
        assert!(empty.take().is_none());
    }

    #[test]
    fn stale_or_invalid_members_do_not_match_a_source() {
        let state = State {
            generation: 5,
            sources: Vec::new(),
        };
        for index in [0, 1, 2, u8::MAX] {
            assert!(
                find(
                    &state,
                    Member {
                        source: 17,
                        generation: 4,
                        index
                    }
                )
                .is_none()
            );
        }
    }
}
