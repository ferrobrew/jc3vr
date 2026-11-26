//! The upscaler's settings and the per-frame sub-pixel jitter, which the frame state reads. The
//! upscaler itself lives in the render layer's `fsr` module.

mod config;
pub mod jitter;

pub use config::{AntiAliasing, FSR_CONFIG, FsrConfig, TaaConfig};
