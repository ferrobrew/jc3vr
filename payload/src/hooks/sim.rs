//! Detours around the sim tick and its phases: timing for the always-on frame breakdown
//! (`crate::frame_timing`) and, under the `profiler` feature, the profiler's flame graph; the input
//! job's thread role; and the input tick's game stage.
//!
//! The timing hooks change nothing the game does: each opens a [`frame_timing::scope`] around the
//! original call. Three hooks do more. The input job's body (`RunInputThread_Proxy`) holds
//! [`ThreadRole::InputJob`] for its extent, so the input tick can tell the job, which overlaps the
//! game thread's world-mutating work, from an inline input update on the game thread. The world's
//! pre-simulation update and the update queue's execution ([`UpdateQueue::Execute`]) together run
//! the input tick's game stage at the top of the pre-simulation queue
//! ([`stage::update_queue_runs`]): on the game thread, after the input job's completion wait and
//! after the update-queue commit that opens the world's pre-simulation update, and before any
//! character, weapon, or vehicle update and the physics step. The stage's acts so see no concurrent
//! world mutation, still precede the world update that reads them, and register what they spawn
//! after the commit. When the update skips its queue (a paused context), the stage runs after it
//! instead ([`stage::world_pre_sim_left`]).
//!
//! The tick itself ([`Game::UpdateGame`]) also counts against the frame it runs in,
//! so the breakdown can split the frames by how many ticks they carried. Inside the tick, the run
//! state's phases are timed at the nearest functions the engine calls for them: the game world's
//! two update queues, the physics phase, and the three calls of the time-budgeted streaming block.
//! The two AI threads are timed around their bodies, which run on CPU fragment workers (or on a
//! thread that runs queued fragments while it waits), so their time lands in the breakdown's other
//! threads unless the game thread ran them itself. The input job's body is timed the same way.
//!
//! The rest of the tick is split in two by [`frame_timing::interval`]s: the head, from the tick's
//! start to the world's pre-simulation queue, and the tail, from the end of the streaming block to
//! the tick's end. The online managers, which run in both and keep their own clocks, are timed on
//! their own inside them by `crate::hooks::online`.
//!
//! Threading: everything here except the AI thread bodies and the input job runs on the game thread,
//! inside the engine's `Game::Update`.

use std::{cell::RefCell, ffi::c_void};

use detours_macro::detour;
use jc3gi::{
    game::{Game, GameObjectUpdateContext, UpdateContexts},
    game_world::GameWorld,
    update_queue::UpdateQueue,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::bindings::stage,
    frame_timing::{self, Interval, Slot},
    thread_role::{self, ThreadRole},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_GAME_BINDER)
        .with_static_binder(&WORLD_UPDATE_PRE_SIM_BINDER)
        .with_static_binder(&UPDATE_QUEUE_EXECUTE_BINDER)
        .with_static_binder(&UPDATE_PHYSICS_BINDER)
        .with_static_binder(&WORLD_UPDATE_POST_SIM_BINDER)
        .with_static_binder(&STREAMER_UPDATE_BINDER)
        .with_static_binder(&LOCATION_SYSTEM_UPDATE_BINDER)
        .with_static_binder(&UPDATE_DELETION_OF_OBJECTS_BINDER)
        .with_static_binder(&AI_THREAD_1_BINDER)
        .with_static_binder(&AI_THREAD_2_BINDER)
        .with_static_binder(&INPUT_JOB_BINDER)
}

#[detour(address = jc3gi::game::Game::UpdateGame_ADDRESS)]
fn update_game(this: *mut Game, update_contexts: *mut UpdateContexts) {
    frame_timing::note_sim_tick();
    let _timing = frame_timing::scope(Slot::SimTick);
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("CGame::UpdateGame");
    tick_opened();
    UPDATE_GAME.get().unwrap().call(this, update_contexts);
    tick_closed();
}

#[detour(address = jc3gi::game_world::GameWorld::UpdatePreSimImpl_ADDRESS)]
fn world_update_pre_sim(this: *mut GameWorld, context: *mut GameObjectUpdateContext) {
    // Before the scope opens, so the head closes as a child of the tick's scope.
    tick_world_entered();
    let _timing = frame_timing::scope(Slot::SimWorldPreSim);
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("GameWorld::UpdatePreSim");
    stage::world_pre_sim_entered();
    WORLD_UPDATE_PRE_SIM.get().unwrap().call(this, context);
    stage::world_pre_sim_left();
}

#[detour(address = jc3gi::update_queue::UpdateQueue::Execute_ADDRESS)]
fn update_queue_execute(
    this: *mut UpdateQueue,
    dt: f32,
    prio_min: i32,
    prio_max: i32,
    context: *mut c_void,
) {
    stage::update_queue_runs();
    UPDATE_QUEUE_EXECUTE
        .get()
        .unwrap()
        .call(this, dt, prio_min, prio_max, context);
}

#[detour(address = jc3gi::game::GameState::UpdatePhysics_ADDRESS)]
fn update_physics(dt: f32) {
    let _timing = frame_timing::scope(Slot::SimPhysics);
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("GameState::UpdatePhysics");
    UPDATE_PHYSICS.get().unwrap().call(dt);
}

#[detour(address = jc3gi::game_world::GameWorld::UpdatePostSimImpl_ADDRESS)]
fn world_update_post_sim(this: *mut GameWorld, context: *mut GameObjectUpdateContext) {
    let _timing = frame_timing::scope(Slot::SimWorldPostSim);
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("GameWorld::UpdatePostSim");
    WORLD_UPDATE_POST_SIM.get().unwrap().call(this, context);
}

#[detour(address = jc3gi::game::StreamerUpdate_ADDRESS)]
fn streamer_update(budget_us: u32) -> u32 {
    let _timing = frame_timing::scope(Slot::SimStreaming);
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("StreamerUpdate");
    STREAMER_UPDATE.get().unwrap().call(budget_us)
}

#[detour(address = jc3gi::game::GameLocationSystemUpdate_ADDRESS)]
fn location_system_update(budget_us: u32) {
    let _timing = frame_timing::scope(Slot::SimStreaming);
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("GameLocationSystemUpdate");
    LOCATION_SYSTEM_UPDATE.get().unwrap().call(budget_us);
}

#[detour(address = jc3gi::game::GameState::UpdateDeletionOfObjects_ADDRESS)]
fn update_deletion_of_objects(budget_us: u32) {
    {
        let _timing = frame_timing::scope(Slot::SimStreaming);
        #[cfg(feature = "profiler")]
        puffin::profile_scope!("GameState::UpdateDeletionOfObjects");
        UPDATE_DELETION_OF_OBJECTS.get().unwrap().call(budget_us);
    }
    // After the scope closes, so the tail opens in the tick's scope.
    tick_streaming_left();
}

#[detour(address = jc3gi::game::RunAIThread1_Proxy_ADDRESS)]
fn ai_thread_1(dt: *mut f32) -> i32 {
    let _timing = frame_timing::scope(Slot::AiThread1);
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("AI thread 1");
    AI_THREAD_1.get().unwrap().call(dt)
}

#[detour(address = jc3gi::game::RunAIThread2_Proxy_ADDRESS)]
fn ai_thread_2(dt: *mut f32) -> i32 {
    let _timing = frame_timing::scope(Slot::AiThread2);
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("AI thread 2");
    AI_THREAD_2.get().unwrap().call(dt)
}

#[detour(address = jc3gi::game::RunInputThread_Proxy_ADDRESS)]
fn input_job(update_contexts: *mut UpdateContexts) -> i32 {
    let _role = thread_role::enter(ThreadRole::InputJob);
    let _timing = frame_timing::scope(Slot::InputJob);
    INPUT_JOB.get().unwrap().call(update_contexts)
}

/// A sim tick opened: start its head.
fn tick_opened() {
    TICK_PHASES.with_borrow_mut(|phases| {
        *phases = TickPhases {
            open: true,
            head: Some(frame_timing::interval()),
            tail: None,
        };
    });
}

/// The head and tail intervals of the sim tick open on the game thread.
#[derive(Default)]
struct TickPhases {
    /// Whether a sim tick is running, so a streaming call outside one opens no tail.
    open: bool,
    head: Option<Interval>,
    tail: Option<Interval>,
}

thread_local! {
    static TICK_PHASES: RefCell<TickPhases> = RefCell::new(TickPhases::default());
}

/// The game world's pre-simulation queue is starting: the head ends here. A pre-simulation update
/// outside a tick, or a second one in the same tick, finds no head.
fn tick_world_entered() {
    if let Some(head) = TICK_PHASES.with_borrow_mut(|phases| phases.head.take()) {
        head.finish(Slot::SimHead);
    }
}

/// The streaming block's last call returned: the tail starts here, or restarts if the block runs
/// more than once in the tick.
fn tick_streaming_left() {
    let earlier = TICK_PHASES.with_borrow_mut(|phases| {
        if !phases.open {
            return None;
        }
        phases.tail.replace(frame_timing::interval())
    });
    // An earlier pass's tail ends where the next one starts, so its time is not lost.
    if let Some(earlier) = earlier {
        earlier.finish(Slot::SimTail);
    }
}

/// The sim tick is returning. A tick that never reached the world update (a state other than the
/// run state) is all head.
fn tick_closed() {
    let phases = TICK_PHASES.with_borrow_mut(std::mem::take);
    if let Some(head) = phases.head {
        head.finish(Slot::SimHead);
    }
    if let Some(tail) = phases.tail {
        tail.finish(Slot::SimTail);
    }
}
