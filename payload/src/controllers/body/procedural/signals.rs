//! The character-state signals the hand-over table reads that the character does not keep: the
//! grapple's ledge climb and the hit reactions. See `docs/engine/character/hands-locomotion-and-state.md`.
//!
//! - The ledge climb is the one the grapple ends in: the reeled-in ledge climb's motion task
//!   ([`NStateTask_ReeledInLedgeClimbMotionTask::Update`](jc3gi::equipment::grappling_hook::NStateTask_ReeledInLedgeClimbMotionTask::Update))
//!   runs on every tick of it, so [`note_ledge_climb`] stamps it from that task and the climb counts
//!   while the stamp is fresh. The game has no on-foot vault or obstacle climb for the player.
//! - A hit reaction is an act: [`Character::HitReaction`](jc3gi::character::character::Character::HitReaction)
//!   queues a stumble or a knock-back straight on the character's animated model
//!   ([`AnimatedModel::QueueAct`](jc3gi::character::character::AnimatedModel::QueueAct)), so
//!   [`note_act`] stamps the local player's from that queue. The character keeps no state for the
//!   reaction's length, so it counts for `hit_reaction_hold_s` after its act, renewed by the next.
//!
//! Both are logged at info level when they start.

use std::{
    ffi::CStr,
    mem::offset_of,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use jc3gi::{
    animation::symbol_table::EventIdSymbolTable,
    character::character::{AnimatedModel, Character},
};
use parking_lot::Mutex;

use crate::hooks::character::resolve_act_id;

/// Publish the local player's character, whose animated model's acts [`note_act`] watches. Called
/// from the solve gate on every pass.
pub(crate) fn set_local_character(character: &Character) {
    LOCAL_MODEL.store(
        character as *const Character as usize + offset_of!(Character, m_AnimatedModel),
        Ordering::Relaxed,
    );
}

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

/// Stamp a hit reaction when `act` is one queued on the local player's animated model `model`.
/// Called from the act queue, for every model.
pub(crate) fn note_act(model: *const AnimatedModel, act: i32, now: Instant) {
    let local = LOCAL_MODEL.load(Ordering::Relaxed);
    if local == 0 || model as usize != local {
        return;
    }
    let Some(ids) = hit_reaction_ids() else {
        return;
    };
    let Some(&(_, name)) = ids.iter().find(|(id, _)| *id == act) else {
        return;
    };
    STATE.lock().hit_reaction_at = Some(now);
    tracing::info!(
        target: "body",
        act = %name.to_string_lossy(),
        "procedural body: a hit reaction starts",
    );
}

/// Whether the grapple's ledge climb ran within the last tick or two.
pub fn in_ledge_climb(now: Instant) -> bool {
    STATE
        .lock()
        .ledge_climb_at
        .is_some_and(|at| now.saturating_duration_since(at) <= TASK_GAP)
}

/// Whether a hit reaction's act was queued within the last `hold_s` seconds.
pub fn in_hit_reaction(now: Instant, hold_s: f32) -> bool {
    let hold = Duration::try_from_secs_f32(hold_s.max(0.0)).unwrap_or_default();
    STATE
        .lock()
        .hit_reaction_at
        .is_some_and(|at| now.saturating_duration_since(at) <= hold)
}

/// How long after the climb's task last ran it still counts as running: a tick or two.
const TASK_GAP: Duration = Duration::from_millis(100);

/// The acts that start a hit reaction the animation plays out: the stumbles and the knock-back.
/// The flinch is a partial the body keeps.
const HIT_REACTION_ACTS: [&CStr; 4] = [
    EventIdSymbolTable::ACT_HITREACT_STUMBLE,
    EventIdSymbolTable::ACT_HITREACT_STUMBLE_BULLET,
    EventIdSymbolTable::ACT_HITREACT_STUMBLE_MELEE,
    EventIdSymbolTable::ACT_HITREACT_FLY,
];

struct State {
    ledge_climb_at: Option<Instant>,
    hit_reaction_at: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    ledge_climb_at: None,
    hit_reaction_at: None,
});

static LOCAL_MODEL: AtomicUsize = AtomicUsize::new(0);

/// The runtime ids of [`HIT_REACTION_ACTS`] with their names, resolved once the act table is live.
fn hit_reaction_ids() -> Option<&'static [(i32, &'static CStr); 4]> {
    if let Some(ids) = HIT_REACTION_IDS.get() {
        return Some(ids);
    }
    let mut ids = [(0, HIT_REACTION_ACTS[0]); 4];
    for (slot, name) in ids.iter_mut().zip(HIT_REACTION_ACTS) {
        *slot = (resolve_act_id(name)?, name);
    }
    Some(HIT_REACTION_IDS.get_or_init(|| ids))
}

static HIT_REACTION_IDS: OnceLock<[(i32, &'static CStr); 4]> = OnceLock::new();
