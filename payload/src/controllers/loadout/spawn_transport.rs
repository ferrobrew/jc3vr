//! The closed controller gate for source-member spawn transport.
//!
//! No callback, detour, source provider, or native engine implementation is installed here. The
//! controlled executor reuses `lifecycle_source::RequestBook` in tests. Native activation requires
//! verified strong-reference wrappers, a bounded complete-group proof, ranking-job quiescence,
//! callback retirement, and reset ordering. Source qualification, ammunition, and staging remain
//! unavailable independently of transport progress. The gate has no production surface, so the
//! whole module is compiled for its tests alone.

use std::fmt;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Status {
    pub admission: Admission,
    pub native_callbacks: bool,
    pub source_qualification: bool,
    pub ammunition: bool,
    pub staging: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Admission {
    Closed,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct AdmissionClosed;

impl fmt::Display for AdmissionClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("loadout spawn transport: production admission is closed")
    }
}

impl std::error::Error for AdmissionClosed {}

/// This gate has no production enable operation. A refusal leaves ordinary gameplay untouched.
#[derive(Debug, Default)]
pub(crate) struct Controller;

impl Controller {
    pub fn status(&self) -> Status {
        Status {
            admission: Admission::Closed,
            native_callbacks: false,
            source_qualification: false,
            ammunition: false,
            staging: false,
        }
    }

    pub fn request(&self) -> Result<std::convert::Infallible, AdmissionClosed> {
        Err(AdmissionClosed)
    }
}

pub(crate) fn controller() -> &'static Controller {
    &CONTROLLER
}

static CONTROLLER: Controller = Controller;

#[path = "spawn_transport/ownership.rs"]
mod ownership;

#[path = "spawn_transport/controlled.rs"]
mod controlled;

#[path = "spawn_transport/tests.rs"]
mod tests;
