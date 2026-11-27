//! The controls summary's presentation: the grid layout, the wrist glance, the page clock, and the
//! hiding of the game's own context hints (`glyphs`, the button glyphs, is compiled only for its
//! tests). The summary's model lives in the gameplay crate's
//! `controllers::summary`, and the panel that draws it in [`crate::hud::summary_panel`].

pub(crate) mod game_hints;
pub mod glance;
#[cfg(test)]
mod glyphs;
pub mod grid;
pub(crate) mod page_clock;
