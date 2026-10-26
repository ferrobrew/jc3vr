//! Read loaded static display sources from the location streaming system's owned list.

use jc3gi::{
    graphics_engine::model_instance::{ModelCollection, ModelInstance, ModelManager},
    hash::hashlittle,
    location::{GameLocationSystemContext, Location, LocationInfo, LocationType},
};

use crate::controllers::weapons::grenade::locker::{
    collection::{MAX_PATH_BYTES, Observation, capture, path, visible},
    format::{Dictionary, grenade_model, loaded_location},
};

/// Copy qualifying sources while the game thread owns a stable loaded-location list.
///
/// # Safety
///
/// The caller runs outside the world update on the game thread and excludes collection retirement.
pub(super) unsafe fn observe() -> Vec<Observation> {
    let Some(context) = (unsafe { GameLocationSystemContext::get() }) else {
        return Vec::new();
    };
    let locations = &context.m_LoadedLocations;
    let Some(count) = count(locations.begin, locations.end, locations.capacity_end) else {
        return Vec::new();
    };
    let Some(manager) = (unsafe { ModelManager::get() }) else {
        return Vec::new();
    };
    let grenade_hash = hashlittle(grenade_model()) as u32;
    (0..count)
        .filter_map(|index| {
            let info = unsafe { (*locations.begin.add(index)).as_ref() }?;
            let location = unsafe { info.m_Location.as_ref() }?;
            unsafe { location_source(location, manager, grenade_hash) }
        })
        .collect()
}

const MAX_LOADED_LOCATIONS: usize = 4096;
const MAX_GRENADES: usize = 16;

fn count(
    begin: *const *mut LocationInfo,
    end: *const *mut LocationInfo,
    capacity: *const *mut LocationInfo,
) -> Option<usize> {
    let stride = std::mem::size_of::<*mut LocationInfo>();
    let bytes = (end as usize).checked_sub(begin as usize)?;
    let allocated = (capacity as usize).checked_sub(begin as usize)?;
    (!begin.is_null()
        && (begin as usize).is_multiple_of(std::mem::align_of::<*mut LocationInfo>())
        && bytes.is_multiple_of(stride)
        && allocated.is_multiple_of(stride)
        && bytes <= allocated
        && bytes / stride <= MAX_LOADED_LOCATIONS)
        .then_some(bytes / stride)
}

unsafe fn location_source(
    location: &Location,
    manager: &ModelManager,
    grenade_hash: u32,
) -> Option<Observation> {
    if location.m_LocationType != LocationType::Near {
        return None;
    }
    let name = unsafe { path(location.m_LocationFilename, MAX_PATH_BYTES) }?;
    let count = loaded_location(&name)?;
    let collection = unsafe { location.m_ModelCollection.as_ref() }?;
    let metadata = unsafe { live_metadata(collection, count, grenade_hash) }?;
    let records = unsafe { capture(location.m_ModelCollection, &metadata, manager) };
    Some(Observation {
        address: location.m_ModelCollection as usize,
        records,
    })
}

unsafe fn live_metadata(
    collection: &ModelCollection,
    count: usize,
    grenade_hash: u32,
) -> Option<Dictionary> {
    if collection.m_NumInstances as usize != count
        || collection.m_ModelInstances.is_null()
        || !(collection.m_ModelInstances as usize)
            .is_multiple_of(std::mem::align_of::<*mut ModelInstance>())
    {
        return None;
    }
    let mut grenades = Vec::new();
    for index in 0..count {
        let Some(instance) = (unsafe { (*collection.m_ModelInstances.add(index)).as_ref() }) else {
            continue;
        };
        if !visible(instance) {
            continue;
        }
        let Some(model) = (unsafe { instance.m_spModel.as_ref() }) else {
            continue;
        };
        if model.m_Id.m_Hash == grenade_hash {
            if grenades.len() == MAX_GRENADES {
                return None;
            }
            grenades.push(index);
        }
    }
    Some(Dictionary { count, grenades })
}

#[cfg(test)]
mod tests {
    use jc3gi::graphics_engine::model_instance::{Model, ModelInstanceFlags};

    use super::*;

    #[test]
    fn live_dictionary_selects_only_the_verified_resource_hash_at_its_current_index() {
        let mut resource: Model = unsafe { std::mem::zeroed() };
        resource.m_Id.m_Hash = 7;
        let mut instance: ModelInstance = unsafe { std::mem::zeroed() };
        instance.m_Flags = ModelInstanceFlags::VISIBLE | ModelInstanceFlags::FLAG_ENABLED;
        instance.m_spModel.px = &mut resource;
        instance.m_spModel.pn = std::ptr::dangling_mut();
        instance.m_BFBC = std::ptr::dangling_mut();
        let mut instances = [std::ptr::null_mut(), std::ptr::addr_of_mut!(instance)];
        let mut collection: ModelCollection = unsafe { std::mem::zeroed() };
        collection.m_NumInstances = instances.len() as u32;
        collection.m_ModelInstances = instances.as_mut_ptr();
        let metadata = unsafe { live_metadata(&collection, 2, 7) }.unwrap();
        assert_eq!(metadata.grenades, [1]);
        assert!(
            unsafe { live_metadata(&collection, 2, 8) }
                .unwrap()
                .grenades
                .is_empty()
        );
        assert!(unsafe { live_metadata(&collection, 3, 7) }.is_none());
        instance.m_Flags.insert(ModelInstanceFlags::FLAG_DELETED);
        assert!(
            unsafe { live_metadata(&collection, 2, 7) }
                .unwrap()
                .grenades
                .is_empty()
        );
    }

    #[test]
    fn loaded_list_rejects_invalid_bounds_before_reading_native_entries() {
        let mut entries = [std::ptr::null_mut::<LocationInfo>(); 2];
        let begin = entries.as_mut_ptr();
        let end = unsafe { begin.add(entries.len()) };
        assert_eq!(count(begin, end, end), Some(2));
        assert_eq!(count(end, begin, end), None);
        assert_eq!(count(begin, end, begin), None);
        assert_eq!(
            count(std::ptr::null(), std::ptr::null(), std::ptr::null()),
            None
        );
        assert_eq!(
            count(begin, (begin as usize + 1) as *const *mut LocationInfo, end),
            None
        );
    }
}
