//! The controller pieces that belong to the HUD layer: the controls map ([`map`]), which drives the
//! game's map screen from the controllers, and the summary's presentation ([`summary`]). The
//! controller subsystems, including the summary's model, live in the gameplay crate.

pub mod map;
pub mod summary;
