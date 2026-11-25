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
//! ([`crate::config::paths`]), so a shot's framing survives a relaunch. The stored transform is the character's full world matrix, so the facing comes back
//! too and no axis convention is assumed.

use std::path::PathBuf;

use jc3gi::{character::character::Character, game_world::GameWorld, types::math::Matrix4};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

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
    /// Instant within [`INSTANT_RANGE_M`] of the character, deferred with streaming beyond it.
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

/// Save the player's current transform under `name`, replacing a bookmark of the same name.
pub fn save_current(name: &str) -> bool {
    let Some(transform) = player_transform() else {
        tracing::warn!("director: no player character to bookmark");
        return false;
    };
    let name = name.trim();
    let name = if name.is_empty() {
        format!("bookmark {}", crate::session::stamp())
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
pub fn request_transform(transform: [f32; 16], mode: TeleportMode) {
    *PENDING.lock() = Some(Request { transform, mode });
}

/// The player's current world position, for the debug tab.
pub fn player_position() -> Option<[f32; 3]> {
    player_transform().map(|t| [t[12], t[13], t[14]])
}

/// Whether a jump is queued or the game world is in the middle of one. Main thread.
pub(crate) fn in_flight() -> bool {
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
            TeleportMode::Auto => distance <= INSTANT_RANGE_M,
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

/// The farthest an instant jump reaches before the streamed path is used. The engine warns about a
/// teleport without the load state past 500 m from the camera; this leaves a margin for the
/// character standing away from the camera.
pub const INSTANT_RANGE_M: f32 = 400.0;

struct Request {
    transform: [f32; 16],
    mode: TeleportMode,
}

static PENDING: Mutex<Option<Request>> = Mutex::new(None);

static STATE: Mutex<Option<State>> = Mutex::new(None);

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    bookmarks: Vec<Bookmark>,
}
impl State {
    fn load() -> Self {
        let Some(path) = path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!(
                    "director: could not parse the bookmarks at {}: {e}",
                    path.display()
                );
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    fn save(&self) {
        let Some(path) = path() else {
            return;
        };
        match toml::to_string(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
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
    crate::config::paths::file("teleports.toml")
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
