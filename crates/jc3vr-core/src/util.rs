use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use parking_lot::Mutex;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VIRTUAL_KEY};

pub fn is_pressed(key: VIRTUAL_KEY) -> bool {
    static LAST_INPUT: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    if LAST_INPUT
        .lock()
        .is_some_and(|last_input| last_input.elapsed() < std::time::Duration::from_millis(250))
    {
        return false;
    }

    let output = unsafe { GetAsyncKeyState(key.0 as _) != 0 };

    if output {
        *LAST_INPUT.lock() = Some(std::time::Instant::now());
    }

    output
}

/// A diagnostic counter bumped on the draw path, where a read-modify-write is dearer than it looks:
/// a locked add waits for the stores the Direct3D runtime has just made to drain, once per draw.
/// The draw path runs its calls one at a time under the engine's context mutex, so a plain load and
/// store counts every call; were two threads ever to bump one counter at once, a count could be lost,
/// which these counts tolerate.
pub(crate) trait DrawPathCounter {
    fn bump(&self);
}

impl DrawPathCounter for AtomicU64 {
    fn bump(&self) {
        self.store(
            self.load(Ordering::Relaxed).wrapping_add(1),
            Ordering::Relaxed,
        );
    }
}

impl DrawPathCounter for AtomicUsize {
    fn bump(&self) {
        self.store(
            self.load(Ordering::Relaxed).wrapping_add(1),
            Ordering::Relaxed,
        );
    }
}

impl DrawPathCounter for AtomicU32 {
    fn bump(&self) {
        self.store(
            self.load(Ordering::Relaxed).wrapping_add(1),
            Ordering::Relaxed,
        );
    }
}

/// Bump a [`DrawPathCounter`].
pub(crate) fn bump(counter: &impl DrawPathCounter) {
    counter.bump();
}
