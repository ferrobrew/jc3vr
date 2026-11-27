//! The status lines: a read-only summary of whether each of the mod's systems is live and healthy,
//! for confirming a headset session at a glance. Each row reads one system's published state and
//! gives a state word ([`Health`]) and a few numbers. The debug UI's Status tab shows every row and
//! logs the summary ([`summary`]) on request, both in the same spelling.
//!
//! A row may be read on any thread, including in the middle of a frame while the game thread holds
//! the VR runtime lock, so the rows read only atomics and leaf locks that their modules publish for
//! any thread (see `docs/mod/threading.md`), never the runtime state, and they take one lock at a
//! time. A caller must not hold the configuration lock while it reads a row, since some rows read
//! the configuration.

pub(crate) mod body;
pub(crate) mod capture;
pub(crate) mod controllers;
pub(crate) mod explosives;
pub(crate) mod feedback;
pub(crate) mod fingers;
pub(crate) mod grapple;
pub(crate) mod grip;
pub(crate) mod hands;
pub(crate) mod inventory;
pub(crate) mod system;
pub(crate) mod vehicles;
pub(crate) mod weapons;

use smol_str::{SmolStr, format_smolstr};

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
            Health::Ok => "Ok",
            Health::Off => "Off",
            Health::Waiting => "Waiting",
            Health::Failed => "Failed",
        }
    }
}

/// A status row's state word, its short detail, and a longer note the detail has no room for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) health: Health,
    pub(crate) detail: SmolStr,
    pub(crate) note: Option<SmolStr>,
}

impl Line {
    fn new(health: Health, detail: impl Into<SmolStr>) -> Self {
        Self {
            health,
            detail: detail.into(),
            note: None,
        }
    }

    fn with_note(self, note: impl Into<SmolStr>) -> Self {
        Self {
            note: Some(note.into()),
            ..self
        }
    }

    fn ok(detail: impl Into<SmolStr>) -> Self {
        Self::new(Health::Ok, detail)
    }

    fn off(detail: impl Into<SmolStr>) -> Self {
        Self::new(Health::Off, detail)
    }

    fn waiting(detail: impl Into<SmolStr>) -> Self {
        Self::new(Health::Waiting, detail)
    }

    fn failed(detail: impl Into<SmolStr>) -> Self {
        Self::new(Health::Failed, detail)
    }

    /// The row's text: the word, then the detail after a comma when there is one.
    pub(crate) fn text(&self) -> SmolStr {
        if self.detail.is_empty() {
            self.health.word().into()
        } else {
            format_smolstr!("{}, {}", self.health.word(), self.detail)
        }
    }
}

/// Every section's title and its rows' labels and lines, top to bottom.
pub(crate) fn read_all() -> impl Iterator<Item = (&'static str, Vec<(&'static str, Line)>)> {
    SECTIONS.iter().map(|section| {
        let rows = section
            .rows
            .iter()
            .map(|row| (row.label, row.read()))
            .collect();
        (section.title, rows)
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
                    let line = row.read();
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
    rows: &'static [Row],
}

/// One row: its label and how to read its line. Each section module owns its rows as constants,
/// so a feature page shows a row through the same formatter as the Status feature.
pub(crate) struct Row {
    label: &'static str,
    read: fn() -> Line,
}

impl Row {
    const fn new(label: &'static str, read: fn() -> Line) -> Self {
        Self { label, read }
    }

    pub(crate) fn label(&self) -> &'static str {
        self.label
    }

    /// The row's line, read now.
    pub(crate) fn read(&self) -> Line {
        (self.read)()
    }
}

/// The sections, top to bottom.
const SECTIONS: &[Section] = &[
    Section {
        title: "Rendering",
        rows: &[
            system::SESSION,
            system::DIRECT_BLIT,
            system::RENDER_SIZE,
            body::RENDER_RATE_BODY,
        ],
    },
    Section {
        title: "Input",
        rows: &[
            system::INPUT_ACTIONS,
            system::LAYOUT_FINGERPRINT,
            system::SELF_CHECK,
            capture::CONTROLLERS,
            capture::PANEL,
        ],
    },
    Section {
        title: "Capture and tools",
        rows: capture::TOOLS,
    },
    Section {
        title: "HUD",
        rows: &[system::ATLAS, system::WORLD_MARKERS],
    },
    Section {
        title: "Gun in hand",
        rows: &[
            controllers::RIGID_GRIP,
            controllers::CANONICAL_GRIP,
            controllers::FLIP_WATCH,
            controllers::ANIMATED_DEVIATION,
        ],
    },
    Section {
        title: "Weapons in hand",
        rows: &[
            weapons::LEFT_HAND,
            weapons::RIGHT_HAND,
            weapons::LEFT_HAND_ON_GUN,
            weapons::RIGHT_HAND_ON_GUN,
            weapons::TAKES,
            weapons::RACKS,
            weapons::ARMED,
            weapons::RIGHT_GRIP,
        ],
    },
    Section {
        title: "Inventory",
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
        title: "Explosives and magazines",
        rows: &[
            explosives::LIVE_GRENADES,
            explosives::C4_BY_HAND,
            explosives::MAGAZINES,
        ],
    },
    Section {
        title: "Aiming",
        rows: &[explosives::DOWN_THE_SIGHTS, explosives::GUN_AGAINST_WALLS],
    },
    Section {
        title: "Hand bodies",
        rows: &[
            controllers::CALLBACK,
            controllers::LEFT_BODY,
            controllers::RIGHT_BODY,
            controllers::BODY_ERRORS,
            controllers::CONTACT_EVENTS,
            controllers::LEFT_GUN_BODY,
            controllers::RIGHT_GUN_BODY,
        ],
    },
    Section {
        title: "Grip",
        rows: &[grip::LEFT_GRIP, grip::RIGHT_GRIP],
    },
    Section {
        title: "Hands in the world",
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
        title: "Feedback",
        rows: &[feedback::HAPTIC_CUES, feedback::IMPACT_SOUNDS],
    },
    Section {
        title: "Fingers",
        rows: &[fingers::LEFT_GRIP, fingers::RIGHT_GRIP],
    },
    Section {
        title: "Grapple",
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
        title: "Vehicles",
        rows: &[
            vehicles::FLIGHT_CONTROLS,
            vehicles::COCKPIT_BUTTONS,
            vehicles::PASSENGER_AIM,
            vehicles::MOUNTED_GUN,
        ],
    },
    Section {
        title: "Locomotion",
        rows: &[
            controllers::STICK_VELOCITY,
            controllers::ACT_FILTER,
            controllers::ACT_WATCHDOG,
            controllers::STEADY_VIEW,
            controllers::NEUTRAL_HEAD,
        ],
    },
    Section {
        title: "Procedural body",
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
        assert_eq!(Line::ok("2 eyes").text(), "Ok, 2 eyes");
        assert_eq!(Line::off("").text(), "Off");
        assert_eq!(Line::failed("See log").text(), "Failed, See log");
    }

    #[test]
    fn every_section_has_rows_and_labels_are_distinct() {
        let mut labels = Vec::new();
        for section in SECTIONS {
            assert!(!section.rows.is_empty(), "{}", section.title);
            labels.extend(section.rows.iter().map(|row| (section.title, row.label)));
        }
        let count = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), count);
    }
}
