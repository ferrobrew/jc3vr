//! The profiler's engine-phase detours live in the render crate and are re-exported here; the old path keeps working until the
//! final cleanup pass.

pub(crate) use jc3vr_render::hooks::profiler::*;
