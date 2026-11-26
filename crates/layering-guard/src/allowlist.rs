//! The allowlist of known upward edges, which may only shrink.
//!
//! The data file (`allowlist.txt`) holds one edge per line, `<file> -> <layer>`, sorted bytewise
//! (`LC_ALL=C sort`) with no duplicates, so concurrent deletions merge without conflicts. An edge
//! is file-level: it allows `<file>` to name any path in `<layer>`, and it is stale once the file
//! names none.

use std::{collections::BTreeSet, fmt};

use crate::layer::Layer;

/// An upward edge from one source file to a layer above its own.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edge {
    /// The file's path relative to the repository root, with `/` separators.
    pub file: String,
    pub to: Layer,
}

impl Edge {
    /// The edge's line in the allowlist.
    pub fn line(&self) -> String {
        format!("{} -> {}", self.file, self.to)
    }
}

/// A parsed allowlist.
#[derive(Debug, Default)]
pub struct Allowlist {
    pub(crate) edges: BTreeSet<Edge>,
}

/// A malformed, unsorted, or duplicated line in the allowlist.
#[derive(Debug, PartialEq, Eq)]
pub struct AllowlistError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for AllowlistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "allowlist: line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for AllowlistError {}

impl Allowlist {
    /// Parses the allowlist. Blank lines and lines starting with `#` are ignored.
    pub fn parse(text: &str) -> Result<Allowlist, AllowlistError> {
        let mut edges = BTreeSet::new();
        let mut previous: Option<String> = None;
        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let content = raw.trim();
            if content.is_empty() || content.starts_with('#') {
                continue;
            }
            let error = |message: String| AllowlistError { line, message };
            let Some((file, layer)) = content.split_once(" -> ") else {
                return Err(error(format!(
                    "expected `<file> -> <layer>`, found `{content}`"
                )));
            };
            let to = Layer::from_name(layer.trim()).ok_or_else(|| {
                error(format!(
                    "unknown layer `{}` (expected one of {})",
                    layer.trim(),
                    Layer::ALL.map(Layer::name).join(", ")
                ))
            })?;
            let edge = Edge {
                file: file.trim().to_owned(),
                to,
            };
            let normalized = edge.line();
            if normalized != content {
                return Err(error(format!("write the line as `{normalized}`")));
            }
            if let Some(previous) = &previous {
                if *previous == normalized {
                    return Err(error(format!("`{normalized}` is listed twice; delete one")));
                }
                if *previous > normalized {
                    return Err(error(format!(
                        "`{normalized}` is out of order after `{previous}`; keep the file sorted \
                         (`LC_ALL=C sort`)"
                    )));
                }
            }
            previous = Some(normalized);
            edges.insert(edge);
        }
        Ok(Allowlist { edges })
    }

    pub fn contains(&self, edge: &Edge) -> bool {
        self.edges.contains(edge)
    }

    pub fn len(&self) -> usize {
        self.edges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn parses_sorted_edges() {
        let text = "# Header.\n\npayload/src/a.rs -> hud\npayload/src/a.rs -> ui\npayload/src/a/b.rs -> core\n";
        let allowlist = Allowlist::parse(text).unwrap();
        assert_eq!(allowlist.len(), 3);
        assert!(allowlist.contains(&Edge {
            file: "payload/src/a.rs".into(),
            to: Layer::Ui
        }));
        assert!(!allowlist.contains(&Edge {
            file: "payload/src/a.rs".into(),
            to: Layer::Render
        }));
    }

    #[test]
    fn edge_order_matches_line_order() {
        let mut edges = [
            Edge {
                file: "payload/src/a/b.rs".into(),
                to: Layer::Core,
            },
            Edge {
                file: "payload/src/a.rs".into(),
                to: Layer::Ui,
            },
            Edge {
                file: "payload/src/a.rs".into(),
                to: Layer::Hud,
            },
            Edge {
                file: "payload/src/a.rs2".into(),
                to: Layer::Core,
            },
        ];
        let mut lines: Vec<String> = edges.iter().map(Edge::line).collect();
        lines.sort();
        edges.sort_by_key(Edge::line);
        assert_eq!(edges.iter().map(Edge::line).collect::<Vec<_>>(), lines);
    }

    #[test]
    fn rejects_unsorted_lines() {
        let error =
            Allowlist::parse("payload/src/b.rs -> hud\npayload/src/a.rs -> hud\n").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("out of order"));
    }

    #[test]
    fn rejects_duplicates() {
        let error =
            Allowlist::parse("payload/src/a.rs -> hud\npayload/src/a.rs -> hud\n").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("listed twice"));
    }

    #[test]
    fn rejects_malformed_lines() {
        assert!(Allowlist::parse("payload/src/a.rs hud\n").is_err());
        assert!(Allowlist::parse("payload/src/a.rs -> rendering\n").is_err());
        assert!(
            Allowlist::parse("payload/src/a.rs  ->  hud\n")
                .unwrap_err()
                .message
                .contains("write the line as")
        );
    }
}
