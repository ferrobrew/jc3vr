//! The controllers live in the gameplay crate and are re-exported here; the controls map ([`map`])
//! lives in the HUD crate and is re-exported beside them through a shim file. The children that
//! belong to the UI layer stay here until its cut: the pause menu pages ([`menu`], [`presets`], and
//! the body page in [`body`]). The input crate's frame is reached through [`frame`].

pub(crate) use jc3vr_gameplay::controllers::*;

pub mod body;
pub mod frame;
pub mod map;
pub(crate) mod menu;
pub mod presets;
