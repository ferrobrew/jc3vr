//! Retained native shape tables and bounded world-body transactions.

use std::{
    collections::HashSet,
    ptr::{self, NonNull},
    sync::atomic::{AtomicU32, Ordering},
};

use jc3gi::{
    character::character::Character,
    game::GameObject,
    havok::{hkAabb, hkReferencedObject, hknpShape},
    physics::{
        PfxInstance, PhysicsGameObject, ProxyState, SharedPfxCharacterSettings, hknpBody, hknpWorld,
    },
};

use crate::controllers::physical_hands::human_hull::{
    ClearanceUnavailableReason,
    geometry::{self, Bounds, GeometryError},
    query::{self, Budget},
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
    mutation_fault: bool,
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
            mutation_fault: false,
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
            && !self.mutation_fault
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
            let (replacement, _) = match unsafe { geometry::narrow(original) } {
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

    /// Discovers settings through every live body's typed native owner, including NPCs that
    /// existed before hook installation. No owner pointer survives this native phase.
    pub(super) unsafe fn discover(
        &mut self,
        world: &hknpWorld,
    ) -> Result<(), ClearanceUnavailableReason> {
        self.original_active = false;
        self.advance_generation();
        let bodies = unsafe { inventory(world, &self.tables)? };
        let mut owners = HashSet::new();
        let type_id = unsafe { Character::TYPE_ID() };
        let settings = bodies.into_iter().filter_map(|id| {
            let owner = unsafe { body_owner(world, id) }?;
            if !owners.insert(owner.as_ptr() as usize) {
                return None;
            }
            let object = unsafe {
                &*owner
                    .as_ptr()
                    .cast::<u8>()
                    .add(PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize)
                    .cast::<GameObject>()
            };
            if !unsafe { object.IsType(type_id) } {
                return None;
            }
            let character = unsafe { &*owner.as_ptr().cast::<Character>() };
            let proxy = unsafe { character.m_PfxCharacter.px.as_ref() }?;
            Some(
                unsafe { proxy.m_Owner.as_mut() }
                    .and_then(|owner| {
                        NonNull::new(unsafe { owner.GetSharedPfxCharacterSettings() })
                    })
                    .ok_or(ClearanceUnavailableReason::IncompleteInventory),
            )
        });
        unsafe { self.adopt_discovered(settings) }
    }

    unsafe fn adopt_discovered(
        &mut self,
        settings: impl IntoIterator<
            Item = Result<NonNull<SharedPfxCharacterSettings>, ClearanceUnavailableReason>,
        >,
    ) -> Result<(), ClearanceUnavailableReason> {
        for settings in settings {
            unsafe { self.adopt(settings?) }.map_err(|error| {
                tracing::warn!(?error, "human hull: discovered native geometry rejected");
                ClearanceUnavailableReason::InvalidGeometry
            })?;
        }
        Ok(())
    }

    pub(super) unsafe fn synchronize(
        &mut self,
        world: &mut hknpWorld,
    ) -> Result<(), ClearanceUnavailableReason> {
        self.advance_generation();
        self.original_active = false;
        if self.mutation_fault {
            return Err(ClearanceUnavailableReason::RollbackRejected);
        }
        let bodies = unsafe { inventory(world, &self.tables)? };
        for table in &self.tables {
            unsafe { table.validate_slots()? };
        }
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
        if let Err(reason) = unsafe { apply(world, &changes) } {
            self.mutation_fault = reason == ClearanceUnavailableReason::RollbackRejected;
            self.migrated_bodies = unsafe { self.count_migrated(world, &bodies) };
            return Err(reason);
        }
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
        let mut budget = Budget::new();
        for &id in bodies {
            let body = unsafe { &*world.m_bodies.add(id as usize) };
            let Some(pair) = self.replacement_pair(body.m_shape) else {
                continue;
            };
            if body.m_broadPhaseId == hknpBody::INVALID_BROAD_PHASE_ID {
                return Err(ClearanceUnavailableReason::PendingBody);
            }
            let proposed = unsafe { query::world_bounds(pair.original, &body.m_transform)? };
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
                    let shape = NonNull::new(other.m_shape)
                        .ok_or(ClearanceUnavailableReason::IncompleteInventory)?;
                    let pending_bounds = unsafe { query::world_bounds(shape, &other.m_transform)? };
                    if query::intersects(proposed, pending_bounds) {
                        return Err(ClearanceUnavailableReason::PendingBody);
                    }
                    continue;
                }
                let other_pair = self.replacement_pair(other.m_shape);
                let (target_shape, target_bounds) = if let Some(other_pair) = other_pair {
                    (other_pair.original, unsafe {
                        query::world_bounds(other_pair.original, &other.m_transform)?
                    })
                } else {
                    let mut aabb = hkAabb::default();
                    unsafe { world.getBodyAabb(other_id, &mut aabb) };
                    let bounds = Bounds {
                        min: aabb.m_min.data,
                        max: aabb.m_max.data,
                    };
                    if !query::valid_bounds(bounds) {
                        return Err(ClearanceUnavailableReason::InvalidBounds);
                    }
                    (
                        NonNull::new(other.m_shape)
                            .ok_or(ClearanceUnavailableReason::IncompleteInventory)?,
                        bounds,
                    )
                };
                if query::intersects(proposed, target_bounds) {
                    unsafe {
                        query::check_target(
                            pair.original,
                            pair.replacement,
                            &body.m_transform,
                            target_shape,
                            &other.m_transform,
                            &mut budget,
                        )?
                    };
                }
            }
        }
        Ok(())
    }

    fn replacement_pair(&self, shape: *const hknpShape) -> Option<&Pair> {
        self.tables
            .iter()
            .flat_map(|table| &table.pairs)
            .find(|pair| shape == pair.replacement.as_ptr())
    }
}

pub(super) struct Table {
    settings: Option<NonNull<SharedPfxCharacterSettings>>,
    pairs: Vec<Pair>,
    installed: bool,
}

impl Table {
    unsafe fn validate_slots(&self) -> Result<(), ClearanceUnavailableReason> {
        let Some(settings) = self.settings else {
            return Ok(());
        };
        for pair in &self.pairs {
            let expected = if self.installed {
                pair.replacement
            } else {
                pair.original
            };
            if unsafe { settings.as_ref() }.m_CapsuleShape[pair.index] != expected.as_ptr() {
                return Err(ClearanceUnavailableReason::MutationRejected);
            }
        }
        Ok(())
    }

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
    apply_with(changes, |id, shape| {
        unsafe { world.setBodyShape(id, shape.as_ptr().cast()) };
        unsafe { (*world.m_bodies.add(id as usize)).m_shape == shape.as_ptr() }
    })
}

fn apply_with(
    changes: &[Change],
    mut set_verified: impl FnMut(u32, NonNull<hknpShape>) -> bool,
) -> Result<(), ClearanceUnavailableReason> {
    for (index, change) in changes.iter().enumerate() {
        if !set_verified(change.id, change.to) {
            let mut recovered = true;
            for previous in changes[..=index].iter().rev() {
                recovered &= set_verified(previous.id, previous.from);
            }
            return Err(if recovered {
                ClearanceUnavailableReason::MutationRejected
            } else {
                ClearanceUnavailableReason::RollbackRejected
            });
        }
    }
    Ok(())
}

unsafe fn body_owner(world: &hknpWorld, id: u32) -> Option<NonNull<PhysicsGameObject>> {
    let buffer = unsafe {
        world
            .m_bodyProperties
            .getWithDefault(hknpWorld::BODY_PROPERTY_PFX_INSTANCE, ptr::null_mut())
            .as_ref()
    }?;
    if id >= buffer.m_capacity || buffer.m_occupancy.is_null() || buffer.m_data.is_null() {
        return None;
    }
    let occupied = unsafe { *buffer.m_occupancy.add((id / 32) as usize) } & (1 << (id % 32)) != 0;
    if !occupied {
        return None;
    }
    let instance =
        unsafe { (*buffer.m_data.cast::<*const PfxInstance>().add(id as usize)).as_ref() }?;
    NonNull::new(instance.m_GameObject.cast())
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
        assert!(query::intersects(proposed, touching));
        assert!(!query::intersects(
            proposed,
            Bounds {
                min: [1.01, 0.5, -0.5],
                ..separate
            }
        ));
    }

    #[test]
    fn nonfinite_or_inverted_bounds_are_not_clearance() {
        assert!(!query::valid_bounds(Bounds {
            min: [0.0; 3],
            max: [f32::NAN; 3]
        }));
        assert!(!query::valid_bounds(Bounds {
            min: [1.0; 3],
            max: [0.0; 3]
        }));
    }

    #[test]
    fn rejected_body_mutation_rolls_back_every_attempted_body_in_reverse() {
        let from = NonNull::dangling();
        let mut target = std::mem::MaybeUninit::<hknpShape>::uninit();
        let to = NonNull::new(target.as_mut_ptr()).unwrap();
        let changes = [Change { id: 3, from, to }, Change { id: 8, from, to }];
        let mut calls = Vec::new();
        let result = apply_with(&changes, |id, shape| {
            calls.push((id, shape));
            !(id == 8 && shape == to)
        });
        assert_eq!(result, Err(ClearanceUnavailableReason::MutationRejected));
        assert_eq!(calls, [(3, to), (8, to), (8, from), (3, from)]);
    }

    #[test]
    fn rollback_rejection_is_distinct_and_never_approves_shutdown() {
        let from = NonNull::dangling();
        let mut target = std::mem::MaybeUninit::<hknpShape>::uninit();
        let to = NonNull::new(target.as_mut_ptr()).unwrap();
        let result = apply_with(&[Change { id: 3, from, to }], |_, _| false);
        assert_eq!(result, Err(ClearanceUnavailableReason::RollbackRejected));
        let mut state = NativeState::new();
        state.request_shutdown();
        state.original_active = true;
        state.mutation_fault = true;
        assert_eq!(state.shutdown_generation(), None);
    }

    #[test]
    fn later_distinct_npc_settings_invalidate_an_earlier_restoration_receipt() {
        let mut state = NativeState::new();
        state.request_shutdown();
        state.original_active = true;
        let initial = state.shutdown_generation().unwrap();
        let earlier_npc = NonNull::dangling();
        let later_npc = NonNull::new(16usize as *mut SharedPfxCharacterSettings).unwrap();
        // Pending adoption records identities without dereferencing native settings.
        unsafe { state.adopt_discovered([Ok(earlier_npc), Ok(earlier_npc)]) }.unwrap();
        let after_first = state.shutdown_generation().unwrap();
        unsafe { state.adopt_discovered([Ok(earlier_npc), Ok(later_npc)]) }.unwrap();
        assert_eq!(state.tables.len(), 2);
        assert_ne!(state.shutdown_generation(), Some(initial));
        assert_ne!(state.shutdown_generation(), Some(after_first));
        let after_later = state.shutdown_generation();
        unsafe { state.adopt(later_npc) }.unwrap();
        assert_eq!(state.tables.len(), 2);
        assert_eq!(state.shutdown_generation(), after_later);
    }

    #[test]
    fn discovery_fault_is_not_a_complete_world_pass() {
        let mut state = NativeState::new();
        state.request_shutdown();
        let result = unsafe {
            state.adopt_discovered([Err(ClearanceUnavailableReason::IncompleteInventory)])
        };
        assert_eq!(result, Err(ClearanceUnavailableReason::IncompleteInventory));
        assert_eq!(state.shutdown_generation(), None);
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
