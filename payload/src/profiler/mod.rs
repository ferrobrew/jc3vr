//! The in-game profiler's halves above the core: [`gpu`], the render half (the timestamp lane over
//! the draw thread's dispatches and the render-pass scopes), and [`ui`], the Performance tab's
//! panel. The core (the scopes, the frame advance, and the trace capture) lives in the core crate
//! and is re-exported here, and [`gpu`] lives in the render crate behind a shim at its old path.

pub(crate) use jc3vr_core::profiler::*;

pub mod gpu;
pub mod ui;
