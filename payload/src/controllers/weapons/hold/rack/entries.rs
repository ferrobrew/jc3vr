//! The weapons a character's interaction entries offer, as a swimming character's pickup finds
//! them: an entry's own pickup interaction offers its context's weapon, and an entry whose node
//! stands for a weapon placement rule ([`WeaponPlacementRule`]) offers the weapon the rule spawned.
//! The engine side is in `docs/engine/gameplay/weapon-equip-and-drop.md`, "Racks".
//!
//! One walk serves every reader of the entries: the hands' rack take
//! ([`crate::controllers::weapons::hold::rack`]), the not-carried condition's detour
//! ([`crate::controllers::weapons::hold::rack::offer`]), and the grenade locker sources
//! ([`crate::controllers::weapons::grenade`]).

use jc3gi::{
    equipment::weapon::WeaponBase,
    interaction::{InteractionGraphEntry, InteractionGraphEntryArray},
    weapon_placement_rule::WeaponPlacementRule,
};

use crate::controllers::weapons::hold::refs::Strong;

/// Walk the weapons `entries` offer, each held alive by a strong reference, until `visit` returns
/// true. An array that is null, reversed, or not a whole number of entries is not walked, and at
/// most [`MAX_ENTRIES`] entries are visited.
///
/// # Safety
///
/// `entries` must be a live entry array of an interaction user proxy, read on a thread where its
/// entries stay live for the walk.
pub(crate) unsafe fn walk(
    entries: &InteractionGraphEntryArray,
    mut visit: impl FnMut(Strong) -> bool,
) {
    let begin = entries.m_Begin;
    let Some(count) = entry_count(begin as usize, entries.m_End as usize) else {
        return;
    };
    for index in 0..count.min(MAX_ENTRIES) {
        // SAFETY: the index is inside the proxy's live entry array.
        let entry = unsafe { *begin.add(index) };
        if let Some(weapon) = unsafe { offered(entry) }
            && visit(weapon)
        {
            return;
        }
    }
}

/// The most entries a walk visits, a bound against a corrupt array.
const MAX_ENTRIES: usize = 256;

/// The entries between `begin` and `end`, or `None` for an array that is null, reversed, or not a
/// whole number of entries.
fn entry_count(begin: usize, end: usize) -> Option<usize> {
    let stride = std::mem::size_of::<*mut InteractionGraphEntry>();
    let bytes = end.checked_sub(begin).filter(|_| begin != 0)?;
    bytes.is_multiple_of(stride).then_some(bytes / stride)
}

/// The weapon `entry` offers, held alive: its context's own weapon, or the weapon its entry node's
/// placement rule spawned.
unsafe fn offered(entry: *mut InteractionGraphEntry) -> Option<Strong> {
    let entry = unsafe { entry.as_ref() }?;
    let graph = unsafe { entry.m_Graph.as_mut() }?;
    if let Some(context) = unsafe { graph.m_Context.as_ref() }
        && let Some(parent) = unsafe { Strong::lock(&context.m_Parent) }
        && unsafe { (*parent.object()).IsType(WeaponBase::TYPE_ID()) }
    {
        return Some(parent);
    }
    let node = unsafe { graph.GetNodeByIndex(entry.m_EntryNodeIndex).as_ref() }?;
    let object = unsafe { Strong::lock(&node.m_GameObject) }?;
    if !unsafe { (*object.object()).IsType(WeaponPlacementRule::TYPE_ID()) } {
        return None;
    }
    // A rule's `CGameObject` subobject is at its start.
    let rule = object.object() as *const WeaponPlacementRule;
    let spawned = unsafe {
        Strong::filled(|out| {
            (*rule).GetSpawnedObject(out, WeaponBase::TYPE_ID(), 0);
        })
    }?;
    unsafe { (*spawned.object()).IsType(WeaponBase::TYPE_ID()) }.then_some(spawned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIDE: usize = std::mem::size_of::<*mut InteractionGraphEntry>();

    #[test]
    fn a_whole_array_counts_its_entries() {
        assert_eq!(entry_count(0x1000, 0x1000), Some(0));
        assert_eq!(entry_count(0x1000, 0x1000 + 3 * STRIDE), Some(3));
    }

    #[test]
    fn a_null_reversed_or_ragged_array_is_not_walked() {
        assert_eq!(entry_count(0, 3 * STRIDE), None);
        assert_eq!(entry_count(0x1000 + STRIDE, 0x1000), None);
        assert_eq!(entry_count(0x1000, 0x1000 + STRIDE + 1), None);
    }
}
