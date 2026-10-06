//! The procedural body: Rico's skeleton posed by the mod from the player's tracked head and body,
//! rather than by the game's animation. See `docs/mod/body/procedural-body.md`.
//!
//! - [`skeleton`]: reading the skeleton's bones, hierarchy, and reference pose, and logging it once
//!   per session.

pub mod skeleton;
