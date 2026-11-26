//! Values one stage of the frame publishes for another, and the small concurrency pieces that go
//! with them: the shapes the layers above otherwise write by hand at each site.
//!
//! - [`Published`]: the latest value a writer stage published, with the time it was published,
//!   read whole by any thread, and current only within a [`Window`]. It replaces the
//!   `Mutex<Option<(T, Instant)>>` slots and the `fn fresh` that each reader wrote beside one.
//! - [`Stamp`] and [`Step`], the two clocks a publication is stamped on: [`Stamp`] is wall time
//!   from the process epoch the frame timing counts from, and [`Step`] counts the physics steps,
//!   for a publication whose staleness is measured in steps so that a hitch of the whole
//!   simulation does not age it. Each clock has its window, [`Window`] and [`StepWindow`], built
//!   from a tuning value.
//! - [`Counter`] and [`counters!`](crate::counters): event counts for a readout, declared once as
//!   a live struct and a snapshot struct with the same fields.
//! - [`EventLog`]: the last `N` events for a readout, newest last, in a fixed array.
//! - [`ReentryGuard`]: a thread-local cell set for the extent of a call and restored after it, for
//!   a detour that must know whether the mod's own call, or an outer engine call, is on the stack.
//! - [`DetourQueue`]: items a detour hands, from any thread, to a later serial point that drains
//!   them.
//!
//! None of these allocate on a read, and a [`Published`] read costs one uncontended leaf-lock
//! acquisition and a copy, as the hand-written slots it replaces did.

mod clock;
mod counters;
mod event_log;
mod queue;
mod reentry;
mod slot;

pub use crate::published::{
    clock::{Clock, Stamp, Step, StepWindow, Window},
    counters::Counter,
    event_log::EventLog,
    queue::DetourQueue,
    reentry::ReentryGuard,
    slot::{Published, Stage, Stamped},
};
