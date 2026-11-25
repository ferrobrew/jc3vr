//! Section headers: how a header row is drawn and kept out of the selection.
//!
//! The settings list has no header row. A header is drawn on one of the list's own row clips with
//! its label in the type style of the list's title, and the list's highlight is moved off it, so it
//! reads as a title over its group rather than as a setting. This module holds the two decisions
//! behind that: which way to move the highlight off a header ([`skip_header`]), and which row clips
//! need their label style changed ([`HeaderStyles`]).

/// Which way to move the list's highlight.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Step {
    /// To the next row (the list's `HighlightNext`).
    Next,
    /// To the previous row (the list's `HighlightPrev`).
    Prev,
}

/// Where to move the highlight when it is on row `selected` (an index into the page's rows) and
/// was on `previous` the last time it was looked at; `None` when `selected` is not a header, or no
/// row in either direction can take the highlight.
///
/// The highlight continues the way it was going, so moving down past a header lands on the row
/// below it and moving up lands on the row above. A header with nothing selectable that way (the
/// first row of a page, reached by moving up or by the list opening on it) sends the highlight
/// the other way.
pub(crate) fn skip_header(
    selected: usize,
    previous: Option<usize>,
    is_header: impl Fn(usize) -> bool,
    rows: usize,
) -> Option<Step> {
    if selected >= rows || !is_header(selected) {
        return None;
    }
    let selectable_below = (selected + 1..rows).any(|row| !is_header(row));
    let selectable_above = (0..selected).any(|row| !is_header(row));
    let moving_up = previous.is_some_and(|previous| previous > selected);
    match (moving_up, selectable_above, selectable_below) {
        (true, true, _) | (false, true, false) => Some(Step::Prev),
        (_, _, true) => Some(Step::Next),
        (_, false, false) => None,
    }
}

/// The label style each row clip was last given, so a clip's style is changed only when the kind
/// of row it shows changes (the list reuses its clips as it scrolls).
#[derive(Clone, Debug, Default)]
pub(crate) struct HeaderStyles {
    /// Per clip: `Some(true)` when styled as a header, `Some(false)` when restored to the row
    /// style, `None` when untouched since the movie loaded.
    clips: Vec<Option<bool>>,
}

impl HeaderStyles {
    /// No clip touched.
    pub(crate) const fn new() -> Self {
        Self { clips: Vec::new() }
    }

    /// Whether clip `slot`, now showing a header when `header` is set, needs its style changed to
    /// match. An untouched clip has the row style, so only a header needs a change there.
    pub(crate) fn needs_change(&self, slot: usize, header: bool) -> bool {
        let current = self.clips.get(slot).copied().flatten().unwrap_or(false);
        current != header
    }

    /// Record that clip `slot` now has the header style when `header` is set, the row style
    /// otherwise.
    pub(crate) fn changed(&mut self, slot: usize, header: bool) {
        if self.clips.len() <= slot {
            self.clips.resize(slot + 1, None);
        }
        self.clips[slot] = Some(header);
    }

    /// The clips that have the header style, which must be restored before a native page uses
    /// them.
    pub(crate) fn styled(&self) -> impl Iterator<Item = usize> + '_ {
        self.clips
            .iter()
            .enumerate()
            .filter_map(|(slot, header)| (*header == Some(true)).then_some(slot))
    }

    /// Forget every clip (the movie that owned them unloaded).
    pub(crate) fn forget(&mut self) {
        self.clips.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page whose headers are at `headers`.
    fn page(headers: &'static [usize]) -> impl Fn(usize) -> bool {
        move |row| headers.contains(&row)
    }

    #[test]
    fn a_setting_row_is_left_alone() {
        assert_eq!(skip_header(1, Some(0), page(&[0, 4]), 8), None);
        assert_eq!(skip_header(9, Some(0), page(&[0, 4]), 8), None);
    }

    #[test]
    fn the_highlight_keeps_its_direction() {
        let rows = page(&[0, 4]);
        assert_eq!(skip_header(4, Some(3), &rows, 8), Some(Step::Next));
        assert_eq!(skip_header(4, Some(5), &rows, 8), Some(Step::Prev));
    }

    #[test]
    fn the_first_header_sends_the_highlight_down() {
        let rows = page(&[0, 4]);
        // The list opening on its first row.
        assert_eq!(skip_header(0, None, &rows, 8), Some(Step::Next));
        // Moving up onto the first row.
        assert_eq!(skip_header(0, Some(1), &rows, 8), Some(Step::Next));
    }

    #[test]
    fn a_last_header_sends_the_highlight_up() {
        let rows = page(&[0, 7]);
        assert_eq!(skip_header(7, Some(6), &rows, 8), Some(Step::Prev));
    }

    #[test]
    fn consecutive_headers_are_crossed_one_at_a_time() {
        let rows = page(&[2, 3]);
        assert_eq!(skip_header(2, Some(1), &rows, 6), Some(Step::Next));
        assert_eq!(skip_header(3, Some(2), &rows, 6), Some(Step::Next));
        assert_eq!(skip_header(3, Some(4), &rows, 6), Some(Step::Prev));
        assert_eq!(skip_header(2, Some(3), &rows, 6), Some(Step::Prev));
    }

    #[test]
    fn a_page_of_headers_leaves_the_highlight() {
        assert_eq!(skip_header(1, Some(0), page(&[0, 1, 2]), 3), None);
    }

    #[test]
    fn styles_change_only_on_a_change_of_kind() {
        let mut styles = HeaderStyles::new();
        assert!(!styles.needs_change(3, false));
        assert!(styles.needs_change(3, true));
        styles.changed(3, true);
        assert!(!styles.needs_change(3, true));
        assert!(styles.needs_change(3, false));
        assert_eq!(styles.styled().collect::<Vec<_>>(), vec![3]);
        styles.changed(3, false);
        assert!(!styles.needs_change(3, false));
        assert_eq!(styles.styled().count(), 0);
        styles.changed(1, true);
        styles.forget();
        assert!(styles.needs_change(1, true));
        assert_eq!(styles.styled().count(), 0);
    }
}
