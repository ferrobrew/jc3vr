//! The character-state signals the hand-over table reads that the character does not keep. See
//! `docs/engine/character/hands-locomotion-and-state.md`.
//!
//! The ledge climb is the one the grapple ends in: the reeled-in ledge climb's motion task
//! ([`NStateTask_ReeledInLedgeClimbMotionTask::Update`](jc3gi::equipment::grappling_hook::NStateTask_ReeledInLedgeClimbMotionTask::Update))
//! runs on every tick of it, so [`note_ledge_climb`] stamps it from that task and the climb counts
//! while the stamp is fresh. The game has no on-foot vault or obstacle climb for the player. The
//! climb's start is logged at info level.

use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Stamp a tick of the local player's ledge climb. Called from the climb's motion task.
pub(crate) fn note_ledge_climb(now: Instant) {
    let mut state = STATE.lock();
    let started = state
        .ledge_climb_at
        .is_none_or(|at| now.saturating_duration_since(at) > TASK_GAP);
    state.ledge_climb_at = Some(now);
    drop(state);
    if started {
        tracing::info!(target: "body", "procedural body: the grapple's ledge climb starts");
    }
}

/// Whether the grapple's ledge climb ran within the last tick or two.
pub fn in_ledge_climb(now: Instant) -> bool {
    STATE
        .lock()
        .ledge_climb_at
        .is_some_and(|at| now.saturating_duration_since(at) <= TASK_GAP)
}

/// How long after the climb's task last ran it still counts as running: a tick or two.
const TASK_GAP: Duration = Duration::from_millis(100);

struct State {
    ledge_climb_at: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    ledge_climb_at: None,
});
