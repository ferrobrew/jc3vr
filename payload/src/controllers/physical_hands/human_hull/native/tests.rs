use std::mem::MaybeUninit;

use crate::controllers::physical_hands::human_hull::{geometry::Bounds, query};

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
    let result =
        unsafe { state.adopt_discovered([Err(ClearanceUnavailableReason::IncompleteInventory)]) };
    assert_eq!(result, Err(ClearanceUnavailableReason::IncompleteInventory));
    assert_eq!(state.shutdown_generation(), None);
}

#[test]
fn a_destroyed_or_freed_slot_is_free_whatever_shape_it_still_names() {
    let dynamic = hknpBody::IS_DYNAMIC;
    // Destroyed: the type flags are cleared while the motion and stale shape remain.
    assert_eq!(slot_kind(0, 7, true), SlotKind::Free);
    // Freed: the motion is invalidated and the stale shape remains.
    assert_eq!(
        slot_kind(0, hknpBody::INVALID_MOTION_ID, true),
        SlotKind::Free
    );
    assert_eq!(
        slot_kind(dynamic, hknpBody::INVALID_MOTION_ID, true),
        SlotKind::Free
    );
    assert_eq!(slot_kind(dynamic, 7, true), SlotKind::Live);
    // A static body has the static motion.
    assert_eq!(slot_kind(hknpBody::IS_STATIC, 0, true), SlotKind::Live);
}

#[test]
fn the_worlds_shapeless_fixed_body_does_not_stall_the_inventory() {
    // The body manager makes the world's fixed body static, with the static motion and no
    // shape. Every world has one, so counting it as unaccountable refused every native pass.
    assert_eq!(
        slot_kind(hknpBody::IS_STATIC, 0, false),
        SlotKind::Shapeless
    );
    assert_eq!(
        slot_kind(hknpBody::IS_DYNAMIC, 7, false),
        SlotKind::Shapeless
    );
}

#[test]
fn leaving_the_hulls_in_place_commits_whatever_restoration_reached() {
    let mut state = NativeState::new();
    state.request_shutdown();
    state.migrated_bodies = 3;
    state.mutation_fault = true;
    assert_eq!(state.shutdown_generation(), None);
    let settings = NonNull::dangling();
    // SAFETY: pending adoption only records this identity and never dereferences it.
    unsafe { state.adopt(settings) }.unwrap();
    state.commit_in_place();
    assert!(state.committed);
    assert!(state.tables.is_empty());
    // Construction after the commit keeps the authored geometry and records nothing.
    unsafe { state.adopt(settings) }.unwrap();
    assert!(state.tables.is_empty());
}

/// Fake heap shapes with live reference counts, which retain and release adjust in place.
struct FakeShapes(Vec<MaybeUninit<hknpShape>>);

impl FakeShapes {
    fn new(count: usize, references: u32) -> Self {
        let mut shapes: Vec<MaybeUninit<hknpShape>> =
            (0..count).map(|_| MaybeUninit::zeroed()).collect();
        for shape in &mut shapes {
            let object = shape.as_mut_ptr().cast::<hkReferencedObject>();
            // SAFETY: the zeroed storage is large enough for the shape's referenced header.
            unsafe {
                (*object).m_memSizeAndRefCount =
                    (1 << hkReferencedObject::MEMORY_SIZE_SHIFT) | references
            };
        }
        Self(shapes)
    }

    fn get(&mut self, index: usize) -> NonNull<hknpShape> {
        NonNull::new(self.0[index].as_mut_ptr()).unwrap()
    }

    fn references(&mut self, index: usize) -> u32 {
        let object = self.get(index).as_ptr().cast::<hkReferencedObject>();
        // SAFETY: the header was initialized in `new`.
        unsafe { (*object).m_memSizeAndRefCount & hkReferencedObject::REFERENCE_COUNT_MASK }
    }
}

/// A settings table whose human slots hold `slot(state)`, and the pairs an earlier payload
/// left in place for those slots.
fn kept_table(
    originals: &mut FakeShapes,
    replacements: &mut FakeShapes,
    slot_holds_replacement: bool,
) -> (Box<MaybeUninit<SharedPfxCharacterSettings>>, Vec<KeptPair>) {
    let mut settings = Box::new(MaybeUninit::<SharedPfxCharacterSettings>::zeroed());
    let mut kept = Vec::new();
    for (slot, state) in HUMAN_STATES.into_iter().enumerate() {
        let index = state as usize;
        let (original, replacement) = (originals.get(slot), replacements.get(slot));
        // SAFETY: the zeroed table is a valid table of null slots.
        unsafe { settings.assume_init_mut() }.m_CapsuleShape[index] = if slot_holds_replacement {
            replacement.as_ptr()
        } else {
            original.as_ptr()
        };
        kept.push(KeptPair {
            index,
            original: original.as_ptr() as usize,
            replacement: replacement.as_ptr() as usize,
        });
    }
    (settings, kept)
}

#[test]
fn a_replacement_left_in_place_is_adopted_and_never_narrowed_again() {
    // Left in place: each slot and the leaked payload reference hold the replacement, and the
    // leaked reference holds the original.
    let mut originals = FakeShapes::new(HUMAN_STATES.len(), 1);
    let mut replacements = FakeShapes::new(HUMAN_STATES.len(), 2);
    let (mut settings, kept) = kept_table(&mut originals, &mut replacements, true);
    let mut state = NativeState::new();
    state.kept = Some(kept);
    let table = NonNull::new(settings.as_mut_ptr()).unwrap();
    unsafe { state.adopt_with(table, |_| panic!("narrowed a kept replacement")) }.unwrap();
    assert_eq!(state.tables.len(), 1);
    assert!(state.tables[0].installed);
    for (slot, state_kind) in HUMAN_STATES.into_iter().enumerate() {
        let pair = &state.tables[0].pairs[slot];
        assert_eq!(pair.original, originals.get(slot));
        assert_eq!(pair.replacement, replacements.get(slot));
        let held = unsafe { settings.assume_init_ref() }.m_CapsuleShape[state_kind as usize];
        assert_eq!(held, replacements.get(slot).as_ptr());
        assert_eq!(originals.references(slot), 2);
        assert_eq!(replacements.references(slot), 3);
    }
    assert_eq!(unsafe { state.tables[0].validate_slots() }, Ok(()));
}

#[test]
fn a_disabled_hull_puts_the_authored_shape_back_in_a_kept_slot() {
    let mut originals = FakeShapes::new(HUMAN_STATES.len(), 1);
    let mut replacements = FakeShapes::new(HUMAN_STATES.len(), 2);
    let (mut settings, kept) = kept_table(&mut originals, &mut replacements, true);
    let mut state = NativeState::new();
    state.enabled = false;
    state.kept = Some(kept);
    let table = NonNull::new(settings.as_mut_ptr()).unwrap();
    unsafe { state.adopt_with(table, |_| panic!("narrowed a kept replacement")) }.unwrap();
    assert!(!state.tables[0].installed);
    for (slot, state_kind) in HUMAN_STATES.into_iter().enumerate() {
        let held = unsafe { settings.assume_init_ref() }.m_CapsuleShape[state_kind as usize];
        assert_eq!(held, originals.get(slot).as_ptr());
        // The pair's and the slot's references to the original; the slot's to the replacement
        // went back.
        assert_eq!(originals.references(slot), 3);
        assert_eq!(replacements.references(slot), 2);
    }
    assert_eq!(unsafe { state.tables[0].validate_slots() }, Ok(()));
}

#[test]
fn a_kept_original_in_a_slot_takes_its_kept_replacement() {
    // Restored before the eject left the pair: the slot holds the original again.
    let mut originals = FakeShapes::new(HUMAN_STATES.len(), 2);
    let mut replacements = FakeShapes::new(HUMAN_STATES.len(), 1);
    let (mut settings, kept) = kept_table(&mut originals, &mut replacements, false);
    let mut state = NativeState::new();
    state.kept = Some(kept);
    let table = NonNull::new(settings.as_mut_ptr()).unwrap();
    unsafe { state.adopt_with(table, |_| panic!("narrowed a kept original")) }.unwrap();
    for (slot, state_kind) in HUMAN_STATES.into_iter().enumerate() {
        let held = unsafe { settings.assume_init_ref() }.m_CapsuleShape[state_kind as usize];
        assert_eq!(held, replacements.get(slot).as_ptr());
        assert_eq!(originals.references(slot), 2);
        assert_eq!(replacements.references(slot), 3);
    }
}

#[test]
fn an_authored_table_is_narrowed_once_per_human_slot() {
    let mut originals = FakeShapes::new(HUMAN_STATES.len(), 1);
    let mut replacements = FakeShapes::new(HUMAN_STATES.len(), 1);
    let (mut settings, _) = kept_table(&mut originals, &mut replacements, false);
    let mut state = NativeState::new();
    state.kept = Some(Vec::new());
    let table = NonNull::new(settings.as_mut_ptr()).unwrap();
    let mut narrowed = 0;
    unsafe {
        state.adopt_with(table, |_| {
            narrowed += 1;
            Ok(replacements.get(narrowed - 1))
        })
    }
    .unwrap();
    assert_eq!(narrowed, HUMAN_STATES.len());
    for slot in 0..HUMAN_STATES.len() {
        // The pair's reference to the original, the slot's having gone back; the creation's
        // and the slot's to the replacement.
        assert_eq!(originals.references(slot), 1);
        assert_eq!(replacements.references(slot), 2);
    }
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
