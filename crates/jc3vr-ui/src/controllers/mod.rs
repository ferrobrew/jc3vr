//! The controller pieces that belong to the UI layer: the pause menu's VR pages ([`menu`]), their
//! presets ([`presets`]), and the body page ([`body::menu`]). The controller subsystems live in the
//! gameplay crate.

pub mod body;
pub mod menu;
pub(crate) mod presets;
