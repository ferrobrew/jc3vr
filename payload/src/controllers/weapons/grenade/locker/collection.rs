//! Static grenade display snapshots captured during their owning location load.
//!
//! Native instances have no game-object weak reference. Only the load callback reads them; the
//! registry retains copied matrices and identities, and retirement precedes native destruction.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    graphics_engine::model_instance::{
        ModelCollection, ModelInstance, ModelInstanceFlags, ModelManager,
    },
    location::Location,
    types::math::Matrix4,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::weapons::grenade::locker::{
    Identity, Provider, Source,
    format::{Dictionary, MAX_BYTES, dictionary},
};

pub(super) fn hook_library() -> HookLibrary {
    if !CLEANUP.swap(true, Ordering::AcqRel) {
        crate::lifecycle::on_cleanup(|_| {
            let mut registry = REGISTRY.lock();
            registry.closed = true;
            registry.collections.clear();
        });
    }
    HookLibrary::new()
        .with_static_binder(&LOAD_MODEL_COLLECTION_BINDER)
        .with_static_binder(&COLLECTION_DESTRUCT_BINDER)
}

pub(super) fn active_entries() -> usize {
    ACTIVE_ENTRIES.load(Ordering::Acquire)
}

/// Keep retirement excluded until exact-source validation and the safe draw both finish.
pub(super) fn with_sources<T>(
    to_cockpit: &impl Fn(Vec3) -> Vec3,
    read: impl FnOnce(Vec<Source>) -> T,
) -> T {
    read_sources(&REGISTRY, to_cockpit, read)
}

fn read_sources<T>(
    registry: &Mutex<Registry>,
    to_cockpit: &impl Fn(Vec3) -> Vec3,
    read: impl FnOnce(Vec<Source>) -> T,
) -> T {
    let registry = registry.lock();
    let sources = registry.sources(to_cockpit);
    let result = read(sources);
    drop(registry);
    result
}

const MAX_COLLECTIONS: usize = 32;
const MAX_PATH_BYTES: usize = 4096;

struct Record {
    index: usize,
    instance: usize,
    resource: usize,
    world: Mat4,
}

struct Collection {
    address: usize,
    generation: u64,
    records: Vec<Record>,
}

struct Registry {
    next: u64,
    closed: bool,
    collections: Vec<Collection>,
}

impl Registry {
    const fn new() -> Self {
        Self {
            next: 1,
            closed: false,
            collections: Vec::new(),
        }
    }

    fn retire(&mut self, address: usize) {
        self.collections.retain(|entry| entry.address != address);
    }

    fn register(&mut self, address: usize, records: Vec<Record>) -> Option<u64> {
        self.retire(address);
        if self.closed || records.is_empty() {
            return None;
        }
        let generation = self.next;
        self.next = self.next.checked_add(1)?;
        if self.collections.len() >= MAX_COLLECTIONS {
            self.collections.remove(0);
        }
        self.collections.push(Collection {
            address,
            generation,
            records,
        });
        Some(generation)
    }

    fn sources(&self, to_cockpit: &impl Fn(Vec3) -> Vec3) -> Vec<Source> {
        self.collections
            .iter()
            .flat_map(|entry| {
                entry.records.iter().filter_map(move |record| {
                    let centre = to_cockpit(record.world.w_axis.truncate());
                    centre.is_finite().then_some(Source {
                        identity: Identity {
                            registration: entry.generation,
                            provider: Provider::Collection {
                                collection: entry.address,
                                index: record.index,
                                resource: record.resource,
                            },
                            weapon: 0,
                            control: 0,
                            data: 0,
                            model: record.instance,
                        },
                        centre,
                    })
                })
            })
            .collect()
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());
static CLEANUP: AtomicBool = AtomicBool::new(false);
static ACTIVE_ENTRIES: AtomicUsize = AtomicUsize::new(0);

pub(super) struct Entry;

impl Entry {
    pub(super) fn enter() -> Self {
        ACTIVE_ENTRIES.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        ACTIVE_ENTRIES.fetch_sub(1, Ordering::Release);
    }
}

#[detour(address = jc3gi::location::Location::LoadModelCollection_ADDRESS)]
fn load_model_collection(location: *mut Location, data: *const u8, size: u32, name: *const u8) {
    let _entry = Entry::enter();
    // SAFETY: the native callback supplies the resource name and its bounded readable byte span.
    let metadata = unsafe { metadata(data, size, name) };
    LOAD_MODEL_COLLECTION
        .get()
        .unwrap()
        .call(location, data, size, name);
    let Some(metadata) = metadata else {
        return;
    };
    // SAFETY: the native load invocation keeps its location and completed collection live.
    let Some(location) = (unsafe { location.as_ref() }) else {
        return;
    };
    let mut registry = REGISTRY.lock();
    if registry.closed {
        return;
    }
    // SAFETY: no collection borrow or native instance pointer escapes this callback. Retirement
    // takes the same lock before native destruction, including while snapshots are copied.
    let records = unsafe { capture(location.m_ModelCollection, &metadata) };
    if records.is_empty() {
        tracing::debug!(target: "controllers", collection = location.m_ModelCollection as usize, expected_instances = metadata.count, "grenade: verified static dictionary has no qualifying live display instances");
    }
    if let Some(generation) = registry.register(location.m_ModelCollection as usize, records) {
        let entry = registry
            .collections
            .last()
            .expect("registration appends its collection");
        for record in &entry.records {
            tracing::debug!(target: "controllers", collection = entry.address, registration = generation, index = record.index, model = record.instance, resource = record.resource, "grenade: observed a verified static collection display source");
        }
    }
}

#[detour(address = jc3gi::graphics_engine::model_instance::ModelCollection::Destruct_ADDRESS)]
fn collection_destruct(collection: *mut ModelCollection) {
    let _entry = Entry::enter();
    REGISTRY.lock().retire(collection as usize);
    COLLECTION_DESTRUCT.get().unwrap().call(collection);
}

unsafe fn metadata(data: *const u8, size: u32, name: *const u8) -> Option<Dictionary> {
    if data.is_null() || name.is_null() || size as usize > MAX_BYTES {
        return None;
    }
    let mut owned_name = Vec::new();
    for index in 0..MAX_PATH_BYTES {
        let byte = unsafe { name.add(index).read() };
        if byte == 0 {
            let metadata = dictionary(&owned_name, unsafe {
                std::slice::from_raw_parts(data, size as usize)
            });
            if owned_name
                .windows(b"massos".len())
                .any(|part| part.eq_ignore_ascii_case(b"massos"))
            {
                tracing::debug!(target: "controllers", resource = %String::from_utf8_lossy(&owned_name), size, qualified = metadata.is_some(), "grenade: inspected a Massos model collection dictionary");
            }
            return metadata;
        }
        owned_name.push(byte);
    }
    None
}

unsafe fn capture(collection: *const ModelCollection, metadata: &Dictionary) -> Vec<Record> {
    let Some(collection) = (unsafe { collection.as_ref() }) else {
        return Vec::new();
    };
    if collection.m_NumInstances as usize != metadata.count
        || collection.m_ModelInstances.is_null()
        || !(collection.m_ModelInstances as usize)
            .is_multiple_of(std::mem::align_of::<*mut ModelInstance>())
    {
        return Vec::new();
    }
    let Some(manager) = (unsafe { ModelManager::get() }) else {
        return Vec::new();
    };
    metadata
        .grenades
        .iter()
        .filter_map(|index| {
            if *index >= metadata.count {
                return None;
            }
            let instance = unsafe { *collection.m_ModelInstances.add(*index) };
            let model = unsafe { instance.as_ref() }?;
            if !visible(model) || model.m_spModel.px == manager.m_FailModel.px {
                return None;
            }
            let mut matrix = Matrix4::default();
            unsafe { model.GetMatrix(&mut matrix) };
            let world = Mat4::from(matrix);
            world.is_finite().then_some(Record {
                index: *index,
                instance: instance as usize,
                resource: model.m_spModel.px as usize,
                world,
            })
        })
        .collect()
}

fn visible(model: &ModelInstance) -> bool {
    model
        .m_Flags
        .contains(ModelInstanceFlags::VISIBLE | ModelInstanceFlags::FLAG_ENABLED)
        && !model
            .m_Flags
            .intersects(ModelInstanceFlags::FLAG_DELETED | ModelInstanceFlags::DYNAMIC)
        && !model.m_spModel.px.is_null()
        && !model.m_spModel.pn.is_null()
        && !model.m_BFBC.is_null()
        && model.m_BFBC_id != u16::MAX
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::controllers::weapons::grenade::locker::{Refusal, complete, nearest};

    fn records() -> Vec<Record> {
        [0, 1]
            .map(|index| Record {
                index,
                instance: 100 + index,
                resource: 200,
                world: Mat4::from_translation(Vec3::X * index as f32),
            })
            .into()
    }

    #[test]
    fn retirement_and_address_reuse_invalidate_an_exact_offer() {
        let mut registry = Registry::new();
        let first = registry.register(1, records()).unwrap();
        let current = registry.sources(&|point| point);
        let offered = nearest(&current, Vec3::ZERO);
        registry.retire(1);
        assert_eq!(
            complete(
                offered,
                &registry.sources(&|point| point),
                Some(Vec3::ZERO),
                || { panic!("a retired collection cannot draw") }
            ),
            Err(Refusal::SourceChanged)
        );
        let replacement = registry.register(1, records()).unwrap();
        assert_ne!(first, replacement);
        assert_eq!(
            complete(
                offered,
                &registry.sources(&|point| point),
                Some(Vec3::ZERO),
                || { panic!("address reuse cannot draw") }
            ),
            Err(Refusal::SourceChanged)
        );
    }

    #[test]
    fn copied_live_pose_selects_each_hand_and_allows_independent_repeated_draws() {
        let mut registry = Registry::new();
        let mut data = records();
        data[0].world = Mat4::from_translation(Vec3::new(4.0, 2.0, 3.0));
        data[1].world = Mat4::from_translation(Vec3::new(5.0, 2.0, 3.0));
        registry.register(1, data).unwrap();
        let current = registry.sources(&|point| point - Vec3::new(4.0, 2.0, 3.0));
        let left = nearest(&current, Vec3::ZERO).unwrap();
        let right = nearest(&current, Vec3::X).unwrap();
        assert_ne!(left.identity, right.identity);
        let draws = Cell::new(0);
        for _ in 0..4 {
            for (offer, grip) in [(left, Vec3::ZERO), (right, Vec3::X)] {
                assert_eq!(
                    complete(Some(offer), &current, Some(grip), || {
                        draws.set(draws.get() + 1);
                        true
                    }),
                    Ok(())
                );
            }
        }
        assert_eq!(draws.get(), 8);
        assert_eq!(registry.sources(&|point| point).len(), 2);
        assert_eq!(registry.sources(&|_| Vec3::NAN).len(), 0);
    }

    #[test]
    fn completion_keeps_retirement_excluded_until_the_draw_returns() {
        let registry = Mutex::new(Registry::new());
        registry.lock().register(1, records()).unwrap();
        let current = registry.lock().sources(&|point| point);
        let offered = nearest(&current, Vec3::ZERO);
        assert_eq!(
            read_sources(&registry, &|point| point, |sources| {
                assert!(registry.try_lock().is_none());
                complete(offered, &sources, Some(Vec3::ZERO), || {
                    assert!(registry.try_lock().is_none());
                    true
                })
            }),
            Ok(())
        );
        assert!(registry.try_lock().is_some());
        registry.lock().retire(1);
        assert!(registry.lock().sources(&|point| point).is_empty());
    }

    #[test]
    fn registry_is_bounded_and_shutdown_does_not_accept_late_loads() {
        let mut registry = Registry::new();
        for address in 0..MAX_COLLECTIONS + 1 {
            registry.register(address, records()).unwrap();
        }
        assert_eq!(registry.collections.len(), MAX_COLLECTIONS);
        assert_eq!(registry.collections[0].address, 1);
        registry.closed = true;
        registry.collections.clear();
        assert!(registry.register(99, records()).is_none());
        assert!(registry.sources(&|point| point).is_empty());
    }

    #[test]
    fn static_instance_requires_live_in_world_visible_nondeleted_resource() {
        let mut model: ModelInstance = unsafe { std::mem::zeroed() };
        assert!(!visible(&model));
        model.m_Flags = ModelInstanceFlags::VISIBLE | ModelInstanceFlags::FLAG_ENABLED;
        model.m_spModel.px = std::ptr::dangling_mut();
        model.m_spModel.pn = std::ptr::dangling_mut();
        model.m_BFBC = std::ptr::dangling_mut();
        assert!(visible(&model));
        for flag in [
            ModelInstanceFlags::FLAG_DELETED,
            ModelInstanceFlags::DYNAMIC,
        ] {
            model.m_Flags.insert(flag);
            assert!(!visible(&model));
            model.m_Flags.remove(flag);
        }
        model.m_BFBC_id = u16::MAX;
        assert!(!visible(&model));
    }
}
