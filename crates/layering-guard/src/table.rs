//! The module table: which crate each payload module path goes to.
//!
//! The table is a data file of `<module path> <layer>` lines (`modules.txt`). A path resolves to the
//! entry with the longest matching prefix, counted in whole segments, so `controllers` can go to
//! gameplay while `controllers::frame` goes to input. Entries may also name single items
//! (`controllers::weapons::scope::lens::EyeCamera`) when the design moves an item ahead of its
//! module.

use std::{collections::BTreeMap, fmt};

use crate::layer::Layer;

/// A parsed module table.
#[derive(Debug, Default)]
pub struct ModuleTable {
    entries: BTreeMap<Vec<String>, Layer>,
}

/// A malformed line in the module table.
#[derive(Debug, PartialEq, Eq)]
pub struct TableError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "module table: line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for TableError {}

impl ModuleTable {
    /// Parses the table. Blank lines and `#` comments are ignored.
    pub fn parse(text: &str) -> Result<ModuleTable, TableError> {
        let mut entries = BTreeMap::new();
        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let content = raw.split('#').next().unwrap_or_default().trim();
            if content.is_empty() {
                continue;
            }
            let error = |message: String| TableError { line, message };
            let mut fields = content.split_whitespace();
            let (Some(path), Some(layer), None) = (fields.next(), fields.next(), fields.next())
            else {
                return Err(error(format!(
                    "expected `<module path> <layer>`, found `{content}`"
                )));
            };
            let layer = Layer::from_name(layer).ok_or_else(|| {
                error(format!(
                    "unknown layer `{layer}` (expected one of {})",
                    Layer::ALL.map(Layer::name).join(", ")
                ))
            })?;
            let segments: Vec<String> = path.split("::").map(str::to_owned).collect();
            if segments.iter().any(|segment| segment.is_empty()) {
                return Err(error(format!("malformed module path `{path}`")));
            }
            if let Some(previous) = entries.insert(segments, layer) {
                return Err(error(format!(
                    "`{path}` is listed twice (also as {previous})"
                )));
            }
        }
        Ok(ModuleTable { entries })
    }

    /// The layer of the longest listed prefix of `segments`.
    pub fn resolve<S: AsRef<str>>(&self, segments: &[S]) -> Option<Layer> {
        let mut key: Vec<String> = segments.iter().map(|s| s.as_ref().to_owned()).collect();
        while !key.is_empty() {
            if let Some(&layer) = self.entries.get(&key) {
                return Some(layer);
            }
            key.pop();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const TABLE: &str = "
        # Comment lines and trailing comments are ignored.
        controllers gameplay
        controllers::frame input   # Pure frame types.
        controllers::summary::glyphs hud
        hud hud
        hud::render_camera_pose render
    ";

    #[test]
    fn longest_prefix_wins() {
        let table = ModuleTable::parse(TABLE).unwrap();
        assert_eq!(
            table.resolve(&["controllers", "weapons", "hold"]),
            Some(Layer::Gameplay)
        );
        assert_eq!(
            table.resolve(&["controllers", "frame", "ControllerFrame"]),
            Some(Layer::Input)
        );
        assert_eq!(
            table.resolve(&["controllers", "summary", "glyphs"]),
            Some(Layer::Hud)
        );
        assert_eq!(
            table.resolve(&["hud", "render_camera_pose", "publish"]),
            Some(Layer::Render)
        );
        assert_eq!(table.resolve(&["hud", "pointer"]), Some(Layer::Hud));
    }

    #[test]
    fn prefixes_are_whole_segments() {
        let table = ModuleTable::parse(TABLE).unwrap();
        assert_eq!(
            table.resolve(&["controllers", "frames"]),
            Some(Layer::Gameplay)
        );
        assert_eq!(table.resolve(&["huds"]), None);
        assert_eq!(table.resolve::<&str>(&[]), None);
    }

    #[test]
    fn rejects_unknown_layers() {
        let error = ModuleTable::parse("vr rendering\n").unwrap_err();
        assert_eq!(error.line, 1);
        assert!(
            error
                .to_string()
                .starts_with("module table: line 1: unknown layer `rendering`")
        );
    }

    #[test]
    fn rejects_duplicates_and_malformed_lines() {
        assert_eq!(
            ModuleTable::parse("a core\n\na input\n").unwrap_err().line,
            3
        );
        assert_eq!(ModuleTable::parse("a core extra\n").unwrap_err().line, 1);
        assert_eq!(ModuleTable::parse("a:: core\n").unwrap_err().line, 1);
    }
}
