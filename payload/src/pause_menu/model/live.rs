//! Live values: the per-frame diff that decides which visible rows need their text rewritten.
//!
//! The settings list shows a scrolling window of the page on a fixed set of row clips. Each frame,
//! the runtime reads which item each clip shows and what text that item should show now; this
//! module remembers what was last written per clip and asks for a write only where the item or its
//! text changed. Keying by clip rather than by item means a row scrolled into a clip is rewritten
//! even when its text has not changed since it was last shown, because the clip's own redraw fills
//! it from the row object and the runtime writes the row object and the clip together.

use smol_str::SmolStr;

/// What the runtime last wrote to each row clip, as `(item index, text)`.
#[derive(Clone, Debug, Default)]
pub(crate) struct LiveSlots {
    slots: Vec<Option<(usize, SmolStr)>>,
}

impl LiveSlots {
    /// No writes recorded.
    pub(crate) const fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// Whether clip `slot`, now showing `item`, needs `text` written; records the write when it
    /// does.
    pub(crate) fn needs_write(&mut self, slot: usize, item: usize, text: &SmolStr) -> bool {
        if self.slots.len() <= slot {
            self.slots.resize(slot + 1, None);
        }
        let entry = &mut self.slots[slot];
        match entry {
            Some((shown, written)) if *shown == item && written == text => false,
            _ => {
                *entry = Some((item, text.clone()));
                true
            }
        }
    }

    /// Record that clip `slot` shows nothing live (empty, or an item with fixed text), so the next
    /// live item it shows is written.
    pub(crate) fn clear_slot(&mut self, slot: usize) {
        if let Some(entry) = self.slots.get_mut(slot) {
            *entry = None;
        }
    }

    /// Forget every write, so the next frame rewrites every live row. Used after anything that may
    /// have redrawn rows behind the diff's back: a changed value (the list redraws the row from
    /// its object), a selected button (the list replaces its text), or a rebuilt page.
    pub(crate) fn invalidate(&mut self) {
        self.slots.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_once_until_the_text_changes() {
        let mut slots = LiveSlots::default();
        let a: SmolStr = "1.80 M".into();
        let b: SmolStr = "1.81 M".into();
        assert!(slots.needs_write(3, 7, &a));
        assert!(!slots.needs_write(3, 7, &a));
        assert!(slots.needs_write(3, 7, &b));
        assert!(!slots.needs_write(3, 7, &b));
    }

    #[test]
    fn a_different_item_in_the_clip_is_rewritten() {
        let mut slots = LiveSlots::default();
        let text: SmolStr = "same".into();
        assert!(slots.needs_write(0, 1, &text));
        // Scrolling moved another item with identical text into the clip: its row object may
        // hold stale text, so the write still happens.
        assert!(slots.needs_write(0, 2, &text));
        assert!(slots.needs_write(0, 1, &text));
    }

    #[test]
    fn clips_are_independent() {
        let mut slots = LiveSlots::default();
        let text: SmolStr = "x".into();
        assert!(slots.needs_write(0, 4, &text));
        assert!(slots.needs_write(12, 4, &text));
        assert!(!slots.needs_write(0, 4, &text));
    }

    #[test]
    fn clearing_and_invalidating_force_writes() {
        let mut slots = LiveSlots::default();
        let text: SmolStr = "x".into();
        assert!(slots.needs_write(1, 1, &text));
        slots.clear_slot(1);
        assert!(slots.needs_write(1, 1, &text));
        slots.clear_slot(40);
        slots.invalidate();
        assert!(slots.needs_write(1, 1, &text));
    }
}
