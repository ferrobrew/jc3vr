//! The embedded door handle table: `tools/door_handles/door_handles.json`, derived offline from the
//! game's entry clips, door meshes, and door constraints by `tools/door_handles/extract_door_handles.py`
//! and compiled into the payload. Each entry is keyed by its door part's name hash
//! ([`Part::m_name`](jc3gi::vehicle::parts::Part)), and gives the handle and the outer face's normal
//! in the part's own space, with the tool's grade ([`HandleGrade`]).

use std::{collections::HashMap, fmt, sync::OnceLock};

use glam::Vec3;
use serde::Deserialize;

/// How the tool placed an entry's handle, from the table's `grade` field.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleGrade {
    /// Where an entry clip's hand is when the clip opens the door, on the door's outer surface.
    Clip,
    /// As [`HandleGrade::Clip`], but with a doubt the tool notes: the hand far from the door, an
    /// entry other than the standard one, or a warp unfinished when the door opens.
    ClipDoubtful,
    /// From the door's box and its constraint point, as no clip reaches the door: hoods, trunks,
    /// and most rear doors.
    Hinge,
}

/// One door part's entry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TableEntry {
    /// The vehicle's model name, the prefix of its part names.
    pub vehicle: String,
    /// The door part's name.
    pub part: String,
    /// The door part's name hash, the table's key.
    pub part_hash: u32,
    /// The handle, in the door part's own space, metres.
    pub handle_m: Vec3,
    /// The door's outer face at the handle, a unit vector in the part's own space.
    pub normal: Vec3,
    pub grade: HandleGrade,
}

/// The embedded table's entry for the door part whose name hashes to `part_hash`. An embedded
/// table that does not parse is logged once and treated as empty, which leaves every door on its
/// box guess.
pub fn lookup(part_hash: u32) -> Option<&'static TableEntry> {
    static TABLE: OnceLock<HashMap<u32, TableEntry>> = OnceLock::new();
    TABLE
        .get_or_init(|| match parse(DOOR_HANDLES_JSON) {
            Ok(entries) => entries.into_iter().map(|e| (e.part_hash, e)).collect(),
            Err(e) => {
                tracing::error!(target: "controllers", "{e}");
                HashMap::new()
            }
        })
        .get(&part_hash)
}

/// Parse a table in the tool's format: a JSON array of entries.
pub fn parse(json: &str) -> Result<Vec<TableEntry>, DoorHandleTableError> {
    serde_json::from_str(json).map_err(DoorHandleTableError)
}

/// The door handle table does not parse.
#[derive(Debug)]
pub struct DoorHandleTableError(serde_json::Error);

impl fmt::Display for DoorHandleTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "door handle table: the table does not parse: {}", self.0)
    }
}

impl std::error::Error for DoorHandleTableError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// The shipped table; regenerate with `tools/door_handles/extract_door_handles.py`.
const DOOR_HANDLES_JSON: &str = include_str!("../../../../../tools/door_handles/door_handles.json");

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn the_embedded_table_parses_with_one_entry_per_door_part() {
        let entries = parse(DOOR_HANDLES_JSON).unwrap();
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|e| e.grade == HandleGrade::Clip));
        assert!(entries.iter().any(|e| e.grade == HandleGrade::Hinge));
        let keys: HashSet<u32> = entries.iter().map(|e| e.part_hash).collect();
        assert_eq!(keys.len(), entries.len(), "one entry per door part");
        for entry in &entries {
            assert!(entry.part.starts_with(&entry.vehicle), "{}", entry.part);
            assert!((entry.normal.length() - 1.0).abs() < 1e-2, "{}", entry.part);
            assert_eq!(lookup(entry.part_hash).map(|e| &e.part), Some(&entry.part));
        }
    }

    #[test]
    fn an_entry_reads_the_tool_line_format() {
        let json = r#"[
  {"vehicle": "testcar", "part": "testcar_door_f_l", "part_hash": 7, "slot": "FrontLeftDoor", "handle_m": [-0.93, 0.99, 0.37], "normal": [-1.0, 0.0, 0.0], "grade": "clip", "clip": "a.ban", "notes": ""},
  {"vehicle": "testcar", "part": "testcar_hood", "part_hash": 8, "slot": "Hood", "handle_m": [0.0, 0.85, -2.06], "normal": [0.0, 1.0, 0.0], "grade": "hinge", "notes": "hinge from the constraint"}
]"#;
        let entries = parse(json).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].handle_m, Vec3::new(-0.93, 0.99, 0.37));
        assert_eq!(entries[1].grade, HandleGrade::Hinge);
    }

    #[test]
    fn an_unknown_grade_is_an_error() {
        let json = r#"[{"vehicle": "v", "part": "v_door", "part_hash": 1, "handle_m": [0, 0, 0], "normal": [1, 0, 0], "grade": "guessed"}]"#;
        let error = parse(json).unwrap_err();
        assert!(error.to_string().starts_with("door handle table: "));
    }

    #[test]
    fn a_part_missing_from_the_table_has_no_entry() {
        assert!(lookup(0).is_none());
    }
}
