//! The controls summary's presentation: the glyphs, the grid layout, the wrist glance, the page
//! clock, and the game's own hint glyphs. The summary's model lives in the gameplay crate's
//! `controllers::summary`, and the panel that draws it in [`crate::hud::summary_panel`].

pub(crate) mod game_hints;
pub mod glance;
#[cfg(test)]
mod glyphs;
pub mod grid;
pub(crate) mod page_clock;
