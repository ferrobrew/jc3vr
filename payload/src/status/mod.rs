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

mod body;
mod controllers;
mod explosives;
mod feedback;
mod fingers;
mod grapple;
mod grip;
mod hands;
mod inventory;
mod system;
mod text;
mod vehicles;
mod weapons;

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

/// One row: its label, and how to read its line.
struct Row {
    label: &'static str,
    display_label: &'static str,
    read: fn() -> Line,
}

/// A row with its stable scenario ID and its authored UI label.
macro_rules! row {
    ($label:literal, $display:literal, $read:path) => {
        Row {
            label: $label,
            display_label: $display,
            read: $read,
        }
    };
}

/// The sections, top to bottom.
const SECTIONS: &[Section] = &[
    Section {
        title: "SAVED SETTINGS",
        display_title: "Saved settings",
        rows: &[row!(
            "SETTINGS FILE",
            "Settings file",
            system::settings_file
        )],
    },
    Section {
        title: "RENDERING",
        display_title: "Rendering",
        rows: &[
            row!("VR SESSION", "VR session", system::session),
            row!("DIRECT EYE BLIT", "Direct eye blit", system::direct_blit),
            row!("FRAME TIME", "Frame time", system::frame_time),
            row!("RENDER SIZE", "Render size", system::render_size),
            row!(
                "RENDER-RATE BODY",
                "Render-rate body",
                body::render_rate_body
            ),
        ],
    },
    Section {
        title: "INPUT",
        display_title: "Input",
        rows: &[
            row!(
                "CONTROLLER ACTIONS",
                "Controller actions",
                system::input_actions
            ),
            row!(
                "LAYOUT FINGERPRINT",
                "Layout fingerprint",
                system::layout_fingerprint
            ),
            row!("SELF-CHECK", "Self-check", system::self_check),
        ],
    },
    Section {
        title: "HUD",
        display_title: "HUD",
        rows: &[
            row!("ATLAS PARTS", "Atlas parts", system::atlas),
            row!("WORLD MARKERS", "World markers", system::world_markers),
            row!("MARKER DEPTH", "Marker depth", system::marker_depth),
        ],
    },
    Section {
        title: "GUN IN HAND",
        display_title: "Gun in hand",
        rows: &[
            row!("RIGID GRIP", "Rigid grip", controllers::rigid_grip),
            row!(
                "CANONICAL GRIP",
                "Canonical grip",
                controllers::canonical_grip
            ),
            row!("FLIP WATCH", "Flip watch", controllers::flip_watch),
            row!(
                "ANIMATED GRIP OFFSET",
                "Animated grip offset",
                controllers::animated_deviation
            ),
            row!(
                "ATTACH BONE CHANGES",
                "Attach bone changes",
                controllers::bone_changes
            ),
        ],
    },
    Section {
        title: "WEAPONS IN HAND",
        display_title: "Weapons in hand",
        rows: &[
            row!("LEFT HAND", "Left hand", weapons::left_hand),
            row!("RIGHT HAND", "Right hand", weapons::right_hand),
            row!(
                "LEFT HAND ON GUN",
                "Left hand on gun",
                weapons::left_hand_on_gun
            ),
            row!(
                "RIGHT HAND ON GUN",
                "Right hand on gun",
                weapons::right_hand_on_gun
            ),
            row!("TAKE FROM HANDS", "Take from hands", weapons::takes),
            row!("TAKE FROM RACKS", "Take from racks", weapons::racks),
            row!("LEFT ACTION", "Left action", weapons::left_action),
            row!("RIGHT ACTION", "Right action", weapons::right_action),
            row!("ARMED BESIDE", "Armed beside", weapons::armed),
            row!("RIGHT HAND GRIP", "Right hand grip", weapons::right_grip),
            row!("SOCKETS", "Sockets", weapons::docks),
            row!("LAST REFUSAL", "Last refusal", weapons::last_refusal),
        ],
    },
    Section {
        title: "INVENTORY",
        display_title: "Inventory",
        rows: &[
            row!("TRACKED WEAPONS", "Tracked weapons", inventory::tracked),
            row!("CARRIED", "Carried", inventory::carried),
            row!(
                "SEPARATE PISTOLS",
                "Separate pistols",
                inventory::separate_pistols
            ),
            row!("SAVE SIDECAR", "Save sidecar", inventory::sidecar),
            row!("LAST LOAD", "Last load", inventory::last_load),
            row!("KEPT IN THE WORLD", "Kept in the world", inventory::pinned),
            row!("LOADOUT TABLE", "Loadout table", inventory::loadout),
        ],
    },
    Section {
        title: "EXPLOSIVES AND MAGAZINES",
        display_title: "Explosives and magazines",
        rows: &[
            row!("LIVE GRENADES", "Live grenades", explosives::live_grenades),
            row!("C4 BY HAND", "C4 by hand", explosives::c4_by_hand),
            row!("MAGAZINES", "Magazines", explosives::magazines),
        ],
    },
    Section {
        title: "AIMING",
        display_title: "Aiming",
        rows: &[
            row!(
                "DOWN THE SIGHTS",
                "Down the sights",
                explosives::down_the_sights
            ),
            row!(
                "GUN AGAINST WALLS",
                "Gun against walls",
                explosives::gun_against_walls
            ),
        ],
    },
    Section {
        title: "HAND BODIES",
        display_title: "Hand bodies",
        rows: &[
            row!("CALLBACK", "Callback", controllers::callback),
            row!(
                "CALLBACK FIRES",
                "Callback fires",
                controllers::callback_fires
            ),
            row!("LEFT BODY", "Left body", controllers::left_body),
            row!("RIGHT BODY", "Right body", controllers::right_body),
            row!("BODY ERRORS", "Body errors", controllers::body_errors),
            row!(
                "CONTACT EVENTS",
                "Contact events",
                controllers::contact_events
            ),
            row!("BLOWS DEALT", "Blows dealt", controllers::blows),
            row!("GRAB JOINTS", "Grab joints", controllers::joints),
            row!("COLLIDER VIEW", "Collider view", controllers::collider_view),
            row!("LEFT GUN BODY", "Left gun body", controllers::left_gun_body),
            row!(
                "RIGHT GUN BODY",
                "Right gun body",
                controllers::right_gun_body
            ),
        ],
    },
    Section {
        title: "GRIP",
        display_title: "Grip",
        rows: &[
            row!("LEFT GRIP", "Left grip", grip::left_grip),
            row!("RIGHT GRIP", "Right grip", grip::right_grip),
            row!("GRIP MARKERS", "Grip markers", grip::markers),
        ],
    },
    Section {
        title: "HANDS IN THE WORLD",
        display_title: "Hands in the world",
        rows: &[
            row!("PALMS", "Palms", hands::palms),
            row!("VEHICLE DOORS", "Vehicle doors", hands::doors),
            row!("CHARACTERS", "Characters", hands::characters),
            row!("MOVING CLIMBS", "Moving climbs", hands::moving_climbs),
            row!("STUNT POSITIONS", "Stunt positions", hands::stunt_positions),
            row!("PUSH UP", "Push up", hands::push_up),
            row!("HINGED THINGS", "Hinged things", hands::articulated),
        ],
    },
    Section {
        title: "FEEDBACK",
        display_title: "Feedback",
        rows: &[
            row!("HAPTIC CUES", "Haptic cues", feedback::haptic_cues),
            row!("IMPACT SOUNDS", "Impact sounds", feedback::impact_sounds),
        ],
    },
    Section {
        title: "FINGERS",
        display_title: "Fingers",
        rows: &[
            row!("LEFT GRIP POSE", "Left grip pose", fingers::left_grip),
            row!("RIGHT GRIP POSE", "Right grip pose", fingers::right_grip),
        ],
    },
    Section {
        title: "GRAPPLE",
        display_title: "Grapple",
        rows: &[
            row!("GRAPPLE STATE", "Grapple state", grapple::state),
            row!("ATTACHED TARGET", "Attached target", grapple::target),
            row!("REEL FROM HAND", "Reel from hand", grapple::reel),
            row!("TETHERS", "Tethers", grapple::tethers),
            row!("HANG SWING", "Hang swing", grapple::swing),
            row!("SNATCH", "Snatch", grapple::snatch),
            row!("LINE HAPTICS", "Line haptics", grapple::haptics),
        ],
    },
    Section {
        title: "VEHICLES",
        display_title: "Vehicles",
        rows: &[
            row!(
                "FLIGHT CONTROLS",
                "Flight controls",
                vehicles::flight_controls
            ),
            row!(
                "COCKPIT BUTTONS",
                "Cockpit buttons",
                vehicles::cockpit_buttons
            ),
            row!("PASSENGER AIM", "Passenger aim", vehicles::passenger_aim),
            row!("MOUNTED GUN", "Mounted gun", vehicles::mounted_gun),
        ],
    },
    Section {
        title: "LOCOMOTION",
        display_title: "Locomotion",
        rows: &[
            row!(
                "STICK VELOCITY",
                "Stick velocity",
                controllers::stick_velocity
            ),
            row!("ACT FILTER", "Act filter", controllers::act_filter),
            row!("ACT WATCHDOG", "Act watchdog", controllers::act_watchdog),
            row!("STEADY VIEW", "Steady view", controllers::steady_view),
            row!("NEUTRAL HEAD", "Neutral head", controllers::neutral_head),
        ],
    },
    Section {
        title: "PROCEDURAL BODY",
        display_title: "Procedural body",
        rows: &[
            row!("MODE", "Mode", controllers::procedural_mode),
            row!(
                "PASS-THROUGH ERROR",
                "Pass-through error",
                controllers::pass_through
            ),
            row!("HAND-OVER CASE", "Hand-over case", controllers::hand_over),
            row!("GROUND CASTS", "Ground casts", controllers::ground_casts),
            row!("STANCE", "Stance", controllers::stance),
            row!("LOWER BODY", "Lower body", controllers::lower_body),
            row!("POSE INPUTS", "Pose inputs", controllers::lower_inputs),
            row!("TWIST BONES", "Twist bones", controllers::twist_bones),
            row!("SEATED TORSO", "Seated torso", body::seated_torso),
            row!("WINGSUIT ARMS", "Wingsuit arms", body::wingsuit_arms),
            row!("PARACHUTE BODY", "Parachute body", body::parachute_body),
            row!("SWIM TORSO", "Swim torso", body::swim_torso),
            row!("ROOF TORSO", "Roof torso", body::roof_torso),
            row!("BODY MIRROR", "Body mirror", body::body_mirror),
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
