//! The source of a recording's start marker: where the player stands, and the named places the
//! marker may be labelled with. The directing tools own both, so they register the source at
//! install ([`register_start_source`]) rather than the recorder naming them.

use std::sync::OnceLock;

/// Where the player stands and the places the player named, for a recording's start marker.
pub(crate) struct StartSource {
    /// The player character's world matrix, row-major with the translation in elements 12 to 14,
    /// or `None` without a player character. Called on the game thread.
    pub(crate) player_transform: fn() -> Option<[f32; 16]>,
    /// The named places a start marker near one is labelled with.
    pub(crate) places: fn() -> Vec<NamedPlace>,
}

/// A place the player named, at a world position.
pub(crate) struct NamedPlace {
    pub(crate) name: String,
    pub(crate) position: [f32; 3],
}

/// Register the start marker's source. One source: a second registration is refused and logged.
pub(crate) fn register_start_source(source: StartSource) {
    if SOURCE.set(source).is_err() {
        tracing::error!(
            "recorder: tracking: a start source is already registered; keeping the first"
        );
    }
}

/// The player character's world matrix, or `None` without a player character or a source.
pub(super) fn player_transform() -> Option<[f32; 16]> {
    SOURCE.get().and_then(|source| (source.player_transform)())
}

/// The named places, or none without a source.
pub(super) fn places() -> Vec<NamedPlace> {
    SOURCE
        .get()
        .map_or_else(Vec::new, |source| (source.places)())
}

static SOURCE: OnceLock<StartSource> = OnceLock::new();
