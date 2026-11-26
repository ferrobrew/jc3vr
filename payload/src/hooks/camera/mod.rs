//! The camera detours live in the render crate and are re-exported here; [`height`], the stance
//! height the camera adjust applies, belongs to the gameplay layer and stays beside them until its
//! cut.

pub(crate) use jc3vr_render::hooks::camera::*;

pub mod height;
