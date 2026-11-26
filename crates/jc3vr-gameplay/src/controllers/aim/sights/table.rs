//! The embedded sight table: `tools/sights/sights.json`, derived offline from the game's weapon
//! models by `tools/sights/extract_sights.py` and compiled into the payload. Each entry carries the
//! tool's confidence in it ([`SightGrade`]); only the trusted grades correct a sight line.

use std::{fmt, sync::OnceLock};

use glam::{Vec2, Vec3};
use serde::Deserialize;

use crate::controllers::config::SightOverride;

/// The tool's confidence in an entry, from the table's `grade` field.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SightGrade {
    /// The line runs down a scope tube's axis.
    Scoped,
    /// Iron sights on a one-handed weapon, passing the tool's plausibility checks.
    Pistol,
    /// Iron sights on a two-handed weapon, or a line that fails the checks.
    IronUncertain,
    /// No line was derived.
    Failed,
    /// Tuned in the headset and pasted back into the table.
    Tuned,
}

impl SightGrade {
    /// Whether an entry of this grade corrects a sight line. The uncertain and failed entries
    /// need review against the tool's plots before they can be relied on.
    pub const fn trusted(self) -> bool {
        matches!(
            self,
            SightGrade::Scoped | SightGrade::Pistol | SightGrade::Tuned
        )
    }

    /// The grade as the table spells it.
    pub const fn label(self) -> &'static str {
        match self {
            SightGrade::Scoped => "scoped",
            SightGrade::Pistol => "pistol",
            SightGrade::IronUncertain => "iron_uncertain",
            SightGrade::Failed => "failed",
            SightGrade::Tuned => "tuned",
        }
    }
}

/// One weapon's entry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TableEntry {
    /// The weapon entity's name.
    pub weapon: String,
    /// The weapon type's name hash, the table's key.
    pub weapon_hash: u32,
    /// The sight line's origin relative to the fire point, in the fire point's frame, metres.
    pub offset_m: Vec3,
    /// The sight line's pitch and yaw relative to the barrel, degrees.
    pub angles_deg: Vec2,
    pub grade: SightGrade,
}

impl TableEntry {
    /// The entry as a sight correction.
    pub fn sight(&self) -> SightOverride {
        SightOverride {
            weapon_hash: self.weapon_hash,
            offset_m: self.offset_m,
            angles_deg: self.angles_deg,
        }
    }
}

/// The embedded table's entries, of every grade. An embedded table that does not parse is logged
/// once and treated as empty, which leaves every weapon on its fire point.
pub fn entries() -> &'static [TableEntry] {
    static TABLE: OnceLock<Vec<TableEntry>> = OnceLock::new();
    TABLE.get_or_init(|| {
        parse(SIGHTS_JSON).unwrap_or_else(|e| {
            tracing::error!(target: "controllers", "{e}");
            Vec::new()
        })
    })
}

/// Parse a table in the tool's format: a JSON array of entries.
pub fn parse(json: &str) -> Result<Vec<TableEntry>, SightTableError> {
    serde_json::from_str(json).map_err(SightTableError)
}

/// The sight table does not parse.
#[derive(Debug)]
pub struct SightTableError(serde_json::Error);

impl fmt::Display for SightTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sight table: the table does not parse: {}", self.0)
    }
}

impl std::error::Error for SightTableError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// The shipped table; regenerate with `tools/sights/extract_sights.py`.
const SIGHTS_JSON: &str = include_str!("../../../../../tools/sights/sights.json");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_table_parses_and_grades_its_entries() {
        let entries = parse(SIGHTS_JSON).unwrap();
        assert!(!entries.is_empty());
        let scoped: Vec<_> = entries
            .iter()
            .filter(|e| e.grade == SightGrade::Scoped)
            .collect();
        assert!(!scoped.is_empty());
        // A scope sits above the bore.
        assert!(scoped.iter().all(|e| e.offset_m.y > 0.0));
        assert!(entries.iter().any(|e| e.grade == SightGrade::Pistol));
    }

    #[test]
    fn an_entry_reads_the_tool_line_format() {
        let json = r#"[
  {"weapon": "w000_test_rifle", "weapon_hash": 12345, "offset_m": [0.0, 0.0869, 0.0], "angles_deg": [0.5, 0.0], "grade": "scoped"},
  {"weapon": "w001_test_pistol", "weapon_hash": 678, "offset_m": [0.0, -0.0035, 0.0], "angles_deg": [2.85, 0.0], "grade": "iron_uncertain"}
]"#;
        let entries = parse(json).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].sight(),
            SightOverride {
                weapon_hash: 12345,
                offset_m: Vec3::new(0.0, 0.0869, 0.0),
                angles_deg: Vec2::new(0.5, 0.0),
            }
        );
        assert_eq!(entries[1].grade, SightGrade::IronUncertain);
    }

    #[test]
    fn only_the_reliable_grades_are_trusted() {
        assert!(SightGrade::Scoped.trusted());
        assert!(SightGrade::Pistol.trusted());
        assert!(SightGrade::Tuned.trusted());
        assert!(!SightGrade::IronUncertain.trusted());
        assert!(!SightGrade::Failed.trusted());
    }

    #[test]
    fn an_unknown_grade_is_an_error() {
        let json = r#"[{"weapon": "w", "weapon_hash": 1, "offset_m": [0, 0, 0], "angles_deg": [0, 0], "grade": "guessed"}]"#;
        let error = parse(json).unwrap_err();
        assert!(error.to_string().starts_with("sight table: "));
    }
}
