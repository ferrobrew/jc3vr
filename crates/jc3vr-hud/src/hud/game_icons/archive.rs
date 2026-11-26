//! The game's file archives, read straight from disk: an index of every file's name hash, and the
//! bytes of one file by its path.
//!
//! The game stores its data in `gameN.tab`/`gameN.arc` pairs under `archives_win64` (the base game)
//! and `patch_win64` (replacements loaded in preference to the base copies). A `.tab` is a `TAB\0`
//! header (a version pair and an alignment) followed by entries of a name hash, an offset, and a
//! size into its `.arc`, which stores each file uncompressed. The name hash is
//! [`hashlittle`] of the file's path. The format and the precedence are those of the offline tooling
//! in `tools/sights/archives.py`, documented in `docs/engine/gameplay/weapon-data.md`.

use std::{
    collections::HashMap,
    fmt, fs,
    io::{self, Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
};

use crate::hash::hashlittle;

/// Every file in the base and patch archives, by name hash.
pub(crate) struct ArchiveIndex {
    arcs: Vec<PathBuf>,
    files: HashMap<u32, Location>,
}

impl ArchiveIndex {
    /// Index the archives under `game_dir`. A later archive's copy of a file replaces an earlier
    /// one: the patch tree's after the base tree's, and a higher-numbered archive's after a lower.
    pub(crate) fn open(game_dir: &Path) -> Result<Self, ArchiveError> {
        let mut index = ArchiveIndex {
            arcs: Vec::new(),
            files: HashMap::new(),
        };
        for tree in TREES {
            let dir = game_dir.join(tree);
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(source) => return Err(ArchiveError::Io { path: dir, source }),
            };
            let mut tabs: Vec<(u32, PathBuf)> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter_map(|path| Some((archive_number(&path)?, path)))
                .collect();
            tabs.sort();
            for (_, tab) in tabs {
                let bytes = fs::read(&tab).map_err(|source| ArchiveError::Io {
                    path: tab.clone(),
                    source,
                })?;
                let entries = parse_tab(&bytes).map_err(|reason| ArchiveError::Tab {
                    path: tab.clone(),
                    reason,
                })?;
                let arc = index.arcs.len();
                index.arcs.push(tab.with_extension("arc"));
                for entry in entries {
                    index.files.insert(
                        entry.name_hash,
                        Location {
                            arc,
                            offset: entry.offset,
                            size: entry.size,
                        },
                    );
                }
            }
        }
        if index.arcs.is_empty() {
            return Err(ArchiveError::NoArchives {
                game_dir: game_dir.to_path_buf(),
            });
        }
        Ok(index)
    }

    /// The number of archives indexed.
    pub(crate) fn archives(&self) -> usize {
        self.arcs.len()
    }

    /// The bytes of the file at `path` (as the game names it, for example
    /// `textures/ui/weapon_icons/301.ddsc`), or `None` when no archive holds it.
    pub(crate) fn read(&self, path: &str) -> Result<Option<Vec<u8>>, ArchiveError> {
        let Some(location) = self.files.get(&hashlittle(path.as_bytes())) else {
            return Ok(None);
        };
        let arc = &self.arcs[location.arc];
        let io_error = |source| ArchiveError::Io {
            path: arc.clone(),
            source,
        };
        let mut file = fs::File::open(arc).map_err(io_error)?;
        file.seek(SeekFrom::Start(u64::from(location.offset)))
            .map_err(io_error)?;
        let mut bytes = vec![0; location.size as usize];
        file.read_exact(&mut bytes).map_err(io_error)?;
        Ok(Some(bytes))
    }
}

/// Why the archives could not be read.
#[derive(Debug)]
pub(crate) enum ArchiveError {
    /// No `game*.tab` under either tree.
    NoArchives {
        game_dir: PathBuf,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Tab {
        path: PathBuf,
        reason: TabError,
    },
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveError::NoArchives { game_dir } => write!(
                f,
                "archives: no game*.tab under {}/{{{}}}",
                game_dir.display(),
                TREES.join(",")
            ),
            ArchiveError::Io { path, source } => {
                write!(f, "archives: could not read {}: {source}", path.display())
            }
            ArchiveError::Tab { path, reason } => {
                write!(f, "archives: {}: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for ArchiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ArchiveError::Io { source, .. } => Some(source),
            ArchiveError::NoArchives { .. } | ArchiveError::Tab { .. } => None,
        }
    }
}

/// Why a `.tab` could not be parsed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum TabError {
    /// Shorter than its header.
    Truncated,
    /// Not a `TAB\0` file of the version the game uses.
    NotATab,
}

impl fmt::Display for TabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TabError::Truncated => "the index is shorter than its header",
            TabError::NotATab => "not a version 2 TAB index",
        })
    }
}

/// One file in a `.tab`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct TabEntry {
    pub name_hash: u32,
    pub offset: u32,
    pub size: u32,
}

/// The entries of a `.tab`. A trailing partial entry is ignored, as the offline tooling does.
pub(crate) fn parse_tab(bytes: &[u8]) -> Result<impl Iterator<Item = TabEntry> + '_, TabError> {
    let header = bytes.get(..TAB_HEADER).ok_or(TabError::Truncated)?;
    if header[..4] != *b"TAB\0" || u16_at(header, 4) != 2 || u16_at(header, 6) != 1 {
        return Err(TabError::NotATab);
    }
    let (entries, _) = bytes[TAB_HEADER..].as_chunks::<TAB_ENTRY>();
    Ok(entries.iter().map(|entry| TabEntry {
        name_hash: u32_at(entry, 0),
        offset: u32_at(entry, 4),
        size: u32_at(entry, 8),
    }))
}

/// The archive trees, in ascending precedence.
const TREES: [&str; 2] = ["archives_win64", "patch_win64"];

/// The `.tab` header's size: magic, two version words, and the alignment.
const TAB_HEADER: usize = 12;

/// A `.tab` entry's size.
const TAB_ENTRY: usize = 12;

/// Where one file sits: its `.arc` (an index into [`ArchiveIndex::arcs`]) and its bytes there.
#[derive(Debug, Copy, Clone)]
struct Location {
    arc: usize,
    offset: u32,
    size: u32,
}

/// `N` for a `gameN.tab`.
fn archive_number(path: &Path) -> Option<u32> {
    if path.extension()? != "tab" {
        return None;
    }
    path.file_stem()?
        .to_str()?
        .strip_prefix("game")?
        .parse()
        .ok()
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_lists_its_entries_and_ignores_a_partial_one() {
        let mut bytes = b"TAB\0".to_vec();
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&16u32.to_le_bytes());
        for (hash, offset, size) in [(7u32, 0u32, 10u32), (9, 16, 4)] {
            bytes.extend_from_slice(&hash.to_le_bytes());
            bytes.extend_from_slice(&offset.to_le_bytes());
            bytes.extend_from_slice(&size.to_le_bytes());
        }
        bytes.extend_from_slice(&[1, 2, 3]);
        let entries: Vec<TabEntry> = parse_tab(&bytes).unwrap().collect();
        assert_eq!(
            entries,
            [
                TabEntry {
                    name_hash: 7,
                    offset: 0,
                    size: 10
                },
                TabEntry {
                    name_hash: 9,
                    offset: 16,
                    size: 4
                },
            ]
        );
    }

    #[test]
    fn a_tab_of_another_kind_is_refused() {
        assert_eq!(parse_tab(b"TAB\0").err(), Some(TabError::Truncated));
        let mut bytes = b"TAB\0".to_vec();
        bytes.extend_from_slice(&[3, 0, 1, 0, 0, 0, 0, 0]);
        assert_eq!(parse_tab(&bytes).err(), Some(TabError::NotATab));
    }

    #[test]
    fn only_numbered_game_tabs_are_archives() {
        assert_eq!(archive_number(Path::new("x/game12.tab")), Some(12));
        assert_eq!(archive_number(Path::new("x/game12.arc")), None);
        assert_eq!(archive_number(Path::new("x/other.tab")), None);
    }
}
