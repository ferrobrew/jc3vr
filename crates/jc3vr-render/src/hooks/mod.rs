//! The render layer's engine-pipeline detours, grouped by the engine system they hook. The hook
//! plumbing they install through lives in the core crate.

pub mod camera;
pub mod clock;
pub mod draw_count;
pub mod graphics_engine;
#[cfg(feature = "profiler")]
pub mod profiler;
pub mod window_focus;
