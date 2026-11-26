//! Debug subsystems: machinery (no egui) backing the debug UI. The render trace ([`trace`]) and the
//! coordinate-frame probe it records ([`coord_frame`]); their egui surfaces live in the UI crate.

pub mod coord_frame;
pub mod register_unpatch;
pub mod register_view;
pub mod texture_census;
pub mod trace;
