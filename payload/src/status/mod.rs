//! The status lines: a read-only summary of whether each of the mod's systems is live and healthy,
//! for confirming a headset session at a glance. Each row reads one system's published state and
//! gives a state word ([`Health`]) and a few numbers. The debug UI's Status tab shows every row and
//! logs the summary ([`summary`]) on request, and the scenario runner's status-row probes read the
//! rows by stable IDs ([`read_row`]). Authored prose and opaque values are composed separately;
//! the UI reads display text, while logs and scenario probes retain their existing spelling.
//!
//! A row may be read on any thread, including in the middle of a frame while the game thread holds
//! the VR runtime lock, so the rows read only atomics and leaf locks that their modules publish for
//! any thread (see `docs/mod/threading.md`), never the runtime state, and they take one lock at a
//! time. A caller must not hold the configuration lock while it reads a row, since some rows read
//! the configuration.

pub(crate) mod body;
pub(crate) mod controllers;
pub(crate) mod explosives;
pub(crate) mod feedback;
pub(crate) mod fingers;
pub(crate) mod grapple;
pub(crate) mod grip;
pub(crate) mod hands;
pub(crate) mod inventory;
pub(crate) mod system;
mod text;
pub(crate) mod vehicles;
pub(crate) mod weapons;

use smol_str::SmolStr;

use crate::status::text::{Text, prose, text};

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

    pub(crate) const fn display_word(self) -> &'static str {
        match self {
            Health::Ok => "Ok",
            Health::Off => "Off",
            Health::Waiting => "Waiting",
            Health::Failed => "Failed",
        }
    }
}

/// A status row with separate text for the UI and the existing log and scenario contracts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) health: Health,
    pub(crate) detail: SmolStr,
    pub(crate) note: Option<String>,
    pub(crate) display_detail: SmolStr,
    pub(crate) display_note: Option<SmolStr>,
}

impl Line {
    fn new(health: Health, detail: impl Into<Text>) -> Self {
        let detail = detail.into();
        Self {
            health,
            detail: detail.legacy,
            display_detail: detail.display,
            note: None,
            display_note: None,
        }
    }

    /// Add a note without changing opaque values in either spelling.
    fn with_note(self, note: impl Into<Text>) -> Self {
        let note = note.into();
        Self {
            note: Some(note.legacy.to_string()),
            display_note: Some(note.display),
            ..self
        }
    }

    fn ok(detail: impl Into<Text>) -> Self {
        Self::new(Health::Ok, detail)
    }

    fn off(detail: impl Into<Text>) -> Self {
        Self::new(Health::Off, detail)
    }

    fn waiting(detail: impl Into<Text>) -> Self {
        Self::new(Health::Waiting, detail)
    }

    fn failed(detail: impl Into<Text>) -> Self {
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

/// Every row's label, top to bottom: the names the scenario runner's status-row probes use.
pub(crate) fn row_labels() -> impl Iterator<Item = &'static str> {
    SECTIONS
        .iter()
        .flat_map(|section| section.rows.iter().map(|row| row.label))
}

/// The line of the row labelled `label`, read now, or `None` when no row has that label. The rows
/// read only published slots, so any thread may ask.
pub(crate) fn read_row(label: &str) -> Option<Line> {
    SECTIONS
        .iter()
        .flat_map(|section| section.rows.iter())
        .find(|row| row.label == label)
        .map(|row| (row.read)())
}

/// Every section's stable ID, display title, and authored row labels and lines, top to bottom.
pub(crate) fn read_all()
-> impl Iterator<Item = (&'static str, &'static str, Vec<(&'static str, Line)>)> {
    SECTIONS.iter().map(|section| {
        let rows = section
            .rows
            .iter()
            .map(|row| (row.display_label, (row.read)()))
            .collect();
        (section.title, section.display_title, rows)
    })
}

/// Every row as `label: text`, sections separated by `|`, for the log.
pub(crate) fn summary() -> String {
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

/// A section of the summary: a header over its rows.
struct Section {
    title: &'static str,
    display_title: &'static str,
    rows: &'static [Row],
}

/// One row: its stable label, its display label, and how to read its line. Each section module
/// owns its rows as constants, so a feature page shows a row through the same formatter as the
/// Status feature.
pub(crate) struct Row {
    label: &'static str,
    display_label: &'static str,
    read: fn() -> Line,
}

impl Row {
    const fn new(label: &'static str, display_label: &'static str, read: fn() -> Line) -> Self {
        Self {
            label,
            display_label,
            read,
        }
    }

    /// The row's authored UI label.
    pub(crate) fn display_label(&self) -> &'static str {
        self.display_label
    }

    /// The row's line, read now.
    pub(crate) fn read(&self) -> Line {
        (self.read)()
    }
}

/// The sections, top to bottom.
const SECTIONS: &[Section] = &[
    Section {
        title: "SAVED SETTINGS",
        display_title: "Saved settings",
        rows: &[system::SETTINGS_FILE],
    },
    Section {
        title: "RENDERING",
        display_title: "Rendering",
        rows: &[
            system::SESSION,
            system::DIRECT_BLIT,
            system::FRAME_TIME,
            system::RENDER_SIZE,
            body::RENDER_RATE_BODY,
        ],
    },
    Section {
        title: "INPUT",
        display_title: "Input",
        rows: &[
            system::INPUT_ACTIONS,
            system::LAYOUT_FINGERPRINT,
            system::SELF_CHECK,
        ],
    },
    Section {
        title: "HUD",
        display_title: "HUD",
        rows: &[system::ATLAS, system::WORLD_MARKERS, system::MARKER_DEPTH],
    },
    Section {
        title: "GUN IN HAND",
        display_title: "Gun in hand",
        rows: &[
            controllers::RIGID_GRIP,
            controllers::CANONICAL_GRIP,
            controllers::FLIP_WATCH,
            controllers::ANIMATED_DEVIATION,
            controllers::BONE_CHANGES,
        ],
    },
    Section {
        title: "WEAPONS IN HAND",
        display_title: "Weapons in hand",
        rows: &[
            weapons::LEFT_HAND,
            weapons::RIGHT_HAND,
            weapons::LEFT_HAND_ON_GUN,
            weapons::RIGHT_HAND_ON_GUN,
            weapons::TAKES,
            weapons::RACKS,
            weapons::LEFT_ACTION,
            weapons::RIGHT_ACTION,
            weapons::ARMED,
            weapons::RIGHT_GRIP,
            weapons::DOCKS,
            weapons::LAST_REFUSAL,
        ],
    },
    Section {
        title: "INVENTORY",
        display_title: "Inventory",
        rows: &[
            inventory::TRACKED,
            inventory::CARRIED,
            inventory::SEPARATE_PISTOLS,
            inventory::SIDECAR,
            inventory::LAST_LOAD,
            inventory::PINNED,
            inventory::LOADOUT,
        ],
    },
    Section {
        title: "EXPLOSIVES AND MAGAZINES",
        display_title: "Explosives and magazines",
        rows: &[
            explosives::LIVE_GRENADES,
            explosives::C4_BY_HAND,
            explosives::MAGAZINES,
        ],
    },
    Section {
        title: "AIMING",
        display_title: "Aiming",
        rows: &[explosives::DOWN_THE_SIGHTS, explosives::GUN_AGAINST_WALLS],
    },
    Section {
        title: "HAND BODIES",
        display_title: "Hand bodies",
        rows: &[
            controllers::CALLBACK,
            controllers::CALLBACK_FIRES,
            controllers::LEFT_BODY,
            controllers::RIGHT_BODY,
            controllers::BODY_ERRORS,
            controllers::CONTACT_EVENTS,
            controllers::BLOWS,
            controllers::JOINTS,
            controllers::COLLIDER_VIEW,
            controllers::LEFT_GUN_BODY,
            controllers::RIGHT_GUN_BODY,
        ],
    },
    Section {
        title: "GRIP",
        display_title: "Grip",
        rows: &[grip::LEFT_GRIP, grip::RIGHT_GRIP, grip::MARKERS],
    },
    Section {
        title: "HANDS IN THE WORLD",
        display_title: "Hands in the world",
        rows: &[
            hands::PALMS,
            hands::DOORS,
            hands::CHARACTERS,
            hands::MOVING_CLIMBS,
            hands::STUNT_POSITIONS,
            hands::PUSH_UP,
            hands::ARTICULATED,
        ],
    },
    Section {
        title: "FEEDBACK",
        display_title: "Feedback",
        rows: &[feedback::HAPTIC_CUES, feedback::IMPACT_SOUNDS],
    },
    Section {
        title: "FINGERS",
        display_title: "Fingers",
        rows: &[fingers::LEFT_GRIP, fingers::RIGHT_GRIP],
    },
    Section {
        title: "GRAPPLE",
        display_title: "Grapple",
        rows: &[
            grapple::STATE,
            grapple::TARGET,
            grapple::REEL,
            grapple::TETHERS,
            grapple::LOCK,
            grapple::SEAT,
            grapple::SWING,
            grapple::SNATCH,
            grapple::HAPTICS,
        ],
    },
    Section {
        title: "VEHICLES",
        display_title: "Vehicles",
        rows: &[
            vehicles::FLIGHT_CONTROLS,
            vehicles::COCKPIT_BUTTONS,
            vehicles::PASSENGER_AIM,
            vehicles::MOUNTED_GUN,
        ],
    },
    Section {
        title: "LOCOMOTION",
        display_title: "Locomotion",
        rows: &[
            controllers::STICK_VELOCITY,
            controllers::ACT_FILTER,
            controllers::ACT_WATCHDOG,
            controllers::STEADY_VIEW,
            controllers::NEUTRAL_HEAD,
        ],
    },
    Section {
        title: "PROCEDURAL BODY",
        display_title: "Procedural body",
        rows: &[
            controllers::PROCEDURAL_MODE,
            controllers::HAND_OVER,
            controllers::TWIST_BONES,
            body::SEATED_TORSO,
            body::WINGSUIT_ARMS,
            body::PARACHUTE_BODY,
            body::SWIM_TORSO,
            body::ROOF_TORSO,
            body::BODY_MIRROR,
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read_as_word_then_detail() {
        assert_eq!(Line::ok("2 EYES").text(), "OK, 2 EYES");
        assert_eq!(Line::off("").text(), "OFF");
        assert_eq!(Line::failed("SEE LOG").text(), "FAILED, SEE LOG");
    }

    #[test]
    fn display_text_leaves_opaque_details_and_notes_unchanged() {
        for value in [
            "123ABCDE",
            "DEADBEEF",
            "/FOO",
            "SLOT_ID",
            "MixedCaseName",
            "OpenXR",
            "11.1 ms P50/P95",
        ] {
            let line = Line::failed(text!("Gun's shape not read: ", value))
                .with_note(text!("Another save's sidecar: ", value));
            assert_eq!(
                line.text(),
                format!("FAILED, GUN'S SHAPE NOT READ: {value}")
            );
            assert_eq!(
                line.display_detail,
                format!("Gun's shape not read: {value}")
            );
            assert_eq!(
                line.note.as_deref(),
                Some(format!("ANOTHER SAVE'S SIDECAR: {value}").as_str())
            );
            assert_eq!(
                line.display_note.as_deref(),
                Some(format!("Another save's sidecar: {value}").as_str())
            );
        }
    }

    #[test]
    fn display_labels_and_health_do_not_replace_machine_spelling() {
        let session = SECTIONS
            .iter()
            .flat_map(|section| section.rows)
            .find(|row| row.label == "VR SESSION")
            .unwrap();
        assert_eq!(session.display_label, "VR session");
        assert!(row_labels().any(|label| label == "C4 BY HAND"));
        assert!(!row_labels().any(|label| label == "C4 by hand"));
        for (health, legacy, display) in [
            (Health::Ok, "OK", "Ok"),
            (Health::Off, "OFF", "Off"),
            (Health::Waiting, "WAITING", "Waiting"),
            (Health::Failed, "FAILED", "Failed"),
        ] {
            assert_eq!(health.word(), legacy);
            assert_eq!(health.display_word(), display);
        }
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
}
