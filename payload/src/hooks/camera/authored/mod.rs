//! Availability of the authored-camera adapter and its test-only composition contract.
//!
//! Native channel identity, baseline/root semantics, and serialized reader ownership are not
//! established. The pure model has no production capture or activation path.

#[cfg(test)]
mod composition;
#[cfg(test)]
mod policy;
#[cfg(test)]
mod tests;

/// Production availability does not depend on a candidate state bit or asset curve presence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Unavailable,
}

/// The native adapter remains unavailable until its source and consumers are verified together.
pub(crate) const fn status() -> Status {
    Status::Unavailable
}
