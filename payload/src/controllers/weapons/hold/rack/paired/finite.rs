//! A finite native pair supplies one initialized copy and the original actor, never two copies.

use std::time::Instant;

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponState},
    game::GameObject,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    loadout::{TakeSource, take::TakeRoute},
    weapons::{
        dual_wield::single::{self, magazine},
        hold::{
            Cause,
            pickup::{self, Refusal, TakeOptions},
            rack::{
                RackRefusal,
                paired::{
                    self, COPY_WAIT, MAX_SOURCES, Member, SOURCE_GRACE, Stage, gun, gun_target,
                },
            },
            refs::{Strong, Weak},
            world::{TargetKind, WorldTarget},
        },
        inventory::{engine, gives},
    },
};

/// Offer each real native gun while keeping a finite source's actor at its placement rule.
///
/// # Safety
///
/// The source and owner are live on the main thread outside the world update.
pub(super) unsafe fn targets(
    owner: &Character,
    weapon: &WeaponBase,
    alive: Strong,
    to_cockpit: &impl Fn(glam::Vec3) -> glam::Vec3,
    out: &mut Vec<WorldTarget>,
) -> bool {
    let address = weapon as *const WeaponBase as usize;
    let busy = gives::member_pending();
    let mut state = STATE.lock();
    let at = match state.iter().position(|s| s.address == address) {
        Some(at) => at,
        None => {
            if !unsafe { paired::qualifies(weapon) } || single::is(address) {
                return false;
            }
            if state.len() >= MAX_SOURCES {
                return true;
            }
            let Some(generation) = paired::next_generation() else {
                return true;
            };
            state.push(Source {
                address,
                generation,
                alive: Some(alive),
                weak: unsafe { engine::weak_to(weapon) },
                seen: Instant::now(),
                copy: Stage::Empty,
                claims: Claims::default(),
                pickup_started: false,
            });
            state.len() - 1
        }
    };
    let source = &mut state[at];
    source.seen = Instant::now();
    if source.alive.is_none() {
        source.alive = source.weak.lock();
        if source.alive.is_none() {
            return true;
        }
    }
    if weapon.m_InfinitePickup || weapon.m_State != WeaponState::E_WS_WEAPON_RACK {
        return !single::is(address);
    }
    if let Some(taken) = source.claims.first {
        unsafe { show_remaining(source, taken) };
    }
    let ready = source.claims.first.is_some() || matches!(source.copy, Stage::Ready(_));
    if ready {
        for index in 0..2 {
            if !source.claims.available(index) {
                continue;
            }
            let Some((points, pose)) = (unsafe { gun(weapon, index) }).and_then(gun_target) else {
                continue;
            };
            out.push(WorldTarget {
                kind: TargetKind::RackMember {
                    weapon: address,
                    generation: source.generation,
                    member: index as u8,
                },
                weapon: address,
                slot: WeaponSlot::E_SLOT_DUAL_WIELD,
                points: points.map(to_cockpit),
                gun: pose,
            });
        }
    }
    let request = if !busy
        && matches!(source.copy, Stage::Empty)
        && !owner.m_Inventory.m_WeaponPendingSpawn[WeaponSlot::E_SLOT_DUAL_WIELD as usize]
    {
        let member = source.callback();
        source.copy = Stage::Waiting { at: Instant::now() };
        Some(member)
    } else {
        None
    };
    drop(state);
    if let Some(member) = request {
        let object = unsafe {
            &*((address + WeaponBase::GAME_OBJECT_BASE_OFFSET as usize) as *const GameObject)
        };
        let tag = unsafe { object.GetUniqueSpawnTagIndex() };
        unsafe { gives::request_member_copy(owner, member, tag) };
        tracing::info!(target: "controllers", source = address, generation = member.generation,
            "rack: requested the finite pair's only initialized copy");
    }
    true
}

pub(super) fn retains_single(object: usize) -> bool {
    STATE.lock().iter().any(|s| {
        s.claims.first.is_some()
            && !s.claims.finished
            && s.address == object
            && s.weak.lock().is_some()
    })
}

pub(super) fn owns(member: Member) -> bool {
    STATE.lock().iter().any(|s| s.matches(member))
}

pub(super) fn waiting(member: Member) -> bool {
    STATE
        .lock()
        .iter()
        .any(|s| s.matches(member) && member.index == 0 && matches!(s.copy, Stage::Waiting { .. }))
}

/// Store the empty initialized actor without crediting its spawned default ammunition.
///
/// # Safety
///
/// The actor is live on the main thread outside the world update.
pub(super) unsafe fn staged(member: Member, actor: Strong) {
    let weapon = unsafe { &mut *engine::weapon_of(&actor) };
    unsafe { single::designate(weapon, "a finite rack member") };
    if !single::is(weapon as *mut WeaponBase as usize)
        || !paired::two_guns(weapon)
        || unsafe { gun(weapon, 0) }.is_none()
        || unsafe { gun(weapon, 1) }.is_none()
    {
        single::undesignate(weapon as *mut WeaponBase as usize);
        unsafe { weapon.Despawn() };
        return;
    }
    unsafe { weapon.SetMagazineAmmo(0) };
    weapon.m_StoredNonMagazineAmmo = 0;
    unsafe { weapon.SetState(WeaponState::E_WS_WEAPON_RACK, true) };
    unsafe { weapon.SetVisible(false, false, false) };
    let mut state = STATE.lock();
    if let Some(source) = state.iter_mut().find(|s| s.matches(member))
        && member.index == 0
        && matches!(source.copy, Stage::Waiting { .. })
    {
        source.copy.supply(actor);
        return;
    }
    drop(state);
    single::undesignate(weapon as *mut WeaponBase as usize);
    unsafe { weapon.Despawn() };
}

/// Claim the staged copy first, then transfer the remaining original through finite pickup.
///
/// # Safety
///
/// The source and local player are live on the main thread outside the world update.
pub(super) unsafe fn take(
    owner: &mut Character,
    hand: Hand,
    member: Member,
    options: TakeOptions,
) -> Result<WeaponSlot, RackRefusal> {
    let (actor, first, pickup_started) = {
        let mut state = STATE.lock();
        let source = state
            .iter_mut()
            .find(|s| s.matches(member))
            .ok_or(RackRefusal::Gone)?;
        if !source.claims.available(member.index as usize)
            || (source.claims.first.is_some() && !single::is(source.address))
        {
            return Err(RackRefusal::Gone);
        }
        let first = source.claims.first.is_none();
        let actor = if first {
            source.copy.take().ok_or(RackRefusal::CopyPending)?
        } else {
            unsafe { engine::strong_to(&*(source.address as *const WeaponBase)) }
                .ok_or(RackRefusal::Gone)?
        };
        (actor, first, source.pickup_started)
    };
    let weapon = unsafe { &mut *engine::weapon_of(&actor) };
    if !single::is(weapon as *mut WeaponBase as usize) {
        unsafe { single::designate(weapon, "a finite rack member") };
    }
    let route = if single::is(weapon as *mut WeaponBase as usize) {
        unsafe { pickup::route(weapon, Some(hand)) }
    } else {
        Err(Refusal::NotTaken)
    };
    let result = match route {
        Ok(TakeRoute::Table { hand }) => {
            if first {
                unsafe {
                    take_first(
                        owner,
                        hand,
                        member,
                        weapon,
                        &mut *(member.source as *mut WeaponBase),
                        options,
                    )
                }
            } else {
                if pickup_started {
                    // A failed carry may already have sent the native source's pickup event.
                    unsafe { weapon.SetState(WeaponState::E_WS_DROPPED, true) };
                }
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
            }
        }
        Ok(TakeRoute::Game(_)) => Err(Refusal::NotTaken),
        Err(refusal) => Err(refusal),
    };
    let started = !first && weapon.m_State != WeaponState::E_WS_WEAPON_RACK;
    let mut state = STATE.lock();
    if let Some(source) = state.iter_mut().find(|s| s.matches(member)) {
        source.pickup_started |= started;
        if result.is_ok() {
            source.claims.accept(member.index as usize);
            if first {
                source.copy.complete(actor, true);
                unsafe { show_remaining(source, member.index as usize) };
            }
        } else if first {
            unsafe { weapon.SetState(WeaponState::E_WS_WEAPON_RACK, true) };
            unsafe { weapon.SetVisible(false, false, false) };
            source.copy.complete(actor, false);
        } else if started {
            // The source event cannot be rolled back; a failed final carry leaves one loose gun.
            unsafe { weapon.SetState(WeaponState::E_WS_DROPPED, true) };
            unsafe { weapon.SetVisible(true, false, false) };
            unsafe { single::hide_extra(weapon) };
        }
    }
    result.map_err(RackRefusal::Pickup)?;
    tracing::info!(target: "controllers", source = member.source, generation = member.generation,
        member = member.index, original = !first, "rack: took a finite native member");
    Ok(WeaponSlot::E_SLOT_DUAL_WIELD)
}

pub(super) fn tick() {
    let mut state = STATE.lock();
    let mut retired = Vec::new();
    let mut at = 0;
    while at < state.len() {
        let source = &mut state[at];
        if source.claims.finished || source.weak.lock().is_none() {
            retired.push(state.swap_remove(at));
            continue;
        }
        if source.seen.elapsed() >= SOURCE_GRACE {
            if source.claims.first.is_some() {
                source.alive = None;
            } else {
                retired.push(state.swap_remove(at));
                continue;
            }
        }
        if matches!(source.copy, Stage::Waiting { at } if at.elapsed() >= COPY_WAIT) {
            source.copy = Stage::Failed;
            tracing::warn!(target: "controllers", "rack: the finite pair's copy did not arrive");
        }
        at += 1;
    }
    drop(state);
    for source in retired {
        unsafe { retire(source) };
    }
}

pub(super) fn clear() {
    let sources = std::mem::take(&mut *STATE.lock());
    for source in sources {
        unsafe { retire(source) };
    }
}

struct Source {
    address: usize,
    generation: u64,
    alive: Option<Strong>,
    weak: Weak,
    seen: Instant,
    copy: Stage,
    claims: Claims,
    pickup_started: bool,
}

impl Source {
    fn matches(&self, member: Member) -> bool {
        member.index < 2 && self.address == member.source && self.generation == member.generation
    }

    fn callback(&self) -> Member {
        Member {
            source: self.address,
            generation: self.generation,
            index: 0,
        }
    }
}

#[derive(Default)]
struct Claims {
    first: Option<usize>,
    finished: bool,
}

impl Claims {
    fn available(&self, index: usize) -> bool {
        index < 2 && !self.finished && self.first != Some(index)
    }

    fn accept(&mut self, index: usize) {
        if self.available(index) {
            if self.first.is_none() {
                self.first = Some(index);
            } else {
                self.finished = true;
            }
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Allocation {
    magazines: [i32; 2],
    stored: i32,
}

impl Allocation {
    fn new(magazine: i32, stored: i32, tuned: i32) -> Option<Self> {
        if magazine < 0 || stored < 0 {
            return None;
        }
        let dealt = magazine::split(magazine, tuned);
        Some(Self {
            magazines: [dealt.slot, dealt.twin],
            stored: stored.checked_add(dealt.over)?,
        })
    }
}

static STATE: Mutex<Vec<Source>> = Mutex::new(Vec::new());

unsafe fn take_first(
    owner: &mut Character,
    hand: Hand,
    member: Member,
    copy: &mut WeaponBase,
    original: &mut WeaponBase,
    options: TakeOptions,
) -> Result<(), Refusal> {
    let allocation = Allocation::new(
        original.m_MagazineAmmo,
        original.m_StoredNonMagazineAmmo,
        unsafe { original.GetMagazineSize() },
    )
    .ok_or(Refusal::NotTaken)?;
    let magazine = original.m_MagazineAmmo;
    let stored = original.m_StoredNonMagazineAmmo;
    let index = member.index as usize;
    let matrix = unsafe { gun(original, index) }
        .ok_or(Refusal::NotTaken)?
        .m_WorldMatrixT1;
    if !unsafe { single::designate(original, "the remaining finite rack member") } {
        return Err(Refusal::NotTaken);
    }
    if let Some(data) = unsafe { (*copy.m_lWeaponData.m_Begin).as_mut() } {
        data.m_WorldMatrixT0 = matrix;
        data.m_WorldMatrixT1 = matrix;
    }
    unsafe { original.SetMagazineAmmo(allocation.magazines[1 - index]) };
    original.m_StoredNonMagazineAmmo = allocation.stored;
    unsafe { copy.SetMagazineAmmo(allocation.magazines[index]) };
    copy.m_StoredNonMagazineAmmo = 0;
    copy.m_InfinitePickup = false;
    unsafe { copy.SetState(WeaponState::E_WS_DROPPED, true) };
    let result = unsafe {
        pickup::take_by_table(
            owner,
            hand,
            copy,
            TakeSource::Rack,
            TakeOptions {
                cause: Cause::Rack,
                ..options
            },
        )
    };
    if result.is_err() {
        unsafe { original.SetMagazineAmmo(magazine) };
        original.m_StoredNonMagazineAmmo = stored;
        single::undesignate(member.source);
        unsafe { original.SetVisible(true, false, false) };
        unsafe { copy.SetMagazineAmmo(0) };
        copy.m_StoredNonMagazineAmmo = 0;
    }
    result
}

unsafe fn show_remaining(source: &Source, taken: usize) {
    let weapon = unsafe { &mut *(source.address as *mut WeaponBase) };
    if taken == 0 {
        let Some(matrix) = (unsafe { gun(weapon, 1) }).map(|g| g.m_WorldMatrixT1) else {
            return;
        };
        if let Some(data) = unsafe { (*weapon.m_lWeaponData.m_Begin).as_mut() } {
            data.m_WorldMatrixT0 = matrix;
            data.m_WorldMatrixT1 = matrix;
        }
    }
    unsafe { weapon.SetVisible(true, false, false) };
    unsafe { single::hide_extra(weapon) };
}

unsafe fn retire(source: Source) {
    let alive = source.weak.lock();
    if let Some(alive) = alive.as_ref() {
        let weapon = unsafe { &mut *engine::weapon_of(alive) };
        if weapon.m_State == WeaponState::E_WS_WEAPON_RACK && !source.claims.finished {
            if let Some(taken) = source.claims.first {
                unsafe { show_remaining(&source, taken) };
            } else {
                unsafe { weapon.SetVisible(true, false, false) };
            }
        }
    } else {
        single::undesignate(source.address);
    }
    if let Stage::Ready(actor) = source.copy {
        single::undesignate(engine::weapon_of(&actor) as usize);
        unsafe { (*engine::weapon_of(&actor)).Despawn() };
    }
    drop(source.alive);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_ammunition_is_conserved_including_native_extra_rounds() {
        for tuned in [0, 1, 15, 24, 60] {
            for magazine in 0..100 {
                for stored in [0, 3, 91] {
                    let allocation = Allocation::new(magazine, stored, tuned).unwrap();
                    assert_eq!(
                        allocation.magazines.iter().sum::<i32>() + allocation.stored,
                        magazine + stored
                    );
                    assert!(
                        allocation
                            .magazines
                            .iter()
                            .all(|&n| n <= magazine::capacity(tuned))
                    );
                }
            }
        }
    }

    #[test]
    fn either_order_consumes_exactly_two_members() {
        for first in [0, 1] {
            let mut claims = Claims::default();
            claims.accept(first);
            assert!(!claims.available(first));
            assert!(claims.available(1 - first));
            claims.accept(first);
            assert!(!claims.finished);
            claims.accept(1 - first);
            assert!(claims.finished);
            assert!(!claims.available(0));
            assert!(!claims.available(1));
        }
    }

    #[test]
    fn refused_first_take_keeps_one_copy_and_both_members() {
        let claims = Claims::default();
        let mut copy = Stage::Waiting { at: Instant::now() };
        assert!(copy.supply(101));
        let actor = copy.take().unwrap();
        assert!(copy.take().is_none());
        copy.complete(actor, false);
        assert!(matches!(copy, Stage::Ready(101)));
        assert!(claims.available(0) && claims.available(1));
        assert!(Allocation::new(-1, 3, 24).is_none());
        assert!(Allocation::new(30, i32::MAX, 24).is_none());
    }

    #[test]
    fn partial_sources_cannot_reclaim_taken_or_invalid_members() {
        let mut claims = Claims::default();
        claims.accept(1);
        assert!(!claims.available(1));
        assert!(claims.available(0));
        assert!(!claims.available(2));
        assert!(!claims.available(usize::MAX));
        let allocation = Allocation::new(13, 27, 24).unwrap();
        assert_eq!(allocation.magazines, [12, 1]);
        assert_eq!(allocation.stored, 27);
    }
}
