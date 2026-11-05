//! The input tick's two stages and the handoff between them.
//!
//! In play the engine runs its input update in the input job (`RunInputThread_Proxy`), which
//! overlaps game-thread work that mutates the world: effect physics models, AI path requests, the
//! traffic and ambient-spawn starts, task continuations, and the character manager's
//! pre-simulation step. The engine keeps the two apart by partition, so the job may touch only input
//! and UI state. The input tick is split along the same line:
//!
//! - the poll stage ([`poll_stage`]) runs in the device poll's post-call detour
//!   ([`crate::hooks::input::look`]). It lands the action writes the last game stage produced, right
//!   after the device poll, where every reader of the action maps sees them (the UI's input update
//!   later in the same job reads the pause and commlink actions from the player map, and the
//!   challenge update reads the player map before the world update), and hands the tick to the game
//!   stage;
//! - the game stage ([`game_stage`]) runs on the game thread at the top of the world's
//!   pre-simulation queue ([`crate::hooks::sim`]), after the input job's completion wait and
//!   before any character, weapon, or vehicle update and the physics step. It runs the bindings'
//!   decisions and every world-mutating act (weapons, inventory, the snatch, the physical
//!   grapple, the physical hands' pickups and stunts, the vehicles), and the game's
//!   active-input-device pin, and produces the writes the next poll stage lands.
//!
//! The top of that queue is after the world's update-queue commit
//! ([`UpdateQueue::CommitAddRemove`](jc3gi::update_queue::UpdateQueue::CommitAddRemove)), where
//! the world's pre-simulation update makes the updates registered since the last commit live
//! ([`PreSimTurn`]). An object the stage spawns there joins the queue at the next commit, at the
//! end of the render update, so its first updates are the next tick's, as for an object the
//! engine's own updates spawn. Spawned before the commit, an explosive's post-simulation update
//! would run in the same tick, before the ammunition manager's pass at the end of the tick has
//! made its physics body, and a grenade's dereferences that body
//! ([`Grenade::UpdatePostSim`](jc3gi::equipment::explosive::Grenade::UpdatePostSim)). The stage
//! therefore runs there whenever the queue runs, and elsewhere (a paused update, which skips the
//! queue and the ammunition manager's pass, or an inline game stage) it runs with [`Spawn`]
//! refused, which acts that spawn such objects check through [`spawn`].
//!
//! Where the poll stage runs outside the input job (the load state, and the frontend before its
//! main menu, call the input update inline on the game thread with no world update after it), the
//! game stage runs inline right after it and its writes land at once, as one call. [`Delivery`]
//! makes that choice from the thread role, never from the game state: the frontend runs the input
//! job too once the world is loaded behind the main menu.
//!
//! Deferring the writes to the next poll stage costs one sim tick between a decision and the game
//! reading it. Deciding in the poll stage instead would put the decisions, and the world reads
//! they rest on, back in the overlap.
//!
//! The [`Handoff`] carries, between one producer and one consumer per direction: the tick's
//! [`PollRecord`] from the poll stage to the game stage, the writes the game stage produced to the
//! next poll stage, where they landed back to the next game stage (which the hold tracker needs
//! before it resolves again), and the right stick's body turn for the next poll stage's look path.

use std::cell::{Cell, RefCell};

use parking_lot::Mutex;

use crate::{
    controllers::{
        bindings::{
            hold::{ActionWrite, DevicePoll, Phase},
            inject::{self, Landing, Outcome},
        },
        diagnostics::{Write, note_injected},
        gamepad_setting,
    },
    frame_timing::{self, Slot},
    thread_role::{self, ThreadRole},
};

/// Where the game stage runs relative to the poll stage that fed it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// The poll stage runs outside the input job, on the game thread with no world update after
    /// it: the game stage runs right after it in the same call, and its writes land at once.
    Inline,
    /// The poll stage runs in the input job: the game stage runs at the top of the world's
    /// pre-simulation queue, and its writes land after the next tick's device poll.
    Deferred,
}

impl Delivery {
    /// The delivery for a poll stage running in `role`.
    pub(crate) const fn for_role(role: ThreadRole) -> Self {
        match role {
            ThreadRole::InputJob => Self::Deferred,
            _ => Self::Inline,
        }
    }
}

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

/// Where the game stage runs within one world pre-simulation update, on the game thread: at the top
/// of its queue when the queue runs, or after the update when it skipped the queue (a paused
/// context). The update commits the update queue before it runs the queue, with nothing between
/// the two, so the first band the update queue runs after [`enter`](Self::enter) is that queue,
/// after the commit.
#[derive(Debug, Default)]
pub(crate) struct PreSimTurn {
    pending: bool,
}

impl PreSimTurn {
    /// The world's pre-simulation update begins.
    pub(crate) fn enter(&mut self) {
        self.pending = true;
    }

    /// The update queue is about to run a band: the game stage runs here, with spawning allowed,
    /// when it is the first band since [`enter`](Self::enter).
    pub(crate) fn queue_runs(&mut self) -> Option<Spawn> {
        std::mem::take(&mut self.pending).then_some(Spawn::Allowed)
    }

    /// The world's pre-simulation update returned: the game stage runs here, with spawning
    /// refused, when no band ran since [`enter`](Self::enter).
    pub(crate) fn leave(&mut self) -> Option<Spawn> {
        std::mem::take(&mut self.pending).then_some(Spawn::Refused)
    }
}

/// The world's pre-simulation update begins.
///
/// Must run on the game thread in the world's pre-simulation detour, before the original.
pub(crate) fn world_pre_sim_entered() {
    PRE_SIM_TURN.with_borrow_mut(PreSimTurn::enter);
}

/// The world's update queue is about to run a band: the game stage, when this is the
/// pre-simulation queue's top.
///
/// Must run in the update queue's execution detour, before the original.
pub(crate) fn update_queue_runs() {
    if let Some(spawn) = PRE_SIM_TURN.with_borrow_mut(PreSimTurn::queue_runs) {
        game_stage(Delivery::Deferred, spawn);
    }
}

/// The world's pre-simulation update returned: the game stage, when the update skipped its queue.
///
/// Must run on the game thread in the world's pre-simulation detour, after the original.
pub(crate) fn world_pre_sim_left() {
    if let Some(spawn) = PRE_SIM_TURN.with_borrow_mut(PreSimTurn::leave) {
        game_stage(Delivery::Deferred, spawn);
    }
}

/// What the poll stage hands the game stage.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PollRecord {
    /// The engine's tick delta, seconds.
    pub dt: f32,
    /// Whether the device poll that preceded the poll stage ran.
    pub poll: DevicePoll,
}

/// A run of writes the game stage produced together, in order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Segment {
    pub writes: Vec<ActionWrite>,
    /// Whether the hold tracker learns where these writes landed
    /// ([`HoldTracker::landed`](crate::controllers::bindings::hold::HoldTracker::landed)).
    pub tracked: bool,
}

/// A segment with what became of each of its writes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LandedSegment {
    pub segment: Segment,
    pub landings: Vec<Landing>,
}

/// What the bindings' game stage hands back.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct GameStageOutput {
    /// The writes left for the next poll stage to land; empty under [`Delivery::Inline`], where
    /// they already landed.
    pub deferred: Vec<Segment>,
    /// The controllers' body-turn input for the VR turn scheme, `-1..=1`, positive turning right,
    /// or `None` when the controllers are not injecting.
    pub body_turn: Option<f32>,
}

/// The poll stage: land the writes the last deferred game stage left, publish this tick for the
/// game stage, and run the game stage inline when the poll stage is not in the input job. Returns
/// the controllers' body turn for the look path.
///
/// Must run in the `InputDeviceManager::Update` post-call detour, after the device poll and while
/// the action maps exist.
pub(crate) fn poll_stage(dt: f32, poll: DevicePoll) -> Option<f32> {
    let delivery = Delivery::for_role(thread_role::current());
    let pending = HANDOFF.lock().take_pending();
    if !pending.is_empty() {
        // SAFETY: the poll stage runs right after the device poll, in the input update that owns
        // the action maps' writers.
        let landed = unsafe { land(pending) };
        HANDOFF.lock().push_landed(landed);
    }
    let missed = HANDOFF.lock().publish(PollRecord { dt, poll });
    if let Some(missed) = missed {
        log_missed(missed);
    }
    if delivery == Delivery::Inline {
        game_stage(Delivery::Inline, Spawn::Refused);
    }
    HANDOFF.lock().take_body_turn()
}

/// The game stage: run the bindings' decisions and acts for the tick the last poll stage
/// published, and the active-input-device pin, with `spawn` saying whether the acts may spawn
/// objects that register for the world's updates ([`spawn`]). Does nothing when no poll stage ran
/// since the last game stage (the device poll was skipped, or the frontend or load state already
/// ran this tick inline).
///
/// Must run on the game thread with no input job in flight: at the top of the world's
/// pre-simulation queue, after the world's pre-simulation update when it skipped its queue, or
/// inline from [`poll_stage`] outside the input job.
fn game_stage(delivery: Delivery, spawn: Spawn) {
    let Some((record, landed)) = HANDOFF.lock().take_for_game() else {
        return;
    };
    let output = {
        let _timing = frame_timing::scope(Slot::Bindings);
        let _spawn = SpawnScope::enter(spawn);
        crate::controllers::bindings::game_tick(record, landed, delivery)
    };
    gamepad_setting::input_tick();
    HANDOFF.lock().finish(output);
}

/// Land each segment's writes in order.
///
/// # Safety
///
/// As [`inject::apply`].
pub(crate) unsafe fn land(segments: Vec<Segment>) -> Vec<LandedSegment> {
    let landed: Vec<LandedSegment> = segments
        .into_iter()
        .map(|segment| {
            let landings = unsafe { inject::apply(&segment.writes) };
            LandedSegment { segment, landings }
        })
        .collect();
    note_injected(|| {
        landed
            .iter()
            .flat_map(|l| l.segment.writes.iter().zip(&l.landings))
            .map(|(write, landing)| injected_write(write, landing))
            .collect()
    });
    landed
}

/// The handoff between the stages. Each direction has one producer and one consumer: the poll
/// stage publishes the record and the landings, and takes the pending writes and the body turn; the
/// game stage takes the record and the landings, and leaves the writes and the body turn.
#[derive(Debug)]
pub(crate) struct Handoff<S, L> {
    /// The last poll stage's tick, until a game stage takes it.
    record: Option<PollRecord>,
    /// The writes the last game stage left, until a poll stage lands them.
    pending: Vec<S>,
    /// Where the poll stages' writes landed, until a game stage takes them.
    landed: Vec<L>,
    /// The body turn the last game stage produced, until a poll stage takes it.
    body_turn: Option<f32>,
    /// How many published records were replaced before a game stage took them.
    missed: u64,
}

impl<S, L> Handoff<S, L> {
    pub(crate) const fn new() -> Self {
        Self {
            record: None,
            pending: Vec::new(),
            landed: Vec::new(),
            body_turn: None,
            missed: 0,
        }
    }

    /// The writes the last game stage left, for the poll stage to land.
    pub(crate) fn take_pending(&mut self) -> Vec<S> {
        std::mem::take(&mut self.pending)
    }

    /// Record where the poll stage's writes landed, for the next game stage. Landings no game stage
    /// has taken yet are kept, in order.
    pub(crate) fn push_landed(&mut self, landed: impl IntoIterator<Item = L>) {
        self.landed.extend(landed);
    }

    /// Publish this tick's record. A record still pending means a poll stage had no game stage
    /// after it; its tick is dropped, since the inputs are per tick, and the running count of
    /// such ticks is returned.
    pub(crate) fn publish(&mut self, record: PollRecord) -> Option<u64> {
        self.record.replace(record).map(|_| {
            self.missed += 1;
            self.missed
        })
    }

    /// The tick for the game stage and where the writes landed since the last one, or `None` when
    /// no poll stage ran since the last game stage. The landings stay for a later game stage then.
    pub(crate) fn take_for_game(&mut self) -> Option<(PollRecord, Vec<L>)> {
        let record = self.record.take()?;
        Some((record, std::mem::take(&mut self.landed)))
    }

    /// The body turn the last game stage produced, once.
    pub(crate) fn take_body_turn(&mut self) -> Option<f32> {
        self.body_turn.take()
    }
}

impl<L> Handoff<Segment, L> {
    /// Leave the game stage's writes and body turn for the next poll stage. Writes no poll stage
    /// has landed yet stay ahead of them.
    pub(crate) fn finish(&mut self, output: GameStageOutput) {
        self.pending.extend(output.deferred);
        self.body_turn = output.body_turn;
    }
}

static HANDOFF: Mutex<Handoff<Segment, LandedSegment>> = Mutex::new(Handoff::new());

thread_local! {
    static PRE_SIM_TURN: RefCell<PreSimTurn> = const { RefCell::new(PreSimTurn { pending: false }) };
    static SPAWN: Cell<Spawn> = const { Cell::new(Spawn::Refused) };
}

/// Holds this thread's [`spawn`] answer for a game stage, and restores the previous one when
/// dropped.
struct SpawnScope {
    previous: Spawn,
}

impl SpawnScope {
    fn enter(spawn: Spawn) -> Self {
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

/// Log a poll stage that had no game stage after it, at every power of two so that a persistent
/// cause does not flood the log.
fn log_missed(missed: u64) {
    if missed.is_power_of_two() {
        tracing::warn!(
            target: "controllers",
            missed,
            "input tick: a poll stage had no game stage after it; its tick was dropped"
        );
    }
}

/// A landed write as the tracking recording notes it.
fn injected_write(write: &ActionWrite, landing: &Landing) -> Write {
    let (phase, held_s) = match write.phase {
        Phase::Value => ("value", None),
        Phase::Clicked => ("clicked", None),
        Phase::Held { seconds } => ("held", Some(seconds)),
        Phase::Released { seconds } => ("released", Some(seconds)),
        Phase::Unclick { .. } => ("unclick", None),
        Phase::Neutral => ("neutral", None),
    };
    Write {
        map: format!("{:?}", write.target.map),
        action: format!("{:?}", write.target.action),
        value: write.value,
        phase,
        held_s,
        outcome: match landing.outcome {
            Outcome::Applied => "applied",
            Outcome::KeptPhysicalPress => "kept physical press",
            Outcome::AlreadyMovedOn => "already moved on",
            Outcome::Unresolved => "unresolved",
        },
    }
}

#[cfg(test)]
mod tests {
    use jc3gi::input::input_action_map::Action;

    use super::*;
    use crate::controllers::bindings::hold::{MapKind, Phase, Target};

    const RECORD: PollRecord = PollRecord {
        dt: 1.0 / 60.0,
        poll: DevicePoll::Running,
    };

    fn segment(action: Action) -> Segment {
        Segment {
            writes: vec![ActionWrite {
                target: Target {
                    map: MapKind::Player,
                    action,
                },
                value: 1.0,
                phase: Phase::Clicked,
            }],
            tracked: true,
        }
    }

    fn output(actions: &[Action], body_turn: Option<f32>) -> GameStageOutput {
        GameStageOutput {
            deferred: actions.iter().copied().map(segment).collect(),
            body_turn,
        }
    }

    #[test]
    fn only_the_input_job_defers() {
        assert_eq!(Delivery::for_role(ThreadRole::InputJob), Delivery::Deferred);
        for role in [
            ThreadRole::Unmarked,
            ThreadRole::Game,
            ThreadRole::GameFrame,
            ThreadRole::GameDebugUi,
            ThreadRole::FrameTail,
        ] {
            assert_eq!(Delivery::for_role(role), Delivery::Inline, "{role:?}");
        }
    }

    /// The update runs its queue: the stage runs at the queue's top, after the commit, and not
    /// again when the update returns.
    #[test]
    fn a_running_queue_takes_the_stage_with_spawning_allowed() {
        let mut turn = PreSimTurn::default();
        turn.enter();
        assert_eq!(turn.queue_runs(), Some(Spawn::Allowed));
        assert_eq!(turn.leave(), None);
    }

    /// A paused update skips its queue: the stage runs after it, and may not spawn.
    #[test]
    fn a_skipped_queue_leaves_the_stage_to_the_update_end_with_spawning_refused() {
        let mut turn = PreSimTurn::default();
        turn.enter();
        assert_eq!(turn.leave(), Some(Spawn::Refused));
        assert_eq!(turn.leave(), None);
    }

    /// Only the first band after the update begins is its queue's top: later bands (the
    /// post-simulation, post-camera, and render queues), and bands outside the update, take nothing.
    #[test]
    fn only_the_first_band_of_the_update_takes_the_stage() {
        let mut turn = PreSimTurn::default();
        assert_eq!(turn.queue_runs(), None);
        turn.enter();
        assert_eq!(turn.queue_runs(), Some(Spawn::Allowed));
        assert_eq!(turn.queue_runs(), None);
        assert_eq!(turn.leave(), None);
        assert_eq!(turn.queue_runs(), None);
    }

    #[test]
    fn spawning_is_refused_outside_a_game_stage_and_restored_after_one() {
        assert_eq!(spawn(), Spawn::Refused);
        {
            let _outer = SpawnScope::enter(Spawn::Allowed);
            assert_eq!(spawn(), Spawn::Allowed);
            {
                let _inner = SpawnScope::enter(Spawn::Refused);
                assert_eq!(spawn(), Spawn::Refused);
            }
            assert_eq!(spawn(), Spawn::Allowed);
        }
        assert_eq!(spawn(), Spawn::Refused);
    }

    #[test]
    fn each_poll_stage_feeds_exactly_one_game_stage() {
        let mut handoff = Handoff::<Segment, u32>::new();
        assert_eq!(handoff.publish(RECORD), None);
        assert_eq!(handoff.take_for_game(), Some((RECORD, vec![])));
        assert_eq!(handoff.take_for_game(), None);
    }

    #[test]
    fn a_game_stage_without_a_poll_stage_does_nothing() {
        let mut handoff = Handoff::<Segment, u32>::new();
        assert_eq!(handoff.take_for_game(), None);
        handoff.push_landed([7]);
        assert_eq!(handoff.take_for_game(), None);
        handoff.publish(RECORD);
        assert_eq!(handoff.take_for_game(), Some((RECORD, vec![7])));
    }

    #[test]
    fn a_poll_stage_without_a_game_stage_drops_its_tick_and_counts_it() {
        let mut handoff = Handoff::<Segment, u32>::new();
        assert_eq!(handoff.publish(RECORD), None);
        let later = PollRecord {
            dt: 2.0 / 60.0,
            ..RECORD
        };
        assert_eq!(handoff.publish(later), Some(1));
        assert_eq!(handoff.publish(later), Some(2));
        assert_eq!(handoff.take_for_game(), Some((later, vec![])));
    }

    #[test]
    fn deferred_writes_land_once_in_the_next_poll_stage() {
        let mut handoff = Handoff::<Segment, u32>::new();
        handoff.publish(RECORD);
        handoff.take_for_game().unwrap();
        handoff.finish(output(&[Action::GUI_PAUSE], Some(0.5)));
        assert_eq!(handoff.take_pending(), vec![segment(Action::GUI_PAUSE)]);
        assert_eq!(handoff.take_body_turn(), Some(0.5));
        assert!(handoff.take_pending().is_empty());
        assert_eq!(handoff.take_body_turn(), None);
    }

    #[test]
    fn writes_no_poll_stage_landed_stay_ahead_of_newer_ones() {
        let mut handoff = Handoff::<Segment, u32>::new();
        handoff.finish(output(&[Action::GUI_PAUSE], None));
        handoff.finish(output(&[Action::GUI_PDA], Some(-1.0)));
        assert_eq!(
            handoff.take_pending(),
            vec![segment(Action::GUI_PAUSE), segment(Action::GUI_PDA)]
        );
        assert_eq!(handoff.take_body_turn(), Some(-1.0));
    }

    #[test]
    fn landings_reach_the_next_game_stage_in_order() {
        let mut handoff = Handoff::<Segment, u32>::new();
        handoff.push_landed([1, 2]);
        handoff.push_landed([3]);
        handoff.publish(RECORD);
        assert_eq!(handoff.take_for_game(), Some((RECORD, vec![1, 2, 3])));
        handoff.publish(RECORD);
        assert_eq!(handoff.take_for_game(), Some((RECORD, vec![])));
    }

    /// The deferred cadence over several ticks: the poll stage lands what the previous game stage
    /// decided, so every decision lands exactly once, one tick later, in order.
    #[test]
    fn deferred_ticks_land_every_decision_once_in_order() {
        let mut handoff = Handoff::<Segment, Vec<Action>>::new();
        let decisions = [Action::GUI_PAUSE, Action::GUI_PDA, Action::RELOAD];
        let mut landed_in_poll = Vec::new();
        let mut seen_by_game = Vec::new();
        for (tick, &action) in decisions.iter().enumerate() {
            let pending = handoff.take_pending();
            let landed: Vec<_> = pending
                .iter()
                .flat_map(|s| s.writes.iter().map(|w| w.target.action))
                .collect();
            landed_in_poll.push(landed.clone());
            if !landed.is_empty() {
                handoff.push_landed([landed]);
            }
            assert_eq!(handoff.publish(RECORD), None, "tick {tick}");
            let (_, landings) = handoff.take_for_game().unwrap();
            seen_by_game.push(landings.concat());
            handoff.finish(output(&[action], None));
        }
        assert_eq!(
            landed_in_poll,
            vec![vec![], vec![Action::GUI_PAUSE], vec![Action::GUI_PDA]]
        );
        assert_eq!(seen_by_game, landed_in_poll);
        assert_eq!(handoff.take_pending(), vec![segment(Action::RELOAD)]);
    }

    /// The inline cadence: the game stage runs right after the poll stage, so nothing is left
    /// pending between ticks.
    #[test]
    fn inline_ticks_leave_nothing_pending() {
        let mut handoff = Handoff::<Segment, u32>::new();
        for _ in 0..3 {
            assert!(handoff.take_pending().is_empty());
            assert_eq!(handoff.publish(RECORD), None);
            assert!(handoff.take_for_game().is_some());
            handoff.finish(GameStageOutput {
                deferred: Vec::new(),
                body_turn: Some(0.25),
            });
            assert_eq!(handoff.take_body_turn(), Some(0.25));
        }
    }

    /// A switch from deferred to inline (the run state handing over to the load state): the writes
    /// the last deferred game stage left land in the inline poll stage before its own game stage.
    #[test]
    fn a_switch_to_inline_lands_the_deferred_writes_first() {
        let mut handoff = Handoff::<Segment, u32>::new();
        handoff.publish(RECORD);
        handoff.take_for_game().unwrap();
        handoff.finish(output(&[Action::GUI_PAUSE], None));
        let pending = handoff.take_pending();
        assert_eq!(pending, vec![segment(Action::GUI_PAUSE)]);
        handoff.push_landed([1]);
        handoff.publish(RECORD);
        assert_eq!(handoff.take_for_game(), Some((RECORD, vec![1])));
    }
}
