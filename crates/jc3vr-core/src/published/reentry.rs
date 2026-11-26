//! [`ReentryGuard`]: a thread-local cell set for the extent of a call.

use std::{cell::Cell, marker::PhantomData, thread::LocalKey};

/// Sets a thread-local [`Cell`] for its lifetime and restores the value it replaced when it drops,
/// so a nested scope and a panic unwinding out of the call both leave the cell as they found it.
/// A detour reads the cell to learn whether the mod's own call, or an outer engine call it cares
/// about, is on this thread's stack. Bound to its thread, since the cell it restores is that
/// thread's.
///
/// ```
/// # use std::cell::Cell;
/// # use jc3vr_core::published::ReentryGuard;
/// thread_local! {
///     static OWN: Cell<bool> = const { Cell::new(false) };
/// }
/// {
///     let _own = ReentryGuard::enter(&OWN, true);
///     assert!(OWN.get());
/// }
/// assert!(!OWN.get());
/// ```
#[must_use = "the cell is set only while the guard is held"]
pub struct ReentryGuard<T: Copy + 'static> {
    key: &'static LocalKey<Cell<T>>,
    previous: T,
    _thread_bound: PhantomData<*const ()>,
}

impl<T: Copy + 'static> ReentryGuard<T> {
    /// Set `key`'s cell to `value` until the guard drops.
    pub fn enter(key: &'static LocalKey<Cell<T>>, value: T) -> Self {
        Self {
            key,
            previous: key.replace(value),
            _thread_bound: PhantomData,
        }
    }

    /// The value the cell held before the guard set it: the outer scope's, or the default.
    pub fn previous(&self) -> T {
        self.previous
    }

    /// Restore the cell now, returning what it held at the exit: for a scope whose callees leave a
    /// result in the cell.
    pub fn exit(self) -> T {
        let value = self.key.replace(self.previous);
        std::mem::forget(self);
        value
    }
}

impl<T: Copy + 'static> Drop for ReentryGuard<T> {
    fn drop(&mut self) {
        self.key.set(self.previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        static FLAG: Cell<bool> = const { Cell::new(false) };
        static VALUE: Cell<Option<u32>> = const { Cell::new(None) };
    }

    #[test]
    fn nested_guards_restore_the_outer_value() {
        {
            let outer = ReentryGuard::enter(&VALUE, Some(1));
            assert_eq!(outer.previous(), None);
            {
                let inner = ReentryGuard::enter(&VALUE, Some(2));
                assert_eq!(inner.previous(), Some(1));
                assert_eq!(VALUE.get(), Some(2));
            }
            assert_eq!(VALUE.get(), Some(1));
        }
        assert_eq!(VALUE.get(), None);
    }

    #[test]
    fn exit_returns_what_the_callees_left() {
        let guard = ReentryGuard::enter(&VALUE, None);
        VALUE.set(Some(7));
        assert_eq!(guard.exit(), Some(7));
        assert_eq!(VALUE.get(), None);
    }

    #[test]
    fn a_panic_unwinding_out_restores_the_cell() {
        let result = std::panic::catch_unwind(|| {
            let _guard = ReentryGuard::enter(&FLAG, true);
            panic!("unwinding through the guard");
        });
        assert!(result.is_err());
        assert!(!FLAG.get());
    }

    #[test]
    fn the_cell_is_per_thread() {
        let _guard = ReentryGuard::enter(&FLAG, true);
        assert!(!std::thread::spawn(|| FLAG.get()).join().unwrap());
    }
}
