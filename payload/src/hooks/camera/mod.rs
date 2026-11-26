//! The camera detours live in the render crate and are re-exported here; [`height`], the stance
//! height the camera adjust applies, lives in the gameplay crate and is re-exported beside them
//! until the final cleanup pass.

pub(crate) use jc3vr_render::hooks::camera::*;

pub mod height;
