//! The page model: what a VR settings page shows, described as data in Rust beside the settings it
//! edits. The renderer turns each [`Item`] into one of the pause movie's own settings rows, so
//! adding a setting never touches the movie.
//!
//! Items reach their state through plain function pointers (non-capturing closures coerce to them),
//! so a page definition is a static description and the settings stay owned by their modules: a
//! toggle's `get` and `set` lock whatever config they belong to. The functions run on the UI
//! capture thread, inside the pause menu's hooks, and must not call back into the movie.

use smol_str::SmolStr;

use crate::pause_menu::model::slider::{SliderFormat, SliderRange, slider_label};

/// A registered VR page: its tab under the VR entry and how to build its contents.
#[derive(Copy, Clone)]
pub(crate) struct PageDef {
    /// The tab's label on the settings screen's tab bar. Plain text: the movie treats a label
    /// beginning with `&` or `#` as a localisation key.
    pub(crate) tab: &'static str,
    /// Builds the page's current contents. Called when the page opens and again whenever it is
    /// rebuilt (a step-by-step page changed step, the page was reset, or the game repopulated the
    /// settings).
    pub(crate) build: fn() -> Page,
    /// Called once when the page's stay ends, whichever way: the player switching to another tab
    /// or leaving the settings screen, the menu deactivating or unloading, or the page failing to
    /// reinstall. Saves what the
    /// page changed and resets any state the page keeps between frames. It may run off the capture
    /// thread (a deactivation requested from another thread), so it must not call into the movie.
    pub(crate) on_close: Option<fn()>,
    /// Called every frame while the page is shown, before its live values are written, for work
    /// that runs over time (a calibration's capture). Returning
    /// [`ButtonOutcome::Rebuild`] rebuilds the page, as a button can.
    pub(crate) on_frame: Option<fn() -> ButtonOutcome>,
}

/// One page's contents: the panel title and its rows, top to bottom.
#[derive(Clone)]
pub(crate) struct Page {
    pub(crate) title: SmolStr,
    pub(crate) items: Vec<Item>,
}

impl Page {
    /// An empty page titled `title`.
    pub(crate) fn new(title: impl Into<SmolStr>) -> Self {
        Self {
            title: title.into(),
            items: Vec::new(),
        }
    }

    /// Append `item`.
    pub(crate) fn item(mut self, item: impl Into<Item>) -> Self {
        self.items.push(item.into());
        self
    }

    /// Append a section header: a title over the rows that follow it, which the highlight passes
    /// over.
    pub(crate) fn header(self, title: impl Into<SmolStr>) -> Self {
        self.item(Item::Header(title.into()))
    }

    /// Append a line of text. The movie draws one line per row, upper-cased, so longer
    /// instructions are split over several text rows.
    pub(crate) fn text(self, text: impl Into<SmolStr>) -> Self {
        self.item(Item::Text(text.into()))
    }

    /// Whether row `index` is a section header.
    pub(crate) fn is_header(&self, index: usize) -> bool {
        matches!(self.items.get(index), Some(Item::Header(_)))
    }

    /// The index of the first button, which a rebuilt page focuses.
    pub(crate) fn first_button(&self) -> Option<usize> {
        self.items
            .iter()
            .position(|item| matches!(item, Item::Button(_)))
    }

    /// Reset every value item to its default. Returns how many were reset.
    pub(crate) fn reset_to_defaults(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.reset_to_default())
            .count()
    }
}

/// One row of a page.
#[derive(Clone)]
pub(crate) enum Item {
    /// A section header: a title over the rows that follow, with nothing to select.
    Header(SmolStr),
    /// A line of explanatory text.
    Text(SmolStr),
    /// An on/off setting.
    Toggle(Toggle),
    /// A choice among a fixed list of options.
    Choice(Choice),
    /// A number within a range.
    Slider(Slider),
    /// A read-only value, refreshed every frame while the page is shown.
    Readout(Readout),
    /// An action.
    Button(Button),
}

/// An on/off setting, shown as the movie's option picker with `OFF` and `ON`.
#[derive(Clone)]
pub(crate) struct Toggle {
    pub(crate) label: SmolStr,
    pub(crate) get: fn() -> bool,
    pub(crate) set: fn(bool),
    pub(crate) default: bool,
}

/// A choice among `options`, by index.
#[derive(Clone)]
pub(crate) struct Choice {
    pub(crate) label: SmolStr,
    pub(crate) options: &'static [&'static str],
    pub(crate) get: fn() -> usize,
    pub(crate) set: fn(usize),
    pub(crate) default: usize,
}

/// A number within `range`, shown as the movie's slider with the value written into its label.
#[derive(Clone)]
pub(crate) struct Slider {
    pub(crate) label: SmolStr,
    pub(crate) range: SliderRange,
    pub(crate) format: SliderFormat,
    pub(crate) get: fn() -> f32,
    pub(crate) set: fn(f32),
    pub(crate) default: f32,
}

/// A read-only value: `label` on the left, `read()` on the right.
#[derive(Clone)]
pub(crate) struct Readout {
    pub(crate) label: SmolStr,
    pub(crate) read: fn() -> SmolStr,
}

/// An action: `label` on the left, `caption` on the right; selecting the row runs `action`.
#[derive(Clone)]
pub(crate) struct Button {
    pub(crate) label: SmolStr,
    pub(crate) caption: SmolStr,
    pub(crate) action: fn() -> ButtonOutcome,
}

/// What a button's action asks of the page.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum ButtonOutcome {
    /// Keep the rows as they are.
    Stay,
    /// Rebuild the page (a step-by-step page moved to another step, or the action changed what the
    /// page lists).
    Rebuild,
}

/// The result of applying a value the movie sent for a row.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Applied {
    /// The item took the value.
    Set,
    /// The item holds no value (a header, text, readout, or button); the movie's reset-all sends
    /// every row's default, these included.
    NotAValue,
    /// The value is outside what the item allows; nothing changed.
    OutOfRange,
}

impl Item {
    /// Apply the integer `value` the movie sent for this item's row.
    pub(crate) fn apply(&self, value: i32) -> Applied {
        match self {
            Item::Toggle(toggle) => match value {
                0 | 1 => {
                    (toggle.set)(value == 1);
                    Applied::Set
                }
                _ => Applied::OutOfRange,
            },
            Item::Choice(choice) => match usize::try_from(value) {
                Ok(index) if index < choice.options.len() => {
                    (choice.set)(index);
                    Applied::Set
                }
                _ => Applied::OutOfRange,
            },
            Item::Slider(slider) => {
                if !(0..=slider.range.max_notch()).contains(&value) {
                    return Applied::OutOfRange;
                }
                (slider.set)(slider.range.value_at(value));
                Applied::Set
            }
            Item::Header(_) | Item::Text(_) | Item::Readout(_) | Item::Button(_) => {
                Applied::NotAValue
            }
        }
    }

    /// Reset a value item to its default. Returns whether this is a value item.
    pub(crate) fn reset_to_default(&self) -> bool {
        match self {
            Item::Toggle(toggle) => (toggle.set)(toggle.default),
            Item::Choice(choice) => (choice.set)(choice.default),
            Item::Slider(slider) => (slider.set)(slider.default),
            Item::Header(_) | Item::Text(_) | Item::Readout(_) | Item::Button(_) => return false,
        }
        true
    }

    /// The text this item's row shows that the movie does not keep up to date, and where: a
    /// readout's value, a slider's label with its current value, or a button's caption (which the
    /// list replaces with `...` when the button is selected, expecting a key capture). `None` for
    /// items whose text the movie draws itself.
    pub(crate) fn live_text(&self) -> Option<LiveText> {
        match self {
            Item::Readout(readout) => Some(LiveText {
                field: LiveField::KeyText,
                text: (readout.read)(),
            }),
            Item::Button(button) => Some(LiveText {
                field: LiveField::KeyText,
                text: button.caption.clone(),
            }),
            Item::Slider(slider) => Some(LiveText {
                field: LiveField::Name,
                text: slider_label(&slider.label, slider.format, (slider.get)()),
            }),
            Item::Header(_) | Item::Text(_) | Item::Toggle(_) | Item::Choice(_) => None,
        }
    }
}

/// A row's changing text and the field that shows it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct LiveText {
    pub(crate) field: LiveField,
    pub(crate) text: SmolStr,
}

/// Which of a row's text fields a [`LiveText`] belongs to.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum LiveField {
    /// The row's label (`m_Name`, drawn on the left).
    Name,
    /// The key-format row's right-hand text (`m_KeyText`).
    KeyText,
}

impl From<Toggle> for Item {
    fn from(value: Toggle) -> Self {
        Item::Toggle(value)
    }
}

impl From<Choice> for Item {
    fn from(value: Choice) -> Self {
        Item::Choice(value)
    }
}

impl From<Slider> for Item {
    fn from(value: Slider) -> Self {
        Item::Slider(value)
    }
}

impl From<Readout> for Item {
    fn from(value: Readout) -> Self {
        Item::Readout(value)
    }
}

impl From<Button> for Item {
    fn from(value: Button) -> Self {
        Item::Button(value)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

    use super::*;

    static FLAG: AtomicBool = AtomicBool::new(false);
    static PICK: AtomicUsize = AtomicUsize::new(0);
    static LEVEL: AtomicU32 = AtomicU32::new(0);

    fn level() -> f32 {
        f32::from_bits(LEVEL.load(Ordering::Relaxed))
    }

    fn set_level(value: f32) {
        LEVEL.store(value.to_bits(), Ordering::Relaxed);
    }

    fn toggle() -> Item {
        Item::Toggle(Toggle {
            label: "Flag".into(),
            get: || FLAG.load(Ordering::Relaxed),
            set: |v| FLAG.store(v, Ordering::Relaxed),
            default: true,
        })
    }

    fn choice() -> Item {
        Item::Choice(Choice {
            label: "Pick".into(),
            options: &["A", "B", "C"],
            get: || PICK.load(Ordering::Relaxed),
            set: |v| PICK.store(v, Ordering::Relaxed),
            default: 2,
        })
    }

    fn slider() -> Item {
        Item::Slider(Slider {
            label: "Level".into(),
            range: SliderRange::new(0.0, 2.0, 20),
            format: SliderFormat::new(1, ""),
            get: level,
            set: set_level,
            default: 1.0,
        })
    }

    // The statics are shared, so every assertion on them lives in this one test.
    #[test]
    fn applies_values_and_defaults() {
        let toggle = toggle();
        assert_eq!(toggle.apply(1), Applied::Set);
        assert!(FLAG.load(Ordering::Relaxed));
        assert_eq!(toggle.apply(0), Applied::Set);
        assert!(!FLAG.load(Ordering::Relaxed));
        assert_eq!(toggle.apply(2), Applied::OutOfRange);
        assert_eq!(toggle.apply(-1), Applied::OutOfRange);
        assert!(!FLAG.load(Ordering::Relaxed));

        let choice = choice();
        assert_eq!(choice.apply(1), Applied::Set);
        assert_eq!(PICK.load(Ordering::Relaxed), 1);
        assert_eq!(choice.apply(3), Applied::OutOfRange);
        assert_eq!(choice.apply(-2), Applied::OutOfRange);
        assert_eq!(PICK.load(Ordering::Relaxed), 1);

        let slider = slider();
        assert_eq!(slider.apply(5), Applied::Set);
        assert_eq!(level(), 0.5);
        assert_eq!(slider.apply(21), Applied::OutOfRange);
        assert_eq!(level(), 0.5);
        assert_eq!(
            slider.live_text(),
            Some(LiveText {
                field: LiveField::Name,
                text: "Level  0.5".into()
            })
        );

        let page = Page::new("Test")
            .header("Section")
            .item(toggle)
            .item(choice)
            .item(slider)
            .text("Line");
        assert_eq!(page.reset_to_defaults(), 3);
        assert!(FLAG.load(Ordering::Relaxed));
        assert_eq!(PICK.load(Ordering::Relaxed), 2);
        assert_eq!(level(), 1.0);
    }

    #[test]
    fn non_value_items_ignore_values() {
        let readout = Item::Readout(Readout {
            label: "Now".into(),
            read: || "12".into(),
        });
        let button = Item::Button(Button {
            label: "Go".into(),
            caption: "PRESS".into(),
            action: || ButtonOutcome::Stay,
        });
        for item in [
            Item::Header("H".into()),
            Item::Text("T".into()),
            readout.clone(),
            button,
        ] {
            assert_eq!(item.apply(0), Applied::NotAValue);
            assert!(!item.reset_to_default());
        }
        assert_eq!(
            readout.live_text(),
            Some(LiveText {
                field: LiveField::KeyText,
                text: "12".into()
            })
        );
    }

    #[test]
    fn finds_the_first_button() {
        let button = Button {
            label: "Go".into(),
            caption: "PRESS".into(),
            action: || ButtonOutcome::Rebuild,
        };
        let page = Page::new("Flow")
            .text("Step")
            .item(button.clone())
            .item(button);
        assert_eq!(page.first_button(), Some(1));
        assert_eq!(Page::new("Empty").text("x").first_button(), None);
    }

    #[test]
    fn finds_headers() {
        let page = Page::new("Sections").header("A").text("x").header("B");
        assert!(page.is_header(0));
        assert!(!page.is_header(1));
        assert!(page.is_header(2));
        assert!(!page.is_header(3));
    }
}
