//! Retained native shape tables and bounded world-body transactions.

mod restore;
mod scan;

use std::{
    collections::HashSet,
    ptr::NonNull,
    sync::atomic::{AtomicU32, Ordering},
};

use jc3gi::{
    character::character::Character,
    havok::{hkReferencedObject, hknpShape},
    physics::{ProxyState, SharedPfxCharacterSettings, hknpBody, hknpWorld},
};

use crate::controllers::physical_hands::{
    bodies,
    characters::engine::proxy_body,
    human_hull::{
        ClearanceUnavailableReason,
        geometry::{self, GeometryError},
        kept::{self, KeptPair},
        native::{
            restore::{Allowance, Plan, RetryBackoff, Surroundings, check_body, plan},
            scan::{DiscoveryClock, Lookup, Migrated, scan},
        },
    },
};

pub(super) struct NativeState {
    pub(super) tables: Vec<Table>,
    pub(super) enabled: bool,
    pub(super) committed: bool,
    pub(super) shutdown_requested: bool,
    pub(super) migrated_bodies: usize,
    pub(super) original_active: bool,
    pub(super) clearance: ClearanceUnavailableReason,
    /// Shutdown attempts deferred while restoration was incomplete.
    pub(super) shutdown_attempts: u32,
    /// The clearance reason last reported for a deferred shutdown attempt.
    pub(super) reported_clearance: Option<ClearanceUnavailableReason>,
    generation: u64,
    mutation_fault: bool,
    /// Whether the local player's body last had a narrowed shape, reported on each change.
    player_narrowed: Option<bool>,
    /// The shape pairs earlier payloads in this process left in place, read on first use.
    kept: Option<Vec<KeptPair>>,
    /// The adopted pairs by shape, dropped whenever the tables change and rebuilt on next use.
    lookup: Option<Lookup>,
    discovery: DiscoveryClock,
    retry: RetryBackoff,
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
            shutdown_attempts: 0,
            reported_clearance: None,
            generation: 0,
            mutation_fault: false,
            player_narrowed: None,
            kept: None,
            lookup: None,
            discovery: DiscoveryClock::new(),
            retry: RetryBackoff::new(),
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

    /// The current bookkeeping generation: any adoption, forgetting, or native pass changes it.
    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    /// The native phase has restored every tracked body, and pending hooks cannot install shapes.
    ///
    /// The retained originals are released: every slot and restored body holds its own reference.
    /// The replacements are deliberately kept: a body made from a slot read before the restoration
    /// may still name one, and a kept convex shape is inert after unload, while a released one
    /// could be freed under a live body.
    pub(super) unsafe fn commit_shutdown(&mut self) {
        assert!(
            self.shutdown_generation().is_some(),
            "human hull: commit before native restoration"
        );
        self.committed = true;
        for table in &mut self.tables {
            unsafe { table.release_originals() };
        }
        self.tables.clear();
        self.lookup = None;
    }

    /// Restoration did not complete in time: stop adopting and leave every table and live body
    /// with the shapes it has now.
    ///
    /// Every retained reference, original and replacement, is kept, so no shape a slot or a body
    /// names can be freed: the shapes are the engine's own convex shapes on its own heap, with no
    /// pointer into the payload, and a settings table's destructor releases its slots' own
    /// references as for any shape. Pending construction hooks see the commit and keep the
    /// authored geometry. The pairs are recorded for the next payload ([`kept`]), which would
    /// otherwise read a replacement still in a slot as authored geometry and narrow it again.
    pub(super) fn commit_in_place(&mut self) {
        self.committed = true;
        let kept = self.kept.get_or_insert_with(kept::load);
        let pairs = self.tables.iter().flat_map(|table| &table.pairs);
        if kept::merge(kept, pairs.map(Pair::kept)) {
            kept::store(kept);
        }
        // Dropping the tables drops the retained pointers without releasing them.
        self.tables.clear();
        self.lookup = None;
    }

    pub(super) unsafe fn adopt(
        &mut self,
        settings: NonNull<SharedPfxCharacterSettings>,
    ) -> Result<(), GeometryError> {
        unsafe {
            self.adopt_with(settings, |shape| {
                geometry::narrow(shape).map(|(replacement, _)| replacement)
            })
        }
    }

    /// Adopt `settings`, narrowing each human slot's authored shape with `narrow`.
    ///
    /// A slot that holds either shape of a pair an earlier payload left in place ([`kept`]) takes
    /// that pair instead: its replacement is never authored geometry, and narrowing it again would
    /// halve the width on every reinjection.
    unsafe fn adopt_with(
        &mut self,
        settings: NonNull<SharedPfxCharacterSettings>,
        mut narrow: impl FnMut(NonNull<hknpShape>) -> Result<NonNull<hknpShape>, GeometryError>,
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
        let kept = self.kept.get_or_insert_with(kept::load);
        for state in HUMAN_STATES {
            let index = state as usize;
            let Some(slot) = NonNull::new(unsafe { settings.as_ref() }.m_CapsuleShape[index])
            else {
                unsafe { table.release_pairs() };
                return Err(GeometryError::InvalidGeometry);
            };
            let kept_pair = kept::find(kept, index, slot.as_ptr() as usize);
            let pair = match kept_pair {
                Some(pair) => Pair::from_kept(pair),
                None => narrow(slot).map(|replacement| Pair {
                    index,
                    original: slot,
                    replacement,
                }),
            };
            let pair = match pair {
                Ok(pair) => pair,
                Err(error) => {
                    unsafe { table.release_pairs() };
                    return Err(error);
                }
            };
            // Every pair holds one reference to each of its shapes. A narrowed replacement arrives
            // with its creation reference; a kept one is shared with the references left behind.
            unsafe { retain(pair.original) };
            if kept_pair.is_some() {
                unsafe { retain(pair.replacement) };
            }
            table.pairs.push(pair);
        }
        // Whatever the slots held, they now hold what the enabled setting asks for.
        unsafe { table.set_installed(self.enabled) };
        if self.enabled {
            self.original_active = false;
        }
        self.tables.push(table);
        self.lookup = None;
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
            self.lookup = None;
            self.advance_generation();
        }
    }

    /// Discovers settings through every live body's typed native owner, including NPCs that
    /// existed before hook installation, when the [`DiscoveryClock`] says a walk is due. No owner
    /// pointer survives this native phase.
    pub(super) unsafe fn discover(
        &mut self,
        world: &hknpWorld,
        bodies: &[u32],
    ) -> Result<(), ClearanceUnavailableReason> {
        if !self.discovery.due(!self.tables.is_empty()) {
            return Ok(());
        }
        self.original_active = false;
        self.advance_generation();
        let mut owners = HashSet::new();
        let type_id = unsafe { Character::TYPE_ID() };
        let settings = bodies.iter().filter_map(|&id| {
            let owner = unsafe { bodies::body_owner(world, id) }?;
            if !owners.insert(owner.get()) {
                return None;
            }
            if !unsafe { bodies::is_type(owner, type_id) } {
                return None;
            }
            let character = unsafe { &*(owner.get() as *const Character) };
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

    /// Narrow or restore the live bodies of `bodies`, the pass's [`inventory`]. Changing a body's
    /// shape never changes which slots hold live bodies, so the one inventory serves the whole
    /// pass.
    pub(super) unsafe fn synchronize(
        &mut self,
        world: &mut hknpWorld,
        bodies: &[u32],
    ) -> Result<(), ClearanceUnavailableReason> {
        let restore = self.shutdown_requested || !self.enabled;
        // A disabled hull with a body that stays obstructed waits out its backoff untouched; a
        // shutdown's attempts are bounded instead.
        if restore && !self.shutdown_requested && !self.original_active && !self.retry.ready() {
            return Ok(());
        }
        if !restore {
            self.retry.reset();
        }
        self.advance_generation();
        self.original_active = false;
        if self.mutation_fault {
            return Err(ClearanceUnavailableReason::RollbackRejected);
        }
        for table in &self.tables {
            unsafe { table.validate_slots()? };
        }
        let lookup = self
            .lookup
            .get_or_insert_with(|| Lookup::build(&self.tables));
        let shapes = bodies.iter().map(|&id| {
            (id, unsafe {
                (*world.m_bodies.add(id as usize)).m_shape.cast_const()
            })
        });
        let scan = scan(shapes, lookup);
        self.migrated_bodies = scan.migrated.len();
        let mut blocked = None;
        let changes = if restore {
            let plan = unsafe { self.plan_restore(world, bodies, &scan.migrated) };
            blocked = plan.blocked;
            plan.approved
                .iter()
                .map(|body| Change {
                    id: body.id,
                    from: body.replacement,
                    to: body.original,
                })
                .collect()
        } else {
            scan.narrowing
        };
        if let Err(reason) = unsafe { apply(world, &changes) } {
            self.mutation_fault = reason == ClearanceUnavailableReason::RollbackRejected;
            if self.mutation_fault {
                self.migrated_bodies = unsafe { self.count_migrated(world, bodies) };
            }
            return Err(reason);
        }
        self.migrated_bodies = if restore {
            self.migrated_bodies - changes.len()
        } else {
            self.migrated_bodies + changes.len()
        };
        unsafe { self.report_player(world) };
        if restore && self.migrated_bodies != 0 {
            // The tables keep the replacements until every body is restored: a narrowed body that
            // changes proxy state takes its new shape from them, and must not take an authored one
            // that no check has cleared. A restored body that changes state is narrowed again and
            // restored on a later pass.
            if !self.shutdown_requested {
                self.retry.record(!changes.is_empty());
            }
            return Err(blocked.unwrap_or(ClearanceUnavailableReason::MutationRejected));
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
        bodies: &[u32],
    ) -> Result<(), ClearanceUnavailableReason> {
        self.request_shutdown();
        unsafe { self.synchronize(world, bodies) }
    }

    fn advance_generation(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("human hull: generation exhausted");
    }

    /// Log whether the local player's body has a narrowed shape whenever that changes, so that a
    /// session log shows the player's own hull taking effect.
    unsafe fn report_player(&mut self, world: &hknpWorld) {
        let Some(id) = (unsafe { player_body(world) }) else {
            return;
        };
        let current = unsafe { (*world.m_bodies.add(id as usize)).m_shape };
        let narrowed = self
            .lookup
            .as_ref()
            .is_some_and(|lookup| lookup.original_of(current).is_some());
        if self.player_narrowed.replace(narrowed) != Some(narrowed) {
            tracing::info!(
                narrowed,
                body = id,
                settings = self.tables.len(),
                migrated_bodies = self.migrated_bodies,
                "human hull: the local player's body shape changed"
            );
        }
    }

    /// Recount the migrated bodies after a rollback the engine refused, when the scan's count no
    /// longer says which shapes the bodies hold.
    unsafe fn count_migrated(&self, world: &hknpWorld, bodies: &[u32]) -> usize {
        let Some(lookup) = &self.lookup else {
            return 0;
        };
        bodies
            .iter()
            .filter(|&&id| {
                let current = unsafe { (*world.m_bodies.add(id as usize)).m_shape };
                lookup.original_of(current).is_some()
            })
            .count()
    }

    /// Decide which migrated bodies this pass restores ([`restore`]), each against the live
    /// bodies around it and the live bodies not yet in the broad phase.
    unsafe fn plan_restore(
        &self,
        world: &hknpWorld,
        bodies: &[u32],
        migrated: &[Migrated],
    ) -> Plan {
        let Some(lookup) = &self.lookup else {
            return Plan {
                approved: Vec::new(),
                blocked: Some(ClearanceUnavailableReason::IncompleteInventory),
            };
        };
        let pending: Vec<u32> = bodies
            .iter()
            .copied()
            .filter(|&id| {
                let broad_phase = unsafe { (*world.m_bodies.add(id as usize)).m_broadPhaseId };
                broad_phase == hknpBody::INVALID_BROAD_PHASE_ID
            })
            .collect();
        let surroundings = Surroundings {
            world,
            lookup,
            pending: &pending,
        };
        let mut allowance = Allowance::new();
        plan(migrated, |body, approved| unsafe {
            check_body(&surroundings, body, approved, &mut allowance)
        })
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

    /// Point every slot at its pair's replacement or original. Each slot is checked on its own,
    /// so a table adopted with a mix of the two ends up consistent.
    unsafe fn set_installed(&mut self, install: bool) {
        let Some(mut settings) = self.settings else {
            return;
        };
        for pair in &self.pairs {
            let next = if install {
                pair.replacement
            } else {
                pair.original
            };
            if unsafe { settings.as_ref() }.m_CapsuleShape[pair.index] == next.as_ptr() {
                continue;
            }
            unsafe { retain(next) };
            let old = unsafe { settings.as_mut() }.m_CapsuleShape[pair.index];
            unsafe { settings.as_mut() }.m_CapsuleShape[pair.index] = next.as_ptr();
            if let Some(old) = NonNull::new(old) {
                unsafe { release(old) };
            }
        }
        self.installed = install;
    }

    unsafe fn release_originals(&mut self) {
        for pair in self.pairs.drain(..) {
            unsafe { release(pair.original) };
        }
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

impl Pair {
    fn kept(&self) -> KeptPair {
        KeptPair {
            index: self.index,
            original: self.original.as_ptr() as usize,
            replacement: self.replacement.as_ptr() as usize,
        }
    }

    /// The references an earlier payload left keep both shapes alive for the life of the process.
    fn from_kept(pair: KeptPair) -> Result<Self, GeometryError> {
        let shape = |address: usize| {
            NonNull::new(address as *mut hknpShape).ok_or(GeometryError::InvalidGeometry)
        };
        Ok(Self {
            index: pair.index,
            original: shape(pair.original)?,
            replacement: shape(pair.replacement)?,
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Change {
    id: u32,
    from: NonNull<hknpShape>,
    to: NonNull<hknpShape>,
}

/// The ids of the live bodies with a shape, by [`slot_kind`].
pub(super) unsafe fn inventory(world: &hknpWorld) -> Result<Vec<u32>, ClearanceUnavailableReason> {
    if world.m_bodies.is_null() || world.m_peakBodyIndex > MAXIMUM_BODY_INDEX {
        return Err(ClearanceUnavailableReason::IncompleteInventory);
    }
    let mut bodies = Vec::new();
    for id in 0..=world.m_peakBodyIndex {
        let body = unsafe { &*world.m_bodies.add(id as usize) };
        match slot_kind(body.m_flags, body.m_motionId, !body.m_shape.is_null()) {
            SlotKind::Free | SlotKind::Shapeless => {}
            SlotKind::Live => bodies.push(id),
        }
    }
    Ok(bodies)
}

/// What a body slot holds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum SlotKind {
    /// No body: a slot never used, or one whose body was destroyed or freed.
    Free,
    /// A live body without a shape, such as the world's fixed body, which the body manager makes
    /// static and shapeless when the world is built and which constraints to the world name. It
    /// cannot hold a narrowed replacement and has nothing to collide with, so it has nothing to
    /// restore and no clearance to check.
    Shapeless,
    /// A live body with a shape.
    Live,
}

/// Classify a slot by the engine's own validity test, a static or dynamic flag, and its motion.
///
/// Destroying a body clears its type flags at once and freeing the slot later invalidates its
/// motion, but neither clears its shape ([`hknpBody::INVALID_MOTION_ID`]), so a dead human's slot
/// keeps naming the narrowed replacement until the slot is reused. Its shape is therefore never
/// evidence of a body: such a slot has nothing to restore, and the body it once held released its
/// reference when it was destroyed.
fn slot_kind(flags: u32, motion_id: u32, has_shape: bool) -> SlotKind {
    if flags & (hknpBody::IS_STATIC | hknpBody::IS_DYNAMIC) == 0
        || motion_id == hknpBody::INVALID_MOTION_ID
    {
        SlotKind::Free
    } else if !has_shape {
        SlotKind::Shapeless
    } else {
        SlotKind::Live
    }
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

/// The local player's character body, when it names a slot of `world`.
///
/// # Safety
///
/// The native physics phase, with `world` the live world.
unsafe fn player_body(world: &hknpWorld) -> Option<u32> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    let body = unsafe { proxy_body(character) }?;
    unsafe { bodies::slot(world, body) }.map(|_| body)
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

#[cfg(test)]
mod tests;
