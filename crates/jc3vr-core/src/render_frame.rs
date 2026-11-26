//! Per-frame values that the game thread prepares for a frame and the draw task reads while it
//! records that frame.
//!
//! With the render update overlapped (`vr.overlap_render_update`, `docs/mod/threading.md`), the game
//! thread prepares frame N+1 -- its located views, its head pose, its interpolation fraction, its
//! hands -- while the draw task is still recording frame N, so a value published as a single slot
//! would reach frame N's draws partway through. A [`RenderFrameSlot`] keeps the values of the two
//! frames that can be live at once, each tagged with the generation of the frame that wrote it:
//!
//! - [`begin_preparing`] opens a new generation at the top of the render update, before any slot is
//!   written for the frame;
//! - [`note_dispatch`] records, on the game thread at each dispatch (the engine's `Draw` prologue),
//!   which generation the dispatch draws;
//! - [`pin_dispatch`] pins that generation on the thread running the draw task for the extent of the
//!   task, so every read there answers with the dispatched frame's values while the game thread
//!   writes the next frame's.
//!
//! Every other reader (the game thread, the fragment workers running the render update's jobs, the
//! frame tail) reads the frame being prepared. Two entries suffice: a generation is opened only
//! after the previous frame's dispatch, and a dispatch is issued only once the frame before it has
//! finished drawing, so at most the dispatched frame and the one being prepared are live.

use std::{
    cell::Cell,
    sync::atomic::{AtomicU64, Ordering},
};

use parking_lot::Mutex;

/// A per-frame value: written for the frame being prepared, read for the frame the reader's thread
/// is drawing (see the module docs).
pub struct RenderFrameSlot<T: Copy> {
    entries: Mutex<[Option<(u64, T)>; 2]>,
}

impl<T: Copy> Default for RenderFrameSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy> RenderFrameSlot<T> {
    pub const fn new() -> Self {
        Self {
            entries: Mutex::new([None, None]),
        }
    }

    /// Write the value for the frame being prepared, replacing any written earlier for it.
    pub fn set(&self, value: T) {
        let generation = PREPARING.load(Ordering::Acquire);
        let mut entries = self.entries.lock();
        let index = index_for(&entries, generation);
        entries[index] = Some((generation, value));
    }

    /// Modify the value for the frame being prepared, starting from `initial` when nothing has been
    /// written for it yet.
    pub fn update(&self, initial: T, f: impl FnOnce(&mut T)) {
        let generation = PREPARING.load(Ordering::Acquire);
        let mut entries = self.entries.lock();
        let index = index_for(&entries, generation);
        let mut value = match entries[index] {
            Some((g, value)) if g == generation => value,
            _ => initial,
        };
        f(&mut value);
        entries[index] = Some((generation, value));
    }

    /// The value for the frame this thread reads: the dispatched frame on a pinned draw task, else
    /// the frame being prepared. The latest value written at or before that frame, so a frame that
    /// wrote nothing reads the one before it; `None` when nothing that old is held.
    pub fn get(&self) -> Option<T> {
        let generation = reader_generation();
        select(&self.entries.lock(), generation)
    }
}

/// Open the next frame's generation. Called once per frame on the game thread at the top of the
/// render update, before any slot is written for it.
pub fn begin_preparing() {
    PREPARING.fetch_add(1, Ordering::AcqRel);
}

/// Record that the dispatch being issued draws the frame being prepared. Called on the game thread
/// in the engine's `Draw` prologue, before the draw task is handed the dispatch.
pub fn note_dispatch() {
    DISPATCHED.store(PREPARING.load(Ordering::Acquire), Ordering::Release);
}

/// Pin the dispatched frame's generation on this thread until the returned guard drops. Held by
/// the draw task's detour for the extent of the task.
pub fn pin_dispatch() -> DispatchPin {
    DispatchPin(PINNED.replace(Some(DISPATCHED.load(Ordering::Acquire))))
}

/// Restores this thread's previous pin when dropped.
pub struct DispatchPin(Option<u64>);

impl Drop for DispatchPin {
    fn drop(&mut self) {
        PINNED.set(self.0);
    }
}

/// The generation of the frame being prepared.
static PREPARING: AtomicU64 = AtomicU64::new(1);
/// The generation the latest dispatch draws.
static DISPATCHED: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// The generation the draw task on this thread is recording, while it runs.
    static PINNED: Cell<Option<u64>> = const { Cell::new(None) };
}

fn reader_generation() -> u64 {
    PINNED
        .get()
        .unwrap_or_else(|| PREPARING.load(Ordering::Acquire))
}

/// The entry to write `generation` into: its own, else an empty one, else the older one.
fn index_for<T: Copy>(entries: &[Option<(u64, T)>; 2], generation: u64) -> usize {
    let held = |index: usize| entries[index].map(|(g, _)| g);
    if held(0) == Some(generation) {
        0
    } else if held(1) == Some(generation) {
        1
    } else if held(0).is_none() {
        0
    } else if held(1).is_none() {
        1
    } else if held(0) <= held(1) {
        0
    } else {
        1
    }
}

/// The latest entry written at or before `generation`.
fn select<T: Copy>(entries: &[Option<(u64, T)>; 2], generation: u64) -> Option<T> {
    entries
        .iter()
        .flatten()
        .filter(|(g, _)| *g <= generation)
        .max_by_key(|(g, _)| *g)
        .map(|(_, value)| *value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_takes_the_latest_entry_at_or_before_its_frame() {
        let entries = [Some((4, 'a')), Some((5, 'b'))];
        assert_eq!(select(&entries, 5), Some('b'));
        assert_eq!(select(&entries, 4), Some('a'));
        assert_eq!(select(&entries, 9), Some('b'));
        assert_eq!(select(&entries, 3), None);
        assert_eq!(select(&[Some((2, 'a')), None], 4), Some('a'));
    }

    #[test]
    fn a_write_replaces_its_own_frame_then_an_empty_entry_then_the_older_one() {
        assert_eq!(index_for(&[Some((5, 'a')), Some((4, 'b'))], 5), 0);
        assert_eq!(index_for(&[Some((4, 'a')), Some((5, 'b'))], 5), 1);
        assert_eq!(index_for(&[Some((4, 'a')), None], 5), 1);
        assert_eq!(index_for(&[Some((5, 'a')), Some((4, 'b'))], 6), 1);
        assert_eq!(index_for(&[Some((3, 'a')), Some((4, 'b'))], 5), 0);
    }

    /// The overlap end to end: the draw task pinned to frame N keeps reading N while the game thread
    /// prepares N+1, and the next dispatch moves the draw task to N+1.
    #[test]
    fn the_draw_task_reads_the_dispatched_frame_while_the_next_is_prepared() {
        static SLOT: RenderFrameSlot<u32> = RenderFrameSlot::new();
        begin_preparing();
        SLOT.set(10);
        note_dispatch();
        let draw_task = std::thread::spawn(|| {
            let _pin = pin_dispatch();
            let before = SLOT.get();
            std::thread::park();
            (before, SLOT.get())
        });
        begin_preparing();
        SLOT.update(0, |value| *value += 11);
        assert_eq!(SLOT.get(), Some(11));
        draw_task.thread().unpark();
        assert_eq!(draw_task.join().unwrap(), (Some(10), Some(10)));

        note_dispatch();
        let pinned = std::thread::spawn(|| {
            let _pin = pin_dispatch();
            SLOT.get()
        });
        assert_eq!(pinned.join().unwrap(), Some(11));
    }
}
