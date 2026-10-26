//! Retained native shape tables and bounded world-body transactions.

use std::{
    ptr::NonNull,
    sync::atomic::{AtomicU32, Ordering},
};

use glam::Vec3;
use jc3gi::{
    havok::{hkAabb, hkReferencedObject, hknpShape},
    physics::{ProxyState, SharedPfxCharacterSettings, hknpBody, hknpWorld},
};

use crate::controllers::physical_hands::human_hull::{
    ClearanceUnavailableReason,
    geometry::{self, Bounds, GeometryError},
};

pub(super) struct NativeState {
    pub(super) tables: Vec<Table>,
    pub(super) enabled: bool,
    pub(super) committed: bool,
    pub(super) shutdown_requested: bool,
    pub(super) migrated_bodies: usize,
    pub(super) original_active: bool,
    pub(super) clearance: ClearanceUnavailableReason,
    generation: u64,
}

// SAFETY: STATE serializes access. Shape mutations run in settings hooks or the native phase;
// shutdown preparation only copies bookkeeping, and commit releases retained references.
unsafe impl Send for NativeState {}

impl NativeState {
    pub(super) const fn new() -> Self {
        Self {
            tables: Vec::new(),
            enabled: true,
            committed: false,
            shutdown_requested: false,
            migrated_bodies: 0,
            original_active: false,
            clearance: ClearanceUnavailableReason::NotRequested,
            generation: 0,
        }
    }

    pub(super) fn request_shutdown(&mut self) {
        if !self.shutdown_requested {
            self.shutdown_requested = true;
            self.advance_generation();
            self.clearance = ClearanceUnavailableReason::RestorationRequested;
        }
    }

    pub(super) fn shutdown_generation(&self) -> Option<u64> {
        (self.shutdown_requested
            && !self.committed
            && self.original_active
            && self.migrated_bodies == 0
            && self.tables.iter().all(|table| !table.installed))
        .then_some(self.generation)
    }

    /// The native phase has restored every tracked body, and pending hooks cannot install shapes.
    pub(super) unsafe fn commit_shutdown(&mut self) {
        assert!(
            self.shutdown_generation().is_some(),
            "human hull: commit before native restoration"
        );
        self.committed = true;
        for table in &mut self.tables {
            unsafe { table.release_pairs() };
        }
        self.tables.clear();
    }

    pub(super) unsafe fn adopt(
        &mut self,
        settings: NonNull<SharedPfxCharacterSettings>,
    ) -> Result<(), GeometryError> {
        if self.committed
            || self
                .tables
                .iter()
                .any(|table| table.settings == Some(settings))
        {
            return Ok(());
        }
        let mut table = Table {
            settings: Some(settings),
            pairs: Vec::new(),
            installed: false,
        };
        if self.shutdown_requested {
            self.tables.push(table);
            self.advance_generation();
            return Ok(());
        }
        for state in HUMAN_STATES {
            let index = state as usize;
            let Some(original) = NonNull::new(unsafe { settings.as_ref() }.m_CapsuleShape[index])
            else {
                unsafe { table.release_pairs() };
                return Err(GeometryError::InvalidGeometry);
            };
            let (replacement, original_bounds) = match unsafe { geometry::narrow(original) } {
                Ok(result) => result,
                Err(error) => {
                    unsafe { table.release_pairs() };
                    return Err(error);
                }
            };
            unsafe { retain(original) };
            table.pairs.push(Pair {
                index,
                original,
                replacement,
                original_bounds,
            });
        }
        if self.enabled {
            unsafe { table.set_installed(true) };
            self.original_active = false;
        }
        self.tables.push(table);
        self.advance_generation();
        Ok(())
    }

    pub(super) fn forget(&mut self, settings: NonNull<SharedPfxCharacterSettings>) {
        if let Some(table) = self
            .tables
            .iter_mut()
            .find(|table| table.settings == Some(settings))
        {
            table.settings = None;
            table.installed = false;
            self.advance_generation();
        }
    }

    pub(super) unsafe fn synchronize(
        &mut self,
        world: &mut hknpWorld,
    ) -> Result<(), ClearanceUnavailableReason> {
        self.advance_generation();
        self.original_active = false;
        let bodies = unsafe { inventory(world, &self.tables)? };
        let restore = self.shutdown_requested || !self.enabled;
        self.migrated_bodies = unsafe { self.count_migrated(world, &bodies) };
        if restore {
            unsafe { self.check_restore(world, &bodies)? };
        }
        let mut changes = Vec::new();
        for &id in &bodies {
            let current = unsafe { (*world.m_bodies.add(id as usize)).m_shape };
            for pair in self.tables.iter().flat_map(|table| &table.pairs) {
                let (from, to) = if restore {
                    (pair.replacement, pair.original)
                } else {
                    (pair.original, pair.replacement)
                };
                if current == from.as_ptr() {
                    changes.push(Change { id, from, to });
                    break;
                }
            }
        }
        unsafe { apply(world, &changes)? };
        let bodies = unsafe { inventory(world, &self.tables)? };
        self.migrated_bodies = unsafe { self.count_migrated(world, &bodies) };
        if restore && self.migrated_bodies != 0 {
            return Err(ClearanceUnavailableReason::MutationRejected);
        }
        for table in &mut self.tables {
            unsafe { table.set_installed(!restore) };
        }
        self.original_active = restore;
        self.clearance = if restore {
            ClearanceUnavailableReason::Restored
        } else {
            ClearanceUnavailableReason::NotRequested
        };
        Ok(())
    }

    pub(super) unsafe fn restore_for_shutdown(
        &mut self,
        world: &mut hknpWorld,
    ) -> Result<(), ClearanceUnavailableReason> {
        self.request_shutdown();
        unsafe { self.synchronize(world) }
    }

    fn advance_generation(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("human hull: generation exhausted");
    }

    unsafe fn count_migrated(&self, world: &hknpWorld, bodies: &[u32]) -> usize {
        bodies
            .iter()
            .filter(|&&id| {
                let current = unsafe { (*world.m_bodies.add(id as usize)).m_shape };
                self.tables
                    .iter()
                    .flat_map(|table| &table.pairs)
                    .any(|pair| current == pair.replacement.as_ptr())
            })
            .count()
    }

    unsafe fn check_restore(
        &self,
        world: &hknpWorld,
        bodies: &[u32],
    ) -> Result<(), ClearanceUnavailableReason> {
        let mut comparisons = 0usize;
        for &id in bodies {
            let body = unsafe { &*world.m_bodies.add(id as usize) };
            let Some(pair) = self
                .tables
                .iter()
                .flat_map(|table| &table.pairs)
                .find(|pair| body.m_shape == pair.replacement.as_ptr())
            else {
                continue;
            };
            if body.m_broadPhaseId == hknpBody::INVALID_BROAD_PHASE_ID {
                return Err(ClearanceUnavailableReason::PendingBody);
            }
            let proposed = transformed_bounds(body, pair.original_bounds)?;
            for &other_id in bodies {
                if id == other_id {
                    continue;
                }
                comparisons += 1;
                if comparisons > MAXIMUM_RESTORE_COMPARISONS {
                    return Err(ClearanceUnavailableReason::BudgetExhausted);
                }
                let other = unsafe { &*world.m_bodies.add(other_id as usize) };
                if other.m_broadPhaseId == hknpBody::INVALID_BROAD_PHASE_ID {
                    return Err(ClearanceUnavailableReason::PendingBody);
                }
                let mut aabb = hkAabb::default();
                unsafe { world.getBodyAabb(other_id, &mut aabb) };
                let target = Bounds {
                    min: aabb.m_min.data,
                    max: aabb.m_max.data,
                };
                if !valid_bounds(target) {
                    return Err(ClearanceUnavailableReason::InvalidBounds);
                }
                if intersects(proposed, target) {
                    return Err(ClearanceUnavailableReason::Obstructed);
                }
            }
        }
        Ok(())
    }
}

pub(super) struct Table {
    settings: Option<NonNull<SharedPfxCharacterSettings>>,
    pairs: Vec<Pair>,
    installed: bool,
}

impl Table {
    unsafe fn set_installed(&mut self, install: bool) {
        if self.installed == install {
            return;
        }
        let Some(mut settings) = self.settings else {
            return;
        };
        for pair in &self.pairs {
            let next = if install {
                pair.replacement
            } else {
                pair.original
            };
            unsafe { retain(next) };
            let old = unsafe { settings.as_mut() }.m_CapsuleShape[pair.index];
            unsafe { settings.as_mut() }.m_CapsuleShape[pair.index] = next.as_ptr();
            if let Some(old) = NonNull::new(old) {
                unsafe { release(old) };
            }
        }
        self.installed = install;
    }

    unsafe fn release_pairs(&mut self) {
        for pair in self.pairs.drain(..) {
            unsafe {
                release(pair.original);
                release(pair.replacement);
            }
        }
    }
}

struct Pair {
    index: usize,
    original: NonNull<hknpShape>,
    replacement: NonNull<hknpShape>,
    original_bounds: Bounds,
}

struct Change {
    id: u32,
    from: NonNull<hknpShape>,
    to: NonNull<hknpShape>,
}

unsafe fn inventory(
    world: &hknpWorld,
    tables: &[Table],
) -> Result<Vec<u32>, ClearanceUnavailableReason> {
    if world.m_bodies.is_null() || world.m_peakBodyIndex > MAXIMUM_BODY_INDEX {
        return Err(ClearanceUnavailableReason::IncompleteInventory);
    }
    let mut bodies = Vec::new();
    for id in 0..=world.m_peakBodyIndex {
        let body = unsafe { &*world.m_bodies.add(id as usize) };
        if body.m_motionId == hknpBody::INVALID_MOTION_ID {
            if tables
                .iter()
                .flat_map(|table| &table.pairs)
                .any(|pair| body.m_shape == pair.replacement.as_ptr())
            {
                return Err(ClearanceUnavailableReason::PendingBody);
            }
            continue;
        }
        if body.m_shape.is_null() {
            return Err(ClearanceUnavailableReason::IncompleteInventory);
        }
        bodies.push(id);
    }
    Ok(bodies)
}

unsafe fn apply(
    world: &mut hknpWorld,
    changes: &[Change],
) -> Result<(), ClearanceUnavailableReason> {
    for (index, change) in changes.iter().enumerate() {
        unsafe { world.setBodyShape(change.id, change.to.as_ptr().cast()) };
        if unsafe { (*world.m_bodies.add(change.id as usize)).m_shape } != change.to.as_ptr() {
            for previous in changes[..=index].iter().rev() {
                unsafe { world.setBodyShape(previous.id, previous.from.as_ptr().cast()) };
            }
            return Err(ClearanceUnavailableReason::MutationRejected);
        }
    }
    Ok(())
}

fn transformed_bounds(
    body: &hknpBody,
    bounds: Bounds,
) -> Result<Bounds, ClearanceUnavailableReason> {
    let translation = Vec3::from_array(body.m_transform.m_translation.data);
    let mut radius = 0.0f32;
    for bits in 0..8 {
        let corner = Vec3::from_array(std::array::from_fn(|axis| {
            if bits & (1 << axis) == 0 {
                bounds.min[axis]
            } else {
                bounds.max[axis]
            }
        }));
        radius = radius.max(corner.length());
    }
    if !translation.is_finite() || !radius.is_finite() {
        return Err(ClearanceUnavailableReason::InvalidBounds);
    }
    let extent = Vec3::splat(radius + RESTORE_MARGIN_M);
    let result = Bounds {
        min: (translation - extent).to_array(),
        max: (translation + extent).to_array(),
    };
    if valid_bounds(result) {
        Ok(result)
    } else {
        Err(ClearanceUnavailableReason::InvalidBounds)
    }
}

fn valid_bounds(bounds: Bounds) -> bool {
    (0..3).all(|axis| {
        bounds.min[axis].is_finite()
            && bounds.max[axis].is_finite()
            && bounds.min[axis] <= bounds.max[axis]
    })
}

fn intersects(a: Bounds, b: Bounds) -> bool {
    (0..3).all(|axis| a.min[axis] <= b.max[axis] && b.min[axis] <= a.max[axis])
}

pub(super) unsafe fn retain(shape: NonNull<hknpShape>) {
    let object = shape.as_ptr().cast::<hkReferencedObject>();
    let count = unsafe { &*(&raw const (*object).m_memSizeAndRefCount).cast::<AtomicU32>() };
    let _ = count.try_update(Ordering::AcqRel, Ordering::Acquire, |word| {
        if word >> hkReferencedObject::MEMORY_SIZE_SHIFT == 0 {
            return None;
        }
        let references = word & hkReferencedObject::REFERENCE_COUNT_MASK;
        assert!(
            references > 0 && references < hkReferencedObject::REFERENCE_COUNT_MASK,
            "human hull: invalid native reference count"
        );
        Some((word & !hkReferencedObject::REFERENCE_COUNT_MASK) | (references + 1))
    });
}

pub(super) unsafe fn release(shape: NonNull<hknpShape>) {
    let object = shape.as_ptr().cast::<hkReferencedObject>();
    let count = unsafe { &*(&raw const (*object).m_memSizeAndRefCount).cast::<AtomicU32>() };
    let previous = count.try_update(Ordering::AcqRel, Ordering::Acquire, |word| {
        if word >> hkReferencedObject::MEMORY_SIZE_SHIFT == 0 {
            return None;
        }
        let references = word & hkReferencedObject::REFERENCE_COUNT_MASK;
        assert!(references > 0, "human hull: invalid native release");
        Some((word & !hkReferencedObject::REFERENCE_COUNT_MASK) | (references - 1))
    });
    if previous.is_ok_and(|word| word & hkReferencedObject::REFERENCE_COUNT_MASK == 1) {
        unsafe { (*object).deleteThisReferencedObject() };
    }
}

const HUMAN_STATES: [ProxyState; 7] = [
    ProxyState::STANDING,
    ProxyState::CROUCHING,
    ProxyState::SWIMMING,
    ProxyState::ROTATED_AND_OFFSETTED,
    ProxyState::ROTATING_CROUCHING,
    ProxyState::PLAYER_STANDING,
    ProxyState::WINGSUITING,
];
const MAXIMUM_BODY_INDEX: u32 = 262_144;
const MAXIMUM_RESTORE_COMPARISONS: usize = 1_000_000;
const RESTORE_MARGIN_M: f32 = 1.0e-3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_request_preserves_the_enabled_setting_and_requires_native_restoration() {
        let mut state = NativeState::new();
        state.request_shutdown();
        let generation = state.generation;
        state.request_shutdown();
        assert!(state.enabled);
        assert_eq!(state.generation, generation);
        assert_eq!(state.shutdown_generation(), None);
        state.original_active = true;
        assert_eq!(state.shutdown_generation(), Some(generation));
        state.migrated_bodies = 1;
        assert_eq!(state.shutdown_generation(), None);
    }

    #[test]
    fn pending_construction_registers_without_touching_native_settings() {
        let mut state = NativeState::new();
        state.request_shutdown();
        state.original_active = true;
        let generation = state.shutdown_generation().unwrap();
        let settings = NonNull::dangling();
        // SAFETY: pending adoption only records this identity and never dereferences it.
        unsafe { state.adopt(settings) }.unwrap();
        assert_eq!(state.tables.len(), 1);
        assert!(state.tables[0].pairs.is_empty());
        assert!(!state.tables[0].installed);
        assert_ne!(state.shutdown_generation(), Some(generation));
        state.forget(settings);
        assert!(state.shutdown_generation().is_some());
        // SAFETY: the state contains no native references and confirms authored geometry.
        unsafe { state.commit_shutdown() };
        assert!(state.committed);
        assert!(state.tables.is_empty());
        assert_eq!(state.shutdown_generation(), None);
    }

    #[test]
    fn touching_world_bounds_prevent_restore() {
        let proposed = Bounds {
            min: [-1.0, 0.0, -1.0],
            max: [1.0, 2.0, 1.0],
        };
        let touching = Bounds {
            min: [1.0, 0.5, -0.5],
            max: [2.0, 1.5, 0.5],
        };
        let separate = Bounds {
            min: [1.001, 0.5, -0.5],
            max: [2.0, 1.5, 0.5],
        };
        assert!(intersects(proposed, touching));
        assert!(!intersects(proposed, separate));
    }

    #[test]
    fn nonfinite_or_inverted_bounds_are_not_clearance() {
        assert!(!valid_bounds(Bounds {
            min: [0.0; 3],
            max: [f32::NAN; 3]
        }));
        assert!(!valid_bounds(Bounds {
            min: [1.0; 3],
            max: [0.0; 3]
        }));
    }

    #[test]
    fn human_policy_excludes_every_quadruped_state() {
        assert_eq!(HUMAN_STATES.len(), 7);
        for state in HUMAN_STATES {
            assert!(
                ![
                    ProxyState::QUADRUPED_LARGE as i32,
                    ProxyState::QUADRUPED_SMALL as i32,
                    ProxyState::QUADRUPED_TINY as i32,
                ]
                .contains(&(state as i32))
            );
        }
    }
}
