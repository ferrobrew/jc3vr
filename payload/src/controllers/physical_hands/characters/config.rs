//! The character grabs' settings.

use serde::{Deserialize, Serialize};

/// Taking hold of characters with an empty hand (see
/// [`crate::controllers::physical_hands::characters`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharactersConfig {
    /// Take hold of characters on foot: hold on, shove, yank them off their feet, and drag and
    /// throw them by their ragdoll.
    pub grab: bool,
    /// Throw a standing character into their ragdoll as the grip takes hold of them, held by the
    /// bone nearest the palm, rather than holding on until a yank.
    pub topple: bool,
    /// Pull a character out of a vehicle seat through an open door: grip them, then yank.
    pub hijack: bool,
}

impl CharactersConfig {
    pub const fn new() -> Self {
        Self {
            grab: true,
            topple: true,
            hijack: true,
        }
    }
}

impl Default for CharactersConfig {
    fn default() -> Self {
        Self::new()
    }
}
