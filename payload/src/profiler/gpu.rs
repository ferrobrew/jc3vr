//! The GPU timestamp lane lives in the render crate and is re-exported here; the old path keeps
//! working until the final cleanup pass.

pub(crate) use jc3vr_render::profiler::gpu::*;
