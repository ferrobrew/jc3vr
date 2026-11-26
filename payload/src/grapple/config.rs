//! Grapple comfort configuration. It lives with the head pose's tunables as `headpose.grapple`
//! ([`crate::headpose::config::GrappleComfortConfig`]), which the head composition reads below the
//! grapple; these re-exports keep the grapple's own paths.

pub use crate::headpose::config::{GrappleComfortConfig, GrappleComfortMode};
