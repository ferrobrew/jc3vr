//! Row ids: the integer every VR row carries as its `m_Type` (and, for buttons, `m_Action`), which
//! the pause movie sends back with a changed value or a selected button.
//!
//! The ids live in a band of their own, far above the game's own setting types, so the command
//! detour can tell a VR row's command from a native one by the id alone and never let a VR id reach
//! the settings manager or the key capture. An id encodes the page and the item index on that page,
//! so a command for a page that is no longer shown is recognised and dropped.

/// The first id of the band. Chosen far above every `SettingType` and every key-binding action the
/// movie sends, and positive, since the list manager reserves negative values for its own context
/// menu actions.
const ID_BASE: i32 = 0x0A00_0000;

/// The number of ids reserved per page (the most items a page can hold).
const PAGE_STRIDE: usize = 0x1_0000;

/// The number of pages the band holds.
const PAGE_LIMIT: usize = 0x100;

/// A VR row's identity: which registered page it belongs to and its index in that page's items.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct RowId {
    pub(crate) page: usize,
    pub(crate) item: usize,
}

impl RowId {
    /// The row id for `item` on `page`.
    pub(crate) fn new(page: usize, item: usize) -> Self {
        Self { page, item }
    }

    /// The row's integer id, or `None` when the page or item index does not fit the band.
    pub(crate) fn encode(self) -> Option<i32> {
        if self.page >= PAGE_LIMIT || self.item >= PAGE_STRIDE {
            return None;
        }
        let offset = self.page * PAGE_STRIDE + self.item;
        Some(ID_BASE + offset as i32)
    }

    /// The row an id names, or `None` when the id is outside the band (a native setting or action).
    pub(crate) fn decode(raw: i32) -> Option<Self> {
        if !in_band(raw) {
            return None;
        }
        let offset = (raw - ID_BASE) as usize;
        Some(Self {
            page: offset / PAGE_STRIDE,
            item: offset % PAGE_STRIDE,
        })
    }
}

/// Whether `raw` is a VR row id.
pub(crate) fn in_band(raw: i32) -> bool {
    raw >= ID_BASE && ((raw - ID_BASE) as usize) < PAGE_LIMIT * PAGE_STRIDE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_corner() {
        for (page, item) in [
            (0, 0),
            (0, 1),
            (1, 0),
            (3, 17),
            (PAGE_LIMIT - 1, PAGE_STRIDE - 1),
        ] {
            let id = RowId::new(page, item);
            let raw = id.encode().expect("in range");
            assert!(in_band(raw));
            assert_eq!(RowId::decode(raw), Some(id));
        }
    }

    #[test]
    fn distinct_rows_get_distinct_ids() {
        let a = RowId::new(0, PAGE_STRIDE - 1).encode().unwrap();
        let b = RowId::new(1, 0).encode().unwrap();
        assert_ne!(a, b);
        assert_eq!(b, a + 1);
    }

    #[test]
    fn out_of_range_rows_do_not_encode() {
        assert_eq!(RowId::new(PAGE_LIMIT, 0).encode(), None);
        assert_eq!(RowId::new(0, PAGE_STRIDE).encode(), None);
    }

    #[test]
    fn native_ids_are_outside_the_band() {
        // The game's setting types and key actions are small non-negative integers, and the list
        // manager's context-menu codes are small negatives.
        for raw in [-5, -1, 0, 1, 42, 56, 1000, 0x0100_0000, ID_BASE - 1] {
            assert!(!in_band(raw), "{raw} must not be in the band");
            assert_eq!(RowId::decode(raw), None);
        }
        let past_end = ID_BASE + (PAGE_LIMIT * PAGE_STRIDE) as i32;
        assert!(!in_band(past_end));
        assert!(!in_band(i32::MAX));
        assert!(!in_band(i32::MIN));
    }
}
