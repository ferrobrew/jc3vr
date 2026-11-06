//! The embedded wells table: `tools/sights/wells.json`, derived offline from the game's weapon
//! models by `tools/sights/extract_wells.py` and compiled into the payload. Each entry gives, in
//! the gun's model frame (+Y up, -Z toward the muzzle, metres), where a weapon takes its rounds
//! and how its action is worked.

use std::{fmt, sync::OnceLock};

use glam::Vec3;
use serde::Deserialize;

use crate::controllers::weapons::reload::chamber::ActionKind;

/// What a weapon loads.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WellKind {
    Magazine,
    /// A shell at a time, through a loading port or the breech.
    Shell,
    /// A round into the muzzle: a rocket or a mortar shell.
    Rocket,
    /// A revolver's cylinder.
    Cylinder,
    /// Nothing to load.
    None,
}

impl WellKind {
    pub const fn label(self) -> &'static str {
        match self {
            WellKind::Magazine => "magazine",
            WellKind::Shell => "shell",
            WellKind::Rocket => "rocket",
            WellKind::Cylinder => "cylinder",
            WellKind::None => "none",
        }
    }
}

/// The action as the table spells it.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableAction {
    Slide,
    ChargingHandle,
    Bolt,
    Pump,
    None,
}

impl From<TableAction> for ActionKind {
    fn from(action: TableAction) -> Self {
        match action {
            TableAction::Slide => ActionKind::Slide,
            TableAction::ChargingHandle => ActionKind::ChargingHandle,
            TableAction::Bolt => ActionKind::Bolt,
            TableAction::Pump => ActionKind::Pump,
            TableAction::None => ActionKind::None,
        }
    }
}

/// The tool's confidence in an entry.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WellGrade {
    /// From the weapon's own magazine part.
    Part,
    /// From the body mesh.
    Mesh,
    /// A class default, not measured.
    Default,
    /// Nothing was derived.
    Failed,
}

impl WellGrade {
    /// Whether the entry's geometry is used. A failed entry falls back to the configured well.
    pub const fn trusted(self) -> bool {
        !matches!(self, WellGrade::Failed)
    }

    pub const fn label(self) -> &'static str {
        match self {
            WellGrade::Part => "part",
            WellGrade::Mesh => "mesh",
            WellGrade::Default => "default",
            WellGrade::Failed => "failed",
        }
    }
}

/// One weapon's entry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WellEntry {
    pub weapon: String,
    /// The weapon type's name hash, the table's key.
    pub weapon_hash: u32,
    pub kind: WellKind,
    /// Where a round enters the gun, on its surface; `None` for a weapon with nothing to load.
    pub mouth_m: Option<Vec3>,
    /// The direction a round travels into the gun, unit length.
    pub insert_axis: Option<Vec3>,
    /// Whether the weapon draws its magazine as a separate model part.
    pub magazine_part: bool,
    /// The top of the magazine part's mesh along the insertion axis, from the part's origin.
    pub part_top_m: Option<f32>,
    /// The magazine part's extent along the insertion axis; without a part, the visible length of
    /// a magazine modelled below the receiver, or `None` for one inside the grip.
    pub part_length_m: Option<f32>,
    /// The top of the gun's spare magazine model along the insertion axis, centred across it, in
    /// that model's own frame; `None` when the gun has no spare magazine model that depicts the
    /// magazine a hand loads.
    #[serde(default)]
    pub spare_top_m: Option<Vec3>,
    /// The stand-in model a round in the hand is drawn with, for a gun with no model of its own
    /// for a loose round.
    #[serde(flatten)]
    pub round: RoundEntry,
    pub action: TableAction,
    /// Where the hand grabs the action.
    pub handle_m: Option<Vec3>,
    /// The action's travel along the barrel, metres.
    pub travel_m: Option<f32>,
    pub grade: WellGrade,
}

/// The stand-in model of a round in the hand for a shell-fed gun or a revolver: the casing the gun's
/// casing-ejection effect throws out. Every field is `None` for any other gun, and for one whose
/// casing the tool could not resolve.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct RoundEntry {
    /// The casing model's `.lod` path, under whose `hashlittle` the game's model cache holds it.
    pub round_model: Option<String>,
    /// The casing's feed end on its axis, in the model's own frame.
    pub round_top_m: Option<Vec3>,
    /// The direction the round travels into the gun, in the model's own frame, unit length.
    pub round_axis: Option<Vec3>,
    /// The casing's radius across its axis.
    pub round_radius_m: Option<f32>,
    /// How many rounds one push loads: one shell, or a revolver's cylinder.
    pub round_count: Option<u32>,
    /// The ring the rounds of a group stand on, from the group's axis to each round's; zero for
    /// one round.
    pub round_ring_m: Option<f32>,
}

/// The embedded table's entries. A table that does not parse is logged once and treated as empty,
/// which leaves every weapon on the class defaults and the configured well.
pub fn entries() -> &'static [WellEntry] {
    static TABLE: OnceLock<Vec<WellEntry>> = OnceLock::new();
    TABLE.get_or_init(|| {
        parse(WELLS_JSON).unwrap_or_else(|e| {
            tracing::error!(target: "controllers", "{e}");
            Vec::new()
        })
    })
}

/// The entry for the weapon type `weapon_hash`.
pub fn lookup(weapon_hash: u32) -> Option<&'static WellEntry> {
    entries().iter().find(|e| e.weapon_hash == weapon_hash)
}

/// Parse a table in the tool's format: a JSON array of entries.
pub fn parse(json: &str) -> Result<Vec<WellEntry>, WellsTableError> {
    serde_json::from_str(json).map_err(WellsTableError)
}

/// The wells table does not parse.
#[derive(Debug)]
pub struct WellsTableError(serde_json::Error);

impl fmt::Display for WellsTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wells table: the table does not parse: {}", self.0)
    }
}

impl std::error::Error for WellsTableError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// The shipped table; regenerate with `tools/sights/extract_wells.py`.
const WELLS_JSON: &str = include_str!("../../../../../tools/sights/wells.json");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_table_parses_with_unit_axes() {
        let entries = parse(WELLS_JSON).unwrap();
        assert!(!entries.is_empty());
        for e in entries.iter().filter(|e| e.kind != WellKind::None) {
            assert!(e.mouth_m.is_some(), "{}: no mouth", e.weapon);
            let axis = e.insert_axis.unwrap_or_default();
            assert!(
                (axis.length() - 1.0).abs() < 0.01,
                "{}: the insertion axis is not unit length",
                e.weapon
            );
        }
        assert!(
            entries
                .iter()
                .any(|e| e.kind == WellKind::Magazine && e.magazine_part)
        );
    }

    #[test]
    fn an_entry_reads_the_tool_line_format() {
        let json = r#"[
  {"weapon": "w000_test_pistol", "weapon_hash": 1, "kind": "magazine", "mouth_m": [0.0, -0.1, 0.05], "insert_axis": [0.0, 1.0, 0.0], "magazine_part": false, "part_name": null, "part_top_m": null, "part_length_m": 0.1, "action": "slide", "handle_m": [0.0, 0.03, 0.08], "travel_m": 0.025, "grade": "mesh"},
  {"weapon": "w001_test_launcher", "weapon_hash": 2, "kind": "rocket", "mouth_m": [0.0, 0.1, -0.6], "insert_axis": [0.0, 0.0, 1.0], "magazine_part": true, "part_name": "model.rpg", "part_top_m": 0.3, "part_length_m": 0.6, "action": "none", "handle_m": null, "travel_m": null, "grade": "default", "notes": "x"}
]"#;
        let entries = parse(json).unwrap();
        assert_eq!(entries[0].kind, WellKind::Magazine);
        assert_eq!(ActionKind::from(entries[0].action), ActionKind::Slide);
        assert_eq!(entries[0].travel_m, Some(0.025));
        assert_eq!(entries[1].handle_m, None);
        assert_eq!(entries[1].grade, WellGrade::Default);
        assert_eq!(entries[1].round, RoundEntry::default());
    }

    #[test]
    fn the_shell_fed_guns_and_the_revolvers_name_their_ejected_casing() {
        let entries = parse(WELLS_JSON).unwrap();
        for e in &entries {
            let round = &e.round;
            let fed = matches!(e.kind, WellKind::Shell | WellKind::Cylinder);
            assert_eq!(round.round_model.is_some(), fed, "{}", e.weapon);
            if !fed {
                continue;
            }
            let axis = round.round_axis.unwrap();
            assert!((axis.length() - 1.0).abs() < 1e-3, "{}", e.weapon);
            let top = round.round_top_m.unwrap();
            // The top lies on the casing's axis, ahead of its centre.
            assert!(top.dot(axis) > 0.01, "{}", e.weapon);
            assert!(top.reject_from(axis).length() < 1e-3, "{}", e.weapon);
            let radius = round.round_radius_m.unwrap();
            assert!((0.003..0.015).contains(&radius), "{}", e.weapon);
            let (count, ring) = (round.round_count.unwrap(), round.round_ring_m.unwrap());
            match e.kind {
                WellKind::Shell => assert_eq!((count, ring), (1, 0.0), "{}", e.weapon),
                _ => {
                    assert_eq!(count, 6, "{}", e.weapon);
                    // Six casings on the ring touch their neighbours.
                    assert!((ring - 2.0 * radius).abs() < 1e-3, "{}", e.weapon);
                }
            }
        }
    }
}
