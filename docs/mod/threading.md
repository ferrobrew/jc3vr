# Threading and the VR runtime's locks

The mod runs code on engine threads it does not own and on a frame-tail worker of its own. The VR runtime has two locks: the runtime state (`VR_STATE`, `crates/jc3vr-view/src/vr/state.rs`), taken briefly by the game thread, and the submit side (`SUBMIT`, `vr/submit.rs`: the frame stream and the swapchain), held for most of a frame while its holder waits on the draw task and the fragment workers. A thread those waits depend on that blocks on either lock deadlocks the game, so where they are taken is a correctness rule. The OpenXR loop is in [vr-runtime.md](vr-runtime.md#the-frame-loop).

## Threads

Beside the game thread and the tail worker, the draw task ([`GraphicsEngine::HandleDrawThreadTask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleDrawThreadTask)) runs the render hooks and `RenderEngine::PostDraw` on a fragment worker, and the game thread runs queued fragments itself while it waits on them. The input job runs the input tick's poll stage concurrently with part of the update and may touch only input, UI, and the mod's own state ([engine/gameplay/input.md](../engine/gameplay/input.md#where-the-device-poll-runs)). The frame waits on the UI capture thread, which runs the pause-menu hooks, while holding the submit side.

## Who holds what across a frame

With the render update overlapped (`vr.overlap_render_update`, on by default), a frame on the game thread runs:

1. `Game::Update` runs the debug UI under the egui state lock, then the simulation, while the previous frame's draw task and tail may still run.
2. The render update opens the frame's slots (`render_frame::begin_preparing`) and calls `vr::update`, `vr::auto_recenter_tick`, `vr::apply_native_resolution`, and `vr::frame_wait`, each locking the runtime state briefly. The engine's render update runs.
3. `WaitedFrame::begin` takes the submit side, which blocks until the previous tail has submitted, calls `xrBeginFrame`, and `vr::arm_direct` acquires the swapchain image for the draw task's `PostDraw`.
4. The eye dispatches run with the submit side held.
5. The game thread submits and releases the submit side, or hands the `FrameContext` and its guard to the tail worker and returns to the next `Game::Update`.

With the overlap off, the game thread waits for the tail (`vr::wait_for_frame_tail`) before step 2. The overlap matches the engine, which fences at the top of [`Game::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Draw), not in the render update. Three things keep it safe. Every value the render update writes for the draw task is a `RenderFrameSlot` (`jc3vr_core::render_frame`), and the draw task pins the frame its dispatch draws (`pin_dispatch`), so its reads answer with that frame's value. The runtime's swapchain calls record on the immediate context, so they run under the engine's context section. And everything from the begin on, a desktop resize, a session stop, and the teardown wait for the tail.

Under the runtime locks the holders take only brief leaf locks, and nothing takes the runtime locks under them. The egui state lock is the other long hold: the draw task takes it in `PostDraw` for the floating panel and the mirror overlay, so with a deferred tail the draw task can wait for the game thread's egui pass, never the reverse (rule 6).

## Lock rules

1. Block on the runtime's locks only on the game thread at the top of the render update, at the frame's begin, or on the shutdown path (rule 6). Take the runtime state before the submit side.
2. Everywhere else, read a published slot: `vr::render_params`, `vr::is_running_published`, `vr::stage::stage`, `jc3vr_input::frame::latest`, `vr::published_status`, and the like.
3. Ask for runtime work by request. `vr::recenter` and its variants set atomic bits that `vr::auto_recenter_tick` services next frame, so any thread may call them under any lock.
4. Take the runtime's locks outermost, never while holding another lock, the config lock included.
5. Keep published slots as leaves: hold them briefly, take no lock under them, and never wait while holding one. A reader that must touch an engine lock try-enters it.
6. Do not wait on the draw task while holding the egui state lock. The shutdown cleanups run under it, so `shutdown_from_game` calls `vr::wait_for_frame_tail` first.
7. Take the eye blit's locks in one order: the engine's immediate-context section, then the direct-blit slot (`DIRECT`), then the blit pipeline (`VR_BLIT`, both in `vr/blit.rs`). A holder of `VR_BLIT` outside the section must not enter the section under it.

## The guard

`jc3vr_core::thread_role` gives each thread a role. The game thread marks itself `Game` and the tail worker `FrameTail`; engine threads stay `Unmarked`. A `RoleScope` narrows the role for an extent: `GameDebugUi` for the egui pass, `GameFrame` for the render update after `vr::frame_wait` (also in flatscreen, so a violation shows without a headset), and `InputJob` for the input job, including a run the game thread steals.

Every blocking lock goes through `lock_state` or `lock_submit`, which treat any role but `Game` as a violation: a debug build logs the caller's location and panics before blocking, and a release build logs the first violation and locks. The guard checks the thread, not the locks held, so rule 4 is not enforced.

## The game tick

`jc3vr_core::game_tick::GameTick` is a token for a point on the game thread where no other engine thread mutates the world, so a game object live when the code reaches it stays live until it returns. Handles to game objects, such as `LocalPlayer` (`jc3vr-body`, `controllers::physics::local_player`), borrow it. `GameTick::enter` lends it to a closure with a fresh lifetime; opening one is `unsafe`, and a debug build admits only the `Game` and `GameDebugUi` roles. Three serial points open a tick: the input tick's game stage at the top of the pre-simulation queue, the hand bodies' frame tick at the top of `Game::Update`, and the palms' tick at the top of the render update.

A tick does not cover engine calls made inside it, which can free objects or reenter the mod's detours, so code that keeps an object across one re-resolves it afterwards. Nor does it make an object exclusive, so a handle lends `&mut` only through `&mut self` and is neither `Clone` nor `Copy`. Code without a tick (engine-thread detours, the parallel pose pass, physics callbacks, and render time) keeps its own `# Safety` contract and fetches the local player itself.
