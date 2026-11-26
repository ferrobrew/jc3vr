//! The controllers live in the gameplay crate and are re-exported here; the children that belong to
//! other layers stay beside them until their cuts: the map ([`map`]) and the summary's presentation
//! (in [`summary`]) to the HUD, the pause menu pages ([`menu`], [`presets`], and the body page in
//! [`body`]) to the UI, and the input crate's frame through [`frame`].

pub(crate) use jc3vr_gameplay::controllers::*;

pub mod body;
pub mod frame;
pub mod map;
pub(crate) mod menu;
pub mod presets;
pub mod summary;
