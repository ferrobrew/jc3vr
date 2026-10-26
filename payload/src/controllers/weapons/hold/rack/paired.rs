//! Independent members of a native unlimited-stock pistol rack.
//!
//! The rack's two native guns remain the visible source. Each member gets one actor through the
//! existing native give path, held outside the loadout until that member is explicitly grabbed.

use std::time::{Duration, Instant};

use glam::Mat4;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{ModelAttachementWeaponComponent, WeaponBase, WeaponData, WeaponState},
    game::GameObject,
    graphics_engine::model_instance::ModelInstanceFlags,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    loadout::{TakeSource, live, take::TakeRoute},
    weapons::{
        dual_wield::single::{self, magazine},
        hold::{
            Cause,
            pickup::{self, Refusal, TakeOptions},
            rack::{RackRefusal, offered_state},
            refs::Strong,
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
/// The source is live on the main thread outside the world update.
pub(crate) unsafe fn targets(
    owner: &Character,
    weapon: &WeaponBase,
    alive: Strong,
    to_cockpit: &impl Fn(glam::Vec3) -> glam::Vec3,
    out: &mut Vec<WorldTarget>,
) -> bool {
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
            (*(address as *mut WeaponBase))
                .PickupStart(engine::player_object(owner) as *mut GameObject);
        });
        tracing::info!(target: "controllers", source = address, generation = member.generation,
            member = member.index, "rack: requested a native source member");
    }
    true
}

/// Split the native rack's allocation, not a fabricated actor's ammunition snapshot.
pub(crate) fn copy_request(member: Member, ammo: i32) -> CopyRequest {
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

/// Whether an arrival still belongs to a waiting member of the same source generation.
pub(crate) fn waiting(member: Member) -> bool {
    let state = STATE.lock();
    find(&state, member)
        .is_some_and(|s| matches!(s.members[member.index as usize], Stage::Waiting { .. }))
}

/// Keep a native-initialized actor at its source, outside the player's inventory and loadout.
///
/// # Safety
///
/// `actor` owns a live weapon on the main thread outside the world update.
pub(crate) unsafe fn staged(member: Member, actor: Strong) {
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
        let matrix = display.m_WorldMatrixT1;
        for index in 0..2 {
            if let Some(gun) = unsafe { (*weapon.m_lWeaponData.m_Begin.add(index)).as_mut() } {
                gun.m_WorldMatrixT0 = matrix;
                gun.m_WorldMatrixT1 = matrix;
            }
        }
    }
    source.members[member.index as usize].supply(actor);
    tracing::info!(target: "controllers", source = member.source, generation = member.generation,
        member = member.index, "rack: the initialized member waits at its source");
}

/// Take exactly the cached source member, never its neighbour or a replacement source.
///
/// # Safety
///
/// The local player and source are live on the main thread outside the world update.
pub(crate) unsafe fn take(
    owner: &mut Character,
    hand: Hand,
    member: Member,
    options: TakeOptions,
) -> Result<WeaponSlot, RackRefusal> {
    let actor = {
        let mut state = STATE.lock();
        let source = find_mut(&mut state, member).ok_or(RackRefusal::Gone)?;
        let stage = &mut source.members[member.index as usize];
        stage.take().ok_or(RackRefusal::CopyPending)?
    };
    let weapon = unsafe { &mut *engine::weapon_of(&actor) };
    // A staged actor is not in the single-pistol inventory watch until the take makes it owned.
    unsafe { single::designate(weapon, "a paired rack member") };
    let result = unsafe { pickup::route(weapon, Some(hand)) }
        .and_then(|route| match route {
            TakeRoute::Table { hand } => Ok(hand),
            TakeRoute::Game(_) => Err(Refusal::NotTaken),
        })
        .and_then(|hand| {
            if !unsafe { owner.m_Inventory.GetAnimationAllowsPickup() } {
                return Err(Refusal::AnimationForbids);
            }
            // The native give already initialized ownership and ammunition; do not request a copy.
            weapon.m_InfinitePickup = false;
            unsafe { weapon.SetState(WeaponState::E_WS_DROPPED, true) };
            unsafe {
                pickup::take_by_table(
                    owner,
                    hand,
                    weapon,
                    TakeSource::Rack,
                    TakeOptions {
                        cause: Cause::Rack,
                        ..options
                    },
                )
            }
        });
    let mut state = STATE.lock();
    if let Some(source) = find_mut(&mut state, member) {
        if result.is_ok() {
            // Hide only the member that left; the other native source gun remains visible.
            unsafe { hide_member(source, member.index as usize) };
        } else {
            unsafe { weapon.SetState(WeaponState::E_WS_WEAPON_RACK, true) };
            unsafe { weapon.SetVisible(false, false, false) };
        }
        source.members[member.index as usize].complete(actor, result.is_ok());
    }
    drop(state);
    result.map_err(RackRefusal::Pickup)?;
    Ok(WeaponSlot::E_SLOT_DUAL_WIELD)
}

/// Retire sources no longer offered, and make failed native requests non-repeating.
pub(crate) fn tick() {
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
    let sources = std::mem::take(&mut STATE.lock().sources);
    for source in sources {
        unsafe { retire(source) };
    }
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
        || !weapon.m_InfinitePickup
        || weapon.m_State != WeaponState::E_WS_WEAPON_RACK
        || !offered_state(weapon)
        || unsafe { engine::slot_of(weapon) } != Some(WeaponSlot::E_SLOT_DUAL_WIELD)
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
    use super::*;

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
