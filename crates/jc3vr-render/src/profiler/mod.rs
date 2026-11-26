//! The profiler's render half: [`gpu`], the timestamp lane over the draw thread's dispatches and
//! the render-pass scopes, with the `profiler` feature, and [`gpu_samples`], what the lane reports
//! of each dispatch to a recording run, in every build. The profiler core lives in the core crate.

#[cfg(feature = "profiler")]
pub mod gpu;
pub mod gpu_samples;
