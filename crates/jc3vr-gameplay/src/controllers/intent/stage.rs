//! Whether the bindings' game stage may spawn objects that register for the world's updates, as the
//! stage sets it on its thread for the acts it runs.
//!
//! The game stage runs at the top of the world's pre-simulation queue, after the update-queue
//! commit, whenever the queue runs, and elsewhere (a paused update, or an inline stage) with
//! spawning refused. The reasons are with the stage (`controllers::bindings::stage`); the acts that
//! spawn such objects check [`spawn`].

use std::cell::Cell;

/// Whether the game stage may spawn objects that register for the world's updates.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Spawn {
    /// The stage runs at the top of the world's pre-simulation queue, after the update-queue
    /// commit: what it registers is first updated in the next tick.
    Allowed,
    /// The stage runs anywhere else: a registration would be made live by the next pre-simulation
    /// commit, ahead of the ammunition manager's pass that completes an explosive.
    Refused,
}

/// Whether the game stage running on this thread may spawn objects that register for the world's
/// updates; [`Spawn::Refused`] outside a game stage.
pub(crate) fn spawn() -> Spawn {
    SPAWN.with(Cell::get)
}

/// Holds this thread's [`spawn`] answer for a game stage, and restores the previous one when
/// dropped.
pub(crate) struct SpawnScope {
    previous: Spawn,
}

impl SpawnScope {
    /// Answer [`spawn`] with `spawn` on this thread until the scope drops.
    pub(crate) fn enter(spawn: Spawn) -> Self {
        Self {
            previous: SPAWN.with(|cell| cell.replace(spawn)),
        }
    }
}

impl Drop for SpawnScope {
    fn drop(&mut self) {
        SPAWN.with(|cell| cell.set(self.previous));
    }
}

thread_local! {
    static SPAWN: Cell<Spawn> = const { Cell::new(Spawn::Refused) };
}
