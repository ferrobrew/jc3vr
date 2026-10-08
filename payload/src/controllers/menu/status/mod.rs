//! The `VR: STATUS` page: a read-only summary of whether each of the mod's systems is live and
//! healthy, for confirming a headset session at a glance. Each row reads one system's published
//! state and shows a state word ([`Health`]) and a few numbers; the rows refresh every frame while
//! the page is shown, and the page logs the same summary once each time it opens.
//!
//! The rows run on the UI capture thread in the middle of a frame, while the game thread holds the
//! VR runtime lock and waits on that thread, so they read only atomics and leaf locks that their
//! modules publish for any thread (see `docs/mod/threading.md`), never the runtime state, and they
//! take one lock at a time.

mod controllers;
mod system;

use std::sync::atomic::{AtomicBool, Ordering};

use smol_str::SmolStr;

use crate::pause_menu::model::page::{Page, PageDef, Readout};

/// The `VR: STATUS` page.
pub(crate) const PAGE: PageDef = PageDef {
    tab: "STATUS",
    build,
    on_close: Some(on_close),
    on_frame: None,
};

/// A row's state word.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Health {
    /// Live and doing what it should.
    Ok,
    /// Switched off in the settings, or not used in the current setup.
    Off,
    /// On, but not live yet: waiting for a session, the player, or a measurement.
    Waiting,
    /// On, and broken.
    Failed,
}

impl Health {
    pub(crate) const fn word(self) -> &'static str {
        match self {
            Health::Ok => "OK",
            Health::Off => "OFF",
            Health::Waiting => "WAITING",
            Health::Failed => "FAILED",
        }
    }
}

/// What a row shows: its state word and a short detail, and for the log, a longer note the row
/// has no room for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) health: Health,
    pub(crate) detail: SmolStr,
    pub(crate) note: Option<String>,
}

impl Line {
    pub(crate) fn new(health: Health, detail: impl Into<SmolStr>) -> Self {
        Self {
            health,
            detail: detail.into(),
            note: None,
        }
    }

    /// The line with `note` added to its logged text.
    pub(crate) fn with_note(self, note: impl Into<String>) -> Self {
        Self {
            note: Some(note.into()),
            ..self
        }
    }

    pub(crate) fn ok(detail: impl Into<SmolStr>) -> Self {
        Self::new(Health::Ok, detail)
    }

    pub(crate) fn off(detail: impl Into<SmolStr>) -> Self {
        Self::new(Health::Off, detail)
    }

    pub(crate) fn waiting(detail: impl Into<SmolStr>) -> Self {
        Self::new(Health::Waiting, detail)
    }

    pub(crate) fn failed(detail: impl Into<SmolStr>) -> Self {
        Self::new(Health::Failed, detail)
    }

    /// The row's text: the word, then the detail after a comma when there is one.
    pub(crate) fn text(&self) -> SmolStr {
        if self.detail.is_empty() {
            self.health.word().into()
        } else {
            smol_str::format_smolstr!("{}, {}", self.health.word(), self.detail)
        }
    }
}

/// A section of the page: a header over its rows.
struct Section {
    title: &'static str,
    rows: &'static [Row],
}

/// One row: its label, how to read its line, and the same as the readout's text.
struct Row {
    label: &'static str,
    read: fn() -> Line,
    show: fn() -> SmolStr,
}

/// A [`Row`] reading the line `$read` returns.
macro_rules! row {
    ($label:literal, $read:path) => {
        Row {
            label: $label,
            read: $read,
            show: || $read().text(),
        }
    };
}

/// The page's sections, top to bottom.
const SECTIONS: &[Section] = &[
    Section {
        title: "SAVED SETTINGS",
        rows: &[row!("SETTINGS FILE", system::settings_file)],
    },
    Section {
        title: "RENDERING",
        rows: &[
            row!("VR SESSION", system::session),
            row!("DIRECT EYE BLIT", system::direct_blit),
            row!("FRAME TIME", system::frame_time),
            row!("RENDER SIZE", system::render_size),
        ],
    },
    Section {
        title: "INPUT",
        rows: &[
            row!("CONTROLLER ACTIONS", system::input_actions),
            row!("LAYOUT FINGERPRINT", system::layout_fingerprint),
            row!("SELF-CHECK", system::self_check),
        ],
    },
    Section {
        title: "HUD",
        rows: &[
            row!("ATLAS PARTS", system::atlas),
            row!("WORLD MARKERS", system::world_markers),
            row!("MARKER DEPTH", system::marker_depth),
        ],
    },
    Section {
        title: "GUN IN HAND",
        rows: &[
            row!("RIGID GRIP", controllers::rigid_grip),
            row!("CANONICAL GRIP", controllers::canonical_grip),
            row!("FLIP WATCH", controllers::flip_watch),
            row!("ANIMATED GRIP OFFSET", controllers::animated_deviation),
            row!("ATTACH BONE CHANGES", controllers::bone_changes),
        ],
    },
    Section {
        title: "HAND BODIES",
        rows: &[
            row!("CALLBACK", controllers::callback),
            row!("CALLBACK FIRES", controllers::callback_fires),
            row!("LEFT BODY", controllers::left_body),
            row!("RIGHT BODY", controllers::right_body),
            row!("BODY ERRORS", controllers::body_errors),
            row!("CONTACT EVENTS", controllers::contact_events),
            row!("BLOWS DEALT", controllers::blows),
            row!("GRAB JOINTS", controllers::joints),
        ],
    },
    Section {
        title: "LOCOMOTION",
        rows: &[
            row!("STICK VELOCITY", controllers::stick_velocity),
            row!("ACT FILTER", controllers::act_filter),
            row!("ACT WATCHDOG", controllers::act_watchdog),
            row!("STEADY VIEW", controllers::steady_view),
            row!("NEUTRAL HEAD", controllers::neutral_head),
        ],
    },
    Section {
        title: "PROCEDURAL BODY",
        rows: &[
            row!("MODE", controllers::procedural_mode),
            row!("PASS-THROUGH ERROR", controllers::pass_through),
            row!("HAND-OVER CASE", controllers::hand_over),
            row!("GROUND CASTS", controllers::ground_casts),
            row!("STANCE", controllers::stance),
        ],
    },
];

fn build() -> Page {
    if !LOGGED.swap(true, Ordering::Relaxed) {
        tracing::info!("status: {}", summary());
    }
    let mut page = Page::new("VR: STATUS");
    for section in SECTIONS {
        page = page.header(section.title);
        for row in section.rows {
            page = page.item(Readout {
                label: row.label.into(),
                read: row.show,
            });
        }
    }
    page
}

/// Let the next opening log the summary again.
fn on_close() {
    LOGGED.store(false, Ordering::Relaxed);
}

/// Whether this stay on the page has logged its summary. The page is rebuilt while it is shown
/// (the game repopulating the settings), which must not log it again.
static LOGGED: AtomicBool = AtomicBool::new(false);

/// Every row as `label: text`, sections separated by `|`.
fn summary() -> String {
    SECTIONS
        .iter()
        .map(|section| {
            let rows = section
                .rows
                .iter()
                .map(|row| {
                    let line = (row.read)();
                    match &line.note {
                        Some(note) => format!("{}: {} ({note})", row.label, line.text()),
                        None => format!("{}: {}", row.label, line.text()),
                    }
                })
                .collect::<Vec<_>>()
                .join("; ");
            format!("{}: {rows}", section.title)
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pause_menu::model::page::Item;

    #[test]
    fn lines_read_as_word_then_detail() {
        assert_eq!(Line::ok("2 EYES").text(), "OK, 2 EYES");
        assert_eq!(Line::off("").text(), "OFF");
        assert_eq!(Line::failed("SEE LOG").text(), "FAILED, SEE LOG");
    }

    #[test]
    fn every_section_has_rows_and_labels_are_distinct() {
        let mut labels = Vec::new();
        for section in SECTIONS {
            assert!(!section.rows.is_empty(), "{}", section.title);
            labels.extend(section.rows.iter().map(|row| row.label));
        }
        let count = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), count);
    }

    #[test]
    fn the_page_is_read_only() {
        let page = build();
        assert!(
            page.items
                .iter()
                .all(|item| matches!(item, Item::Header(_) | Item::Readout(_)))
        );
        assert_eq!(page.reset_to_defaults(), 0);
    }
}
