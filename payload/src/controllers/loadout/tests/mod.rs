//! The loadout core's tests, split by concern: the transitions and their refusals, the socket
//! grab's decision, the take's route, the slot projection, the spawn reset and the caches, and the
//! invariants under arbitrary sequences. The shared fixtures and the invariant check are in [`fixtures`].

mod fixtures;
mod grab;
mod projection;
mod properties;
mod reset;
mod take;
mod transitions;
