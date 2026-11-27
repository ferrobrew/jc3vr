//! Teleport bookmarks: save the player's transform under a name, jump back to it later.
//!
//! The jump goes through the game world's own teleport entry points rather than a transform
//! write: the character's transform is owned by its physics proxy, and the engine's teleport also
//! forces the neutral animation state, resets the camera behind the new transform, and fires the
//! events the HUD and the streaming systems listen for. Two variants exist. The instant one places
//! the character synchronously and needs the destination resident; the deferred one goes through
//! the load state so the destination streams in first. [`TeleportMode::Auto`] picks by distance
//! against the engine's own "too far without load state" threshold.
//!
//! Bookmarks persist in `teleports.toml` in the configuration directory
//! ([`jc3vr_core::config::engine::paths`]), so a shot's framing survives a relaunch. The stored transform is the character's full world matrix, so the facing comes back
//! too and no axis convention is assumed.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use jc3gi::{character::character::Character, game_world::GameWorld, types::math::Matrix4};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::tuning::TOOLING;

/// A saved player transform.
#[derive(Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub name: String,
    /// The character's world matrix, row-major, translation in elements 12 to 14.
    pub transform: [f32; 16],
}
impl Bookmark {
    /// The bookmark's world position.
    pub fn position(&self) -> [f32; 3] {
        [self.transform[12], self.transform[13], self.transform[14]]
    }
}

/// How a jump reaches its destination.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TeleportMode {
    /// Instant within the tuning's
    /// [`instant_teleport_range_m`](crate::tuning::DirectorTuning::instant_teleport_range_m) of the
    /// character, deferred with streaming beyond it.
    Auto,
    /// Place the character synchronously. The destination must already be streamed in.
    Instant,
    /// Enter the load state so the destination streams in, then place the character.
    Streamed,
}

/// The persisted bookmark list.
pub fn bookmarks() -> Vec<Bookmark> {
    STATE
        .lock()
        .get_or_insert_with(State::load)
        .bookmarks
        .clone()
}

/// Why the bookmarks file could not be loaded, if it could not. While it could not, the file is left
/// untouched and changes to the bookmarks last only for the session.
pub fn load_error() -> Option<BookmarksError> {
    STATE
        .lock()
        .get_or_insert_with(State::load)
        .load_error
        .clone()
}

/// A bookmarks file that exists but could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookmarksError {
    pub path: PathBuf,
    pub kind: BookmarksErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookmarksErrorKind {
    /// The file could not be read.
    Read(String),
    /// The file is not TOML, or not the bookmarks' shape.
    Parse(String),
}

impl fmt::Display for BookmarksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        match &self.kind {
            BookmarksErrorKind::Read(why) => {
                write!(f, "director: bookmarks: could not read {path}: {why}")
            }
            BookmarksErrorKind::Parse(why) => {
                write!(f, "director: bookmarks: could not parse {path}: {why}")
            }
        }
    }
}

impl std::error::Error for BookmarksError {}

/// Save the player's current transform under `name`, replacing a bookmark of the same name.
pub fn save_current(name: &str) -> bool {
    let Some(transform) = player_transform() else {
        tracing::warn!("director: no player character to bookmark");
        return false;
    };
    let name = name.trim();
    let name = if name.is_empty() {
        format!("bookmark {}", jc3vr_core::session::stamp())
    } else {
        name.to_owned()
    };
    let mut state = STATE.lock();
    let state = state.get_or_insert_with(State::load);
    state.bookmarks.retain(|b| b.name != name);
    state.bookmarks.push(Bookmark {
        name: name.clone(),
        transform,
    });
    state.save();
    tracing::info!("director: bookmark saved: {name}");
    true
}

/// Remove the bookmark at `index`.
pub fn remove(index: usize) {
    let mut state = STATE.lock();
    let state = state.get_or_insert_with(State::load);
    if index < state.bookmarks.len() {
        let removed = state.bookmarks.remove(index);
        state.save();
        tracing::info!("director: bookmark removed: {}", removed.name);
    }
}

/// Queue a jump to the bookmark at `index`; serviced on the next main-thread update.
pub fn request_bookmark(index: usize, mode: TeleportMode) {
    let bookmark = STATE
        .lock()
        .get_or_insert_with(State::load)
        .bookmarks
        .get(index)
        .cloned();
    match bookmark {
        Some(b) => request_transform(b.transform, mode),
        None => tracing::warn!("director: no bookmark at index {index}"),
    }
}

/// Queue a jump to `position`, keeping the character's current facing.
pub fn request_position(position: [f32; 3], mode: TeleportMode) {
    let mut transform = player_transform().unwrap_or(Matrix4::IDENTITY.data);
    transform[12..15].copy_from_slice(&position);
    request_transform(transform, mode);
}

/// Queue a jump to a full transform.
pub(crate) fn request_transform(transform: [f32; 16], mode: TeleportMode) {
    *PENDING.lock() = Some(Request { transform, mode });
}

/// The player's current world position, for the debug tab.
pub fn player_position() -> Option<[f32; 3]> {
    player_transform().map(|t| [t[12], t[13], t[14]])
}

/// Whether a jump is queued or the game world is in the middle of one. Main thread.
pub fn in_flight() -> bool {
    if PENDING.lock().is_some() {
        return true;
    }
    // SAFETY: the game world singleton is main-thread data, read from the main thread.
    unsafe { GameWorld::get().is_some_and(|world| world.IsTeleporting()) }
}

/// Run the queued jump, if any. Main thread: both teleport entry points assert it.
pub(super) fn service_pending() {
    let Some(request) = PENDING.lock().take() else {
        return;
    };
    // SAFETY: the game world singleton and the player character are main-thread data, and this
    // runs from the main-thread update.
    unsafe {
        let Some(world) = GameWorld::get() else {
            tracing::warn!("director: teleport dropped: no game world");
            return;
        };
        if world.IsTeleporting() {
            tracing::warn!("director: teleport dropped: one is already in flight");
            return;
        }
        let Some(from) = player_position() else {
            tracing::warn!("director: teleport dropped: no player character");
            return;
        };
        let to = &request.transform[12..15];
        let distance =
            ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2) + (to[2] - from[2]).powi(2))
                .sqrt();
        let instant = match request.mode {
            TeleportMode::Instant => true,
            TeleportMode::Streamed => false,
            TeleportMode::Auto => distance <= TOOLING.get().director.instant_teleport_range_m,
        };
        let matrix = Matrix4 {
            data: request.transform,
        };
        tracing::info!(
            "director: teleport {:.0} m to ({:.1}, {:.1}, {:.1}), {}",
            distance,
            to[0],
            to[1],
            to[2],
            if instant { "instant" } else { "streamed" }
        );
        if instant {
            world.TeleportPlayerInstant(
                &matrix,
                std::ptr::null(),
                GameWorld::TELEPORT_FLAG_KEEP_MISSION,
            );
        } else {
            world.TeleportPlayer(
                &matrix,
                std::ptr::null(),
                true,
                false,
                0.0,
                0.0,
                GameWorld::TELEPORT_FLAG_KEEP_MISSION,
                std::ptr::null_mut(),
                false,
            );
        }
    }
}

struct Request {
    transform: [f32; 16],
    mode: TeleportMode,
}

static PENDING: Mutex<Option<Request>> = Mutex::new(None);

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// The bookmarks in memory, and why the file could not be loaded, if it could not.
#[derive(Default)]
struct State {
    bookmarks: Vec<Bookmark>,
    /// Set when the file exists but could not be read or parsed. While it is, the file is never
    /// written: the in-memory list is not what the file holds, and a save would replace bookmarks
    /// that are still recoverable from it.
    load_error: Option<BookmarksError>,
}

/// The bookmarks file's contents.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct BookmarkFile {
    bookmarks: Vec<Bookmark>,
}

impl State {
    fn load() -> Self {
        match path() {
            Some(path) => Self::load_from(&path),
            None => Self::default(),
        }
    }

    /// Load the bookmarks at `path`. A missing file is an empty list; an unreadable or unparsable
    /// one is an empty list that refuses to save.
    fn load_from(path: &Path) -> Self {
        let parsed = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str::<BookmarkFile>(&text).map_err(|error| BookmarksError {
                path: path.to_owned(),
                kind: BookmarksErrorKind::Parse(error.to_string()),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(BookmarkFile::default())
            }
            Err(error) => Err(BookmarksError {
                path: path.to_owned(),
                kind: BookmarksErrorKind::Read(error.to_string()),
            }),
        };
        match parsed {
            Ok(file) => Self {
                bookmarks: file.bookmarks,
                load_error: None,
            },
            Err(error) => {
                tracing::warn!("{error}; the file is left untouched");
                Self {
                    bookmarks: Vec::new(),
                    load_error: Some(error),
                }
            }
        }
    }

    fn save(&self) {
        if let Some(path) = path() {
            self.save_to(&path);
        }
    }

    fn save_to(&self, path: &Path) {
        if let Some(error) = &self.load_error {
            tracing::warn!("{error}; the change is kept for this session and not saved");
            return;
        }
        let file = BookmarkFile {
            bookmarks: self.bookmarks.clone(),
        };
        match toml::to_string(&file) {
            Ok(text) => {
                if let Err(e) = std::fs::write(path, text) {
                    tracing::warn!(
                        "director: could not write the bookmarks to {}: {e}",
                        path.display()
                    );
                }
            }
            Err(e) => tracing::warn!("director: could not serialize the bookmarks: {e}"),
        }
    }
}

/// The bookmarks file, in the configuration directory beside the settings.
fn path() -> Option<PathBuf> {
    jc3vr_core::config::engine::paths::file("teleports.toml")
}

/// The player's current world matrix (`m_WorldMatrixT1`, what `GetTransform` returns). Main
/// thread.
pub(crate) fn player_transform() -> Option<[f32; 16]> {
    // SAFETY: read-only access to game-thread data from the main thread.
    unsafe {
        Character::GetLocalPlayerCharacter()
            .as_ref()
            .map(|c| c.m_WorldMatrixT1.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jc3vr-teleport-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    fn bookmark(name: &str) -> Bookmark {
        let mut transform = Matrix4::IDENTITY.data;
        transform[12..15].copy_from_slice(&[1.0, 2.0, 3.0]);
        Bookmark {
            name: name.to_owned(),
            transform,
        }
    }

    #[test]
    fn a_missing_file_is_an_empty_list_that_saves() {
        let dir = scratch_dir("missing");
        let path = dir.join("teleports.toml");
        let mut state = State::load_from(&path);
        assert!(state.bookmarks.is_empty());
        assert_eq!(state.load_error, None);
        state.bookmarks.push(bookmark("first"));
        state.save_to(&path);
        let reloaded = State::load_from(&path);
        assert_eq!(reloaded.bookmarks.len(), 1);
        assert_eq!(reloaded.bookmarks[0].name, "first");
        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn an_unparsable_file_is_reported_and_never_overwritten() {
        let dir = scratch_dir("unparsable");
        let path = dir.join("teleports.toml");
        let original = "[[bookmarks]]\nname = \"kept\"\ntransform = [1.0, 2.0\n";
        std::fs::write(&path, original).expect("write the broken file");
        let mut state = State::load_from(&path);
        assert!(state.bookmarks.is_empty());
        let error = state.load_error.clone().expect("a parse error");
        assert!(matches!(error.kind, BookmarksErrorKind::Parse(_)));
        assert!(
            error
                .to_string()
                .starts_with("director: bookmarks: could not parse ")
        );
        state.bookmarks.push(bookmark("new"));
        state.save_to(&path);
        assert_eq!(std::fs::read_to_string(&path).expect("reread"), original);
        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn an_unreadable_file_is_reported_and_never_overwritten() {
        let dir = scratch_dir("unreadable");
        // A directory where the file should be cannot be read as text.
        let path = dir.join("teleports.toml");
        std::fs::create_dir(&path).expect("directory in the file's place");
        let state = State::load_from(&path);
        let error = state.load_error.clone().expect("a read error");
        assert!(matches!(error.kind, BookmarksErrorKind::Read(_)));
        state.save_to(&path);
        assert!(path.is_dir());
        drop(std::fs::remove_dir_all(&dir));
    }
}
