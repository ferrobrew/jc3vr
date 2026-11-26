//! The summary's model lives in the gameplay crate and is re-exported here; its presentation (the
//! glyphs, the grid, the glance, the page clock, and the game's hint glyphs) belongs to the HUD
//! layer and stays beside it until that cut.

pub(crate) use jc3vr_gameplay::controllers::summary::*;

pub mod game_hints;
pub mod glance;
#[cfg(test)]
mod glyphs;
pub(crate) mod grid;
pub(crate) mod page_clock;
