//! Registers the directing tools as the source of a tracking recording's start marker: the
//! player's transform, and the teleport bookmarks a marker near one is named after.

use jc3vr_render::recorder::tracking::{NamedPlace, StartSource, register_start_source};

use crate::director::teleport;

/// Register [`teleport`] as the start marker's source. Call once, before the first recording.
pub fn register() {
    register_start_source(StartSource {
        player_transform: teleport::player_transform,
        places: bookmark_places,
    });
}

/// The teleport bookmarks, as named places.
fn bookmark_places() -> Vec<NamedPlace> {
    teleport::bookmarks()
        .into_iter()
        .map(|bookmark| NamedPlace {
            position: bookmark.position(),
            name: bookmark.name,
        })
        .collect()
}
