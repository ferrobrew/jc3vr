//! The weapon objects that are single pistols, readable from any thread without a lock.
//!
//! The set is a fixed table of addresses. The input tick adds and removes entries on the main
//! thread outside the world update, and the pickup's detour adds one inside the world update, from
//! the pickup's track message or a swimming character's pickup; the two never run at once, so
//! there is one writer at a time. The detours and the per-gun reads on the simulation's threads
//! only test membership. An entry
//! is claimed with a compare-and-swap, so two writers never take the same place; a reader that
//! races a removal sees the pistol as single for at most that read, which shows or fires its first
//! gun alone once more.

use std::sync::atomic::{AtomicUsize, Ordering};

/// How many single pistols the table holds: the slot's, the carried ones, one in a hand, and
/// those the player let go of that the VR inventory still tracks, with room to spare.
pub const CAPACITY: usize = 32;

/// Whether the weapon at `object` is a single pistol.
pub fn contains(object: usize) -> bool {
    object != 0
        && COUNT.load(Ordering::Relaxed) != 0
        && TABLE.iter().any(|e| e.load(Ordering::Relaxed) == object)
}

/// Add the weapon at `object`. Returns whether it was added: false when it is already in the set,
/// or when the table is full.
pub fn insert(object: usize) -> Result<bool, Full> {
    if object == 0 || contains(object) {
        return Ok(false);
    }
    for entry in &TABLE {
        if entry
            .compare_exchange(0, object, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            COUNT.fetch_add(1, Ordering::Relaxed);
            return Ok(true);
        }
    }
    Err(Full)
}

/// Remove the weapon at `object`. Returns whether it was in the set.
pub fn remove(object: usize) -> bool {
    if object == 0 {
        return false;
    }
    let mut removed = false;
    for entry in &TABLE {
        if entry
            .compare_exchange(object, 0, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            COUNT.fetch_sub(1, Ordering::Relaxed);
            removed = true;
        }
    }
    removed
}

/// Every weapon in the set.
pub fn all() -> Vec<usize> {
    TABLE
        .iter()
        .map(|e| e.load(Ordering::Relaxed))
        .filter(|&o| o != 0)
        .collect()
}

/// How many weapons the set holds.
pub fn len() -> usize {
    COUNT.load(Ordering::Relaxed)
}

/// The table is full: the pistol stays the game's pair.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Full;

impl std::fmt::Display for Full {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "single pistols: the table of {CAPACITY} is full")
    }
}

impl std::error::Error for Full {}

static TABLE: [AtomicUsize; CAPACITY] = [const { AtomicUsize::new(0) }; CAPACITY];
static COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    // The table is a process-wide static, so the tests use addresses no other test uses and remove
    // what they add.

    #[test]
    fn an_object_is_added_once_and_removed() {
        let object = 0x5100_0010;
        assert_eq!(insert(object), Ok(true));
        assert!(contains(object));
        assert_eq!(insert(object), Ok(false), "already a single pistol");
        assert!(all().contains(&object));
        assert!(remove(object));
        assert!(!contains(object));
        assert!(!remove(object));
    }

    #[test]
    fn the_null_object_is_never_a_member() {
        assert_eq!(insert(0), Ok(false));
        assert!(!contains(0));
        assert!(!remove(0));
    }
}
