//! Debug subsystems: machinery (no egui) backing the debug UI, such as the render trace
//! ([`trace`]); their egui surfaces live in the UI crate.

pub mod texture_census;
pub mod trace;
