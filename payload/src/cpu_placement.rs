//! Keeping the game's threads on the cores that share the largest L3 cache.
//!
//! On a processor whose cores do not all share one L3 of one size, such as a two-die part with
//! stacked cache on one die, the draw task's cost depends on where it runs. The draw task is one
//! thread recording every draw of a frame through the Direct3D runtime, and its time goes on the
//! runtime's bookkeeping per state change: reference counts on the bound resources, its command
//! chunks, and the engine's own per-call locks, a working set that misses the smaller cache and is
//! shared with the runtime's command-stream thread. On the die with the larger cache it runs in
//! little more than half the time it takes where the scheduler places it
//! (`docs/mod/performance/frame-budget.md`, "Where the draw task's threads run").
//!
//! While `vr.large_cache_cores` is on, this narrows the process's affinity to the cores of the
//! largest L3, when the L3 caches differ in size, and puts the affinity it found back when the
//! switch goes off and on eject. Threads created afterwards, the runtime's among them, inherit the
//! narrowed affinity. A processor whose L3 caches are all one size, or which has one, is left alone:
//! there the choice of cores is the scheduler's to make.

use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use windows::Win32::System::{
    SystemInformation::{
        GetLogicalProcessorInformationEx, RelationCache, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    },
    Threading::{GetCurrentProcess, GetProcessAffinityMask, SetProcessAffinityMask},
};

use crate::config::Config;

/// Apply or release the placement to match `vr.large_cache_cores`. Game thread, once per frame;
/// does nothing while the placement already matches the switch.
pub(crate) fn update() {
    let wanted = Config::lock_query(|c| c.vr.large_cache_cores);
    let mut state = STATE.lock();
    match (wanted, state.applied) {
        (true, None) if !state.declined => apply(&mut state),
        (false, Some(_)) => release(&mut state),
        _ => {}
    }
}

/// Put the affinity the placement found back, for eject.
pub(crate) fn restore() {
    release(&mut STATE.lock());
}

/// The cores the placement narrowed the process to, if it did.
pub(crate) fn applied_mask() -> Option<usize> {
    STATE.lock().applied.map(|applied| applied.mask)
}

/// One L3 cache: its size in bytes and the logical processors that share it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct L3 {
    size: u32,
    mask: usize,
}

/// The logical processors of the largest L3 caches, within `process_mask`, when the L3 caches are
/// not all one size; `None` when they are, when there is one, or when narrowing to them would leave
/// the process with no processor or with all of the ones it has.
fn large_cache_mask(caches: &[L3], process_mask: usize) -> Option<usize> {
    let largest = caches.iter().map(|cache| cache.size).max()?;
    if caches.iter().all(|cache| cache.size == largest) {
        return None;
    }
    let mask = caches
        .iter()
        .filter(|cache| cache.size == largest)
        .fold(0, |mask, cache| mask | cache.mask)
        & process_mask;
    (mask != 0 && mask != process_mask).then_some(mask)
}

#[derive(Clone, Copy)]
struct Applied {
    mask: usize,
    original: usize,
}

struct State {
    applied: Option<Applied>,
    /// The processor offered no different-sized L3 caches, or the placement failed; not retried
    /// until the switch is turned off and on again.
    declined: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    applied: None,
    declined: false,
});
/// Whether eject has been asked to put the affinity back, which is asked once.
static CLEANUP_REGISTERED: AtomicBool = AtomicBool::new(false);

fn apply(state: &mut State) {
    let caches = l3_caches();
    let mut original = 0usize;
    let mut system = 0usize;
    // SAFETY: the pseudo-handle of the current process, and two out-parameters.
    if let Err(error) =
        unsafe { GetProcessAffinityMask(GetCurrentProcess(), &mut original, &mut system) }
    {
        tracing::warn!(%error, "cpu placement: the process affinity could not be read");
        state.declined = true;
        return;
    }
    let Some(mask) = large_cache_mask(&caches, original) else {
        tracing::info!(
            ?caches,
            original = format_args!("{original:#x}"),
            "cpu placement: the L3 caches are one size, so the threads stay where the scheduler puts them"
        );
        state.declined = true;
        return;
    };
    // SAFETY: the pseudo-handle of the current process and a subset of its own affinity.
    match unsafe { SetProcessAffinityMask(GetCurrentProcess(), mask) } {
        Ok(()) => {
            tracing::info!(
                mask = format_args!("{mask:#x}"),
                original = format_args!("{original:#x}"),
                "cpu placement: the game's threads now run on the cores of the largest L3 cache"
            );
            state.applied = Some(Applied { mask, original });
            if !CLEANUP_REGISTERED.swap(true, Ordering::Relaxed) {
                crate::lifecycle::on_cleanup(|_| restore());
            }
        }
        Err(error) => {
            tracing::warn!(%error, "cpu placement: the process affinity could not be set");
            state.declined = true;
        }
    }
}

fn release(state: &mut State) {
    state.declined = false;
    let Some(applied) = state.applied.take() else {
        return;
    };
    // SAFETY: the pseudo-handle of the current process and the affinity it had before.
    if let Err(error) = unsafe { SetProcessAffinityMask(GetCurrentProcess(), applied.original) } {
        tracing::warn!(%error, "cpu placement: the original process affinity could not be restored");
    }
}

/// Every L3 cache of processor group 0, from the system's processor topology.
fn l3_caches() -> Vec<L3> {
    let mut length = 0u32;
    // SAFETY: a size query with no buffer, which fails with the length it needs.
    let _ = unsafe { GetLogicalProcessorInformationEx(RelationCache, None, &mut length) };
    if length == 0 {
        return Vec::new();
    }
    // Eight-byte aligned, as the records it holds are.
    let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
    let records = buffer
        .as_mut_ptr()
        .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>();
    // SAFETY: `buffer` holds at least `length` bytes.
    if unsafe { GetLogicalProcessorInformationEx(RelationCache, Some(records), &mut length) }
        .is_err()
    {
        return Vec::new();
    }
    let mut caches = Vec::new();
    let mut offset = 0usize;
    while offset + std::mem::size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>() <= length as usize
    {
        // SAFETY: `offset` is the start of a record the call wrote, within `length` bytes.
        let record = unsafe {
            &*buffer
                .as_ptr()
                .cast::<u8>()
                .add(offset)
                .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()
        };
        if record.Size == 0 {
            break;
        }
        if record.Relationship == RelationCache {
            // SAFETY: a cache record's union holds its cache relationship.
            let cache = unsafe { record.Anonymous.Cache };
            // SAFETY: a cache relationship's first group mask is always present.
            let group = unsafe { cache.Anonymous.GroupMask };
            if cache.Level == 3 && group.Group == 0 {
                caches.push(L3 {
                    size: cache.CacheSize,
                    mask: group.Mask,
                });
            }
        }
        offset += record.Size as usize;
    }
    caches
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u32 = 1 << 20;

    #[test]
    fn a_two_die_part_with_stacked_cache_narrows_to_the_large_die() {
        let caches = [
            L3 {
                size: 96 * MIB,
                mask: 0x00ff_00ff,
            },
            L3 {
                size: 32 * MIB,
                mask: 0xff00_ff00,
            },
        ];
        assert_eq!(large_cache_mask(&caches, 0xffff_ffff), Some(0x00ff_00ff));
        assert_eq!(large_cache_mask(&caches, 0x0000_ffff), Some(0x0000_00ff));
    }

    #[test]
    fn equal_or_single_caches_are_left_to_the_scheduler() {
        let equal = [
            L3 {
                size: 32 * MIB,
                mask: 0x00ff_00ff,
            },
            L3 {
                size: 32 * MIB,
                mask: 0xff00_ff00,
            },
        ];
        assert_eq!(large_cache_mask(&equal, 0xffff_ffff), None);
        let single = [L3 {
            size: 96 * MIB,
            mask: 0xffff,
        }];
        assert_eq!(large_cache_mask(&single, 0xffff), None);
        assert_eq!(large_cache_mask(&[], 0xffff), None);
    }

    #[test]
    fn a_process_already_off_the_large_die_is_not_emptied() {
        let caches = [
            L3 {
                size: 96 * MIB,
                mask: 0x00ff,
            },
            L3 {
                size: 32 * MIB,
                mask: 0xff00,
            },
        ];
        assert_eq!(large_cache_mask(&caches, 0xff00), None);
        assert_eq!(large_cache_mask(&caches, 0x00ff), None);
    }
}
