//! Restoring narrowed bodies one at a time, each as soon as its own surroundings are clear.
//!
//! A restored body is independently safe: its authored shape is checked against everything near
//! it in the shapes those neighbours will have after the pass. Bodies are decided in turn, so a
//! body approved earlier in the pass counts at its original and one not yet approved at the shape
//! it holds, and every pair of bodies is checked in the shapes it ends the pass with. Candidates
//! come from the engine's broad phase ([`hknpWorld::queryAabb`]), not from a walk of every body.

use std::{mem::MaybeUninit, ptr::NonNull};

use jc3gi::{
    animation::skeleton::{HK_ARRAY_CAPACITY_MASK, HK_ARRAY_DONT_DEALLOCATE_FLAG, HkArray},
    havok::{hkAabb, hkContainerHeapAllocator, hknpShape},
    physics::{hknpAabbQuery, hknpBody, hknpWorld},
};

use crate::controllers::physical_hands::human_hull::{
    ClearanceUnavailableReason,
    geometry::Bounds,
    native::{SlotKind, scan::Lookup, scan::Migrated, slot_kind},
    query::{self, Budget},
};

/// Which migrated bodies a pass restores, and why any other waits.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Plan {
    pub(super) approved: Vec<Migrated>,
    /// The first reason a body was refused this pass, if one was.
    pub(super) blocked: Option<ClearanceUnavailableReason>,
}

/// Decide each body of `migrated` in turn with `check`, which is given the bodies approved so far.
/// A refused body waits for a later pass without holding back the rest; an exhausted budget ends
/// the pass, since every later check would draw on it too.
pub(super) fn plan(
    migrated: &[Migrated],
    mut check: impl FnMut(&Migrated, &[Migrated]) -> Result<(), ClearanceUnavailableReason>,
) -> Plan {
    let mut plan = Plan::default();
    for body in migrated {
        match check(body, &plan.approved) {
            Ok(()) => plan.approved.push(*body),
            Err(reason) => {
                plan.blocked.get_or_insert(reason);
                if reason == ClearanceUnavailableReason::BudgetExhausted {
                    break;
                }
            }
        }
    }
    plan
}

/// The world a pass checks against: the live world, its adopted pairs, and the live bodies not in
/// the broad phase, which no broad-phase query returns.
pub(super) struct Surroundings<'a> {
    pub(super) world: &'a hknpWorld,
    pub(super) lookup: &'a Lookup,
    pub(super) pending: &'a [u32],
}

/// The work one pass may spend across all of its bodies.
pub(super) struct Allowance {
    budget: Budget,
    candidates: usize,
}

impl Allowance {
    pub(super) fn new() -> Self {
        Self {
            budget: Budget::new(),
            candidates: MAXIMUM_RESTORE_CANDIDATES,
        }
    }

    fn take_candidate(&mut self) -> Result<(), ClearanceUnavailableReason> {
        self.candidates = self
            .candidates
            .checked_sub(1)
            .ok_or(ClearanceUnavailableReason::BudgetExhausted)?;
        Ok(())
    }
}

/// Check that `body`'s original clears everything around it.
///
/// # Safety
///
/// The native pre-simulation phase, with `surroundings.world` the live world.
pub(super) unsafe fn check_body(
    surroundings: &Surroundings,
    body: &Migrated,
    approved: &[Migrated],
    allowance: &mut Allowance,
) -> Result<(), ClearanceUnavailableReason> {
    let world = surroundings.world;
    let native = unsafe { &*world.m_bodies.add(body.id as usize) };
    if native.m_broadPhaseId == hknpBody::INVALID_BROAD_PHASE_ID {
        return Err(ClearanceUnavailableReason::PendingBody);
    }
    let proposed = unsafe { query::world_bounds(body.original, &native.m_transform)? };
    for &id in surroundings.pending {
        if id == body.id {
            continue;
        }
        let other = unsafe { &*world.m_bodies.add(id as usize) };
        let shape =
            NonNull::new(other.m_shape).ok_or(ClearanceUnavailableReason::IncompleteInventory)?;
        let bounds = unsafe { query::world_bounds(shape, &other.m_transform)? };
        if query::intersects(proposed, bounds) {
            return Err(ClearanceUnavailableReason::PendingBody);
        }
    }
    let target = |other: &Migrated, shape: NonNull<hknpShape>, allowance: &mut Allowance| {
        let other_native = unsafe { &*world.m_bodies.add(other.id as usize) };
        let bounds = unsafe { query::world_bounds(shape, &other_native.m_transform)? };
        if query::intersects(proposed, bounds) {
            unsafe { check(body, native, shape, other_native, allowance)? };
        }
        Ok(())
    };
    // An approved body's broad-phase box is still its narrow one, so it is checked directly.
    for other in approved {
        allowance.take_candidate()?;
        target(other, other.original, allowance)?;
    }
    let hits = unsafe { BroadPhaseHits::query(world, proposed) };
    for &id in hits.ids() {
        if id == body.id || approved.iter().any(|other| other.id == id) {
            continue;
        }
        allowance.take_candidate()?;
        let other = unsafe { &*world.m_bodies.add(id as usize) };
        let has_shape = !other.m_shape.is_null();
        match slot_kind(other.m_flags, other.m_motionId, has_shape) {
            SlotKind::Free | SlotKind::Shapeless => continue,
            SlotKind::Live => {}
        }
        // SAFETY: a live slot of this kind has a shape.
        let shape = unsafe { NonNull::new_unchecked(other.m_shape) };
        if let Some(original) = surroundings.lookup.original_of(shape.as_ptr()) {
            // Not yet approved, so it keeps the replacement it holds through this pass.
            let migrated = Migrated {
                id,
                original,
                replacement: shape,
            };
            target(&migrated, shape, allowance)?;
            continue;
        }
        let mut aabb = hkAabb::default();
        unsafe { world.getBodyAabb(id, &mut aabb) };
        let bounds = Bounds {
            min: aabb.m_min.data,
            max: aabb.m_max.data,
        };
        if !query::valid_bounds(bounds) {
            return Err(ClearanceUnavailableReason::InvalidBounds);
        }
        if query::intersects(proposed, bounds) {
            unsafe { check(body, native, shape, other, allowance)? };
        }
    }
    Ok(())
}

/// Paces restoration while the hull is disabled and some body stays obstructed, which may last as
/// long as an NPC leans on a wall: each pass that restores nothing doubles the wait before the
/// next attempt, up to [`MAXIMUM_RETRY_WAIT_PASSES`]. A shutdown never waits on it, since its own
/// attempts are bounded.
#[derive(Debug, Default)]
pub(super) struct RetryBackoff {
    wait: u32,
    remaining: u32,
}

impl RetryBackoff {
    pub(super) const fn new() -> Self {
        Self {
            wait: 0,
            remaining: 0,
        }
    }

    /// Whether this pass may attempt a restoration.
    pub(super) fn ready(&mut self) -> bool {
        if self.remaining == 0 {
            true
        } else {
            self.remaining -= 1;
            false
        }
    }

    /// An attempt left bodies narrowed; `progressed` when it restored any.
    pub(super) fn record(&mut self, progressed: bool) {
        self.wait = if progressed {
            0
        } else {
            (self.wait * 2).clamp(1, MAXIMUM_RETRY_WAIT_PASSES)
        };
        self.remaining = self.wait;
    }

    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }
}

/// The candidates one pass may examine across all of its bodies.
const MAXIMUM_RESTORE_CANDIDATES: usize = 65_536;

/// The longest wait between attempts while disabled, in pre-simulation passes.
const MAXIMUM_RETRY_WAIT_PASSES: u32 = 64;

unsafe fn check(
    body: &Migrated,
    native: &hknpBody,
    target: NonNull<hknpShape>,
    other: &hknpBody,
    allowance: &mut Allowance,
) -> Result<(), ClearanceUnavailableReason> {
    unsafe {
        query::check_target(
            body.original,
            body.replacement,
            &native.m_transform,
            target,
            &other.m_transform,
            &mut allowance.budget,
        )
    }
}

/// The ids [`hknpWorld::queryAabb`] returned, in storage the query grew from the container
/// allocator and that is freed there on drop.
struct BroadPhaseHits {
    array: HkArray<u32>,
}

impl BroadPhaseHits {
    unsafe fn query(world: &hknpWorld, bounds: Bounds) -> Self {
        // SAFETY: the engine's callers zero the query too; every field is plain data.
        let mut query = unsafe { MaybeUninit::<hknpAabbQuery>::zeroed().assume_init() };
        query.m_materialId = u16::MAX;
        query.m_aabb.m_min.data = bounds.min;
        query.m_aabb.m_max.data = bounds.max;
        // An empty array that owns nothing, as the engine's callers pass it.
        let mut hits = Self {
            array: HkArray {
                data: std::ptr::null_mut(),
                size: 0,
                capacity_and_flags: HK_ARRAY_DONT_DEALLOCATE_FLAG as i32,
            },
        };
        unsafe { world.queryAabb(&query, &mut hits.array) };
        hits
    }

    fn ids(&self) -> &[u32] {
        match usize::try_from(self.array.size) {
            Ok(len) if len > 0 && !self.array.data.is_null() => {
                // SAFETY: the query wrote `size` ids to its storage.
                unsafe { std::slice::from_raw_parts(self.array.data, len) }
            }
            _ => &[],
        }
    }
}

impl Drop for BroadPhaseHits {
    fn drop(&mut self) {
        let flags = self.array.capacity_and_flags as u32;
        if flags & HK_ARRAY_DONT_DEALLOCATE_FLAG != 0 || self.array.data.is_null() {
            return;
        }
        let bytes = (flags & HK_ARRAY_CAPACITY_MASK) as usize * size_of::<u32>();
        // SAFETY: storage the query grew is owned by the array and came from this allocator.
        unsafe {
            hkContainerHeapAllocator::get_s_alloc().bufFree(self.array.data.cast(), bytes as i32)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(id: u32) -> Migrated {
        let shape = NonNull::new((0x1000 + id as usize * 0x10) as *mut hknpShape).unwrap();
        Migrated {
            id,
            original: shape,
            replacement: shape,
        }
    }

    #[test]
    fn one_obstructed_body_does_not_hold_back_the_others() {
        let migrated = [body(1), body(2), body(3)];
        let plan = plan(&migrated, |body, _| {
            if body.id == 2 {
                Err(ClearanceUnavailableReason::Obstructed)
            } else {
                Ok(())
            }
        });
        assert_eq!(plan.approved, [body(1), body(3)]);
        assert_eq!(plan.blocked, Some(ClearanceUnavailableReason::Obstructed));
    }

    #[test]
    fn each_body_is_checked_against_the_bodies_approved_before_it() {
        let migrated = [body(1), body(2), body(3)];
        let mut seen = Vec::new();
        let plan = plan(&migrated, |body, approved| {
            seen.push((body.id, approved.iter().map(|b| b.id).collect::<Vec<_>>()));
            // The second body's original would overlap the first's.
            if body.id == 2 && approved.iter().any(|b| b.id == 1) {
                Err(ClearanceUnavailableReason::Obstructed)
            } else {
                Ok(())
            }
        });
        assert_eq!(seen, [(1, vec![]), (2, vec![1]), (3, vec![1])]);
        assert_eq!(plan.approved, [body(1), body(3)]);
    }

    #[test]
    fn an_exhausted_budget_ends_the_pass() {
        let migrated = [body(1), body(2), body(3)];
        let mut checked = 0;
        let plan = plan(&migrated, |body, _| {
            checked += 1;
            if body.id == 2 {
                Err(ClearanceUnavailableReason::BudgetExhausted)
            } else {
                Ok(())
            }
        });
        assert_eq!(checked, 2);
        assert_eq!(plan.approved, [body(1)]);
    }

    #[test]
    fn a_disabled_hull_backs_off_while_nothing_restores() {
        let mut backoff = RetryBackoff::new();
        assert!(backoff.ready());
        let mut gaps = Vec::new();
        for _ in 0..9 {
            backoff.record(false);
            let mut skipped = 0;
            while !backoff.ready() {
                skipped += 1;
            }
            gaps.push(skipped);
        }
        assert_eq!(gaps, [1, 2, 4, 8, 16, 32, 64, 64, 64]);
        // Progress retries on the next pass.
        backoff.record(true);
        assert!(backoff.ready());
        backoff.record(false);
        assert!(!backoff.ready());
        backoff.reset();
        assert!(backoff.ready());
    }
}
