//! Timing detours around the sim tick and its phases, for the always-on frame breakdown
//! (`crate::frame_timing`) and, under the `profiler` feature, the profiler's flame graph.
//!
//! These hooks change nothing the game does: each opens a [`frame_timing::scope`] around the
//! original call. The tick itself ([`Game::UpdateGame`]) also counts against the frame it runs in,
//! so the breakdown can split the frames by how many ticks they carried. Inside the tick, the run
//! state's phases are timed at the nearest functions the engine calls for them: the game world's
//! two update queues, the physics phase, and the three calls of the time-budgeted streaming block.
//! The two AI threads are timed around their bodies, which run on CPU fragment workers (or on a
//! thread that runs queued fragments while it waits), so their time lands in the breakdown's other
//! threads unless the game thread ran them itself.
//!
//! Threading: everything here except the AI thread bodies runs on the game thread, inside the
//! engine's `Game::Update`.

use detours_macro::detour;
use jc3gi::{
    game::{Game, GameObjectUpdateContext, UpdateContexts},
    game_world::GameWorld,
};
use re_utilities::hook_library::HookLibrary;

use crate::frame_timing::{self, Slot};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_GAME_BINDER)
        .with_static_binder(&WORLD_UPDATE_PRE_SIM_BINDER)
        .with_static_binder(&UPDATE_PHYSICS_BINDER)
        .with_static_binder(&WORLD_UPDATE_POST_SIM_BINDER)
        .with_static_binder(&STREAMER_UPDATE_BINDER)
        .with_static_binder(&LOCATION_SYSTEM_UPDATE_BINDER)
        .with_static_binder(&UPDATE_DELETION_OF_OBJECTS_BINDER)
        .with_static_binder(&AI_THREAD_1_BINDER)
        .with_static_binder(&AI_THREAD_2_BINDER)
}

#[detour(address = jc3gi::game::Game::UpdateGame_ADDRESS)]
fn update_game(this: *mut Game, update_contexts: *mut UpdateContexts) {
    frame_timing::note_sim_tick();
    let _timing = frame_timing::scope(Slot::SimTick);
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("CGame::UpdateGame");
    UPDATE_GAME.get().unwrap().call(this, update_contexts);
}

#[detour(address = jc3gi::game_world::GameWorld::UpdatePreSimImpl_ADDRESS)]
fn world_update_pre_sim(this: *mut GameWorld, context: *mut GameObjectUpdateContext) {
    let _timing = frame_timing::scope(Slot::SimWorldPreSim);
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("GameWorld::UpdatePreSim");
    WORLD_UPDATE_PRE_SIM.get().unwrap().call(this, context);
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
    let _timing = frame_timing::scope(Slot::SimStreaming);
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("GameState::UpdateDeletionOfObjects");
    UPDATE_DELETION_OF_OBJECTS.get().unwrap().call(budget_us);
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
