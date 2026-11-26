//! Grapple comfort configuration. It lives with the head pose's tunables as `headpose.grapple`
//! ([`jc3vr_render::headpose::config::GrappleComfortConfig`]), which the head composition reads below the
//! grapple; these re-exports keep the grapple's own paths.

pub use jc3vr_render::headpose::config::{GrappleComfortConfig, GrappleComfortMode};
