//! The error a settings check returns, which the tolerant load reads to reset the setting it names
//! ([`crate::config::engine::load::reset_named`]).

use std::fmt;

/// A setting that is well-typed but out of range or inconsistent with another. The message leads
/// with the setting's path, relative to the checked subtree, which the tolerant load resets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invalid(pub String);

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid setting: {}", self.0)
    }
}

impl std::error::Error for Invalid {}
