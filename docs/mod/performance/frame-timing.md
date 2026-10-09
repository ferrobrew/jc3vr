# The always-on frame breakdown

The profiler ([profiler.md](profiler.md)) shows where a frame goes in detail, but only while it collects, and a capture has to be taken on purpose. `payload/src/frame_timing/` is the coarse version that always runs, in every build: it says whether a slow session is the mod, the engine, the OpenXR runtime, or a wait, so the next capture can be aimed.

## What it measures

A fixed set of slots (`frame_timing::Slot`), each a region of the frame wrapped in a `frame_timing::scope`. Every scope records its self time: its length minus the scopes that closed inside it on the same thread. The engine's [`Game::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Update) scope therefore measures the engine's own work with the mod's hooks inside it taken out, and the self times on one thread add up to that thread's covered time without double counting.

Each slot has a kind:

- `mod`: the mod's own CPU work. The debug UI, the input tick and the controller bindings, the physical hands, the aim update, the render-update hook's glue, the submit's blit, the desktop mirror, the frame tail's glue, and the post-draw overlays on the draw task. Inside the engine's own work: the character pose-pass hooks around the HumanIK solve (`character_pose`), the procedural body at the solve gate (`body_gate`), the hand bodies' pre-simulation callback (`hand_bodies`), the UI capture thread's hooks (`ui_capture`), and the world markers' raycasts, placement, and depth sampling (`world_markers`).
- `openxr`: OpenXR calls that do work rather than pace. `xrBeginFrame` with the view and head location, the controllers' action sync and reads, and `xrEndFrame`.
- `engine`: the engine's own work. The sim tick and its phases (below), the rest of `Game::Update`, the engine's render update, and its game-thread draw per dispatch.
- `wait`: blocking. `xrWaitFrame`, the swapchain image wait, the VR runtime lock (which the deferred tail holds until it submits the previous frame), and the drains that wait on the draw task.

Self time is also bucketed by the thread it ran on, from its role in `crate::thread_role`: the game thread, the frame tail, and every other thread (the draw task and the fragment workers). The game thread is the one that bounds the frame rate. Its untracked time is the frame interval minus everything the scopes covered there, which is the engine's main loop outside `Game::Update` plus timer overhead.

The frame interval is the time between successive `Game::Update` entries.

## The sim tick

The sim tick is timed apart from the rest of `Game::Update`, and split into its largest phases, so a capture-free log answers what a tick costs and where. The hooks are in `payload/src/hooks/sim.rs`; they time and count, and change nothing.

- `sim_tick`: each [`Game::UpdateGame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateGame), less the phases below. What is left is the run state's managers (missions, heat, spawning, the camera, vehicles' pre-update, and the rest), its waits on the AI and light fragments, and the character manager's and effects' updates between the phases.
- `sim_world_pre` and `sim_world_post`: the game world's two update queues, [`GameWorld::UpdatePreSim`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-UpdatePreSim) and [`GameWorld::UpdatePostSim`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-UpdatePostSim). The characters' update passes run in them, so the pose-pass hooks (`character_pose`, `body_gate`) nest inside when the game thread runs a pass itself.
- `sim_physics`: the run state's physics phase, [`GameState::UpdatePhysics`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameState##func-UpdatePhysics): the physics step and the collision, damage, and shape-sensor dispatch after it. The hand bodies' callback (`hand_bodies`) nests inside it.
- `sim_streaming`: the tick's time-budgeted streaming block, timed at its three calls, [`StreamerUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/game##func-StreamerUpdate), [`GameLocationSystemUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/game##func-GameLocationSystemUpdate), and [`GameState::UpdateDeletionOfObjects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameState##func-UpdateDeletionOfObjects). The block has a budget per tick, so its time per second grows with the tick rate when there is work.
- `ai_thread_1` and `ai_thread_2`: the AI threads' bodies, [`RunAIThread1_Proxy`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/game##func-RunAIThread1_Proxy) and [`RunAIThread2_Proxy`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/game##func-RunAIThread2_Proxy). Each runs on a CPU fragment worker, so its time lands in `other` unless the game thread ran it while waiting. The first is joined inside the same tick, so its length can show as wait in `sim_tick`. The second is kicked at the end of the tick and joined by the render update and the next tick, so it overlaps the frame's render update rather than adding to the tick.

The log line's `sim:` part counts the window's ticks and splits the frames by how many ticks each ran, as `<ticks>=<frames>/<mean ms>/<max ms>` for the counts that occurred (`0`, `1`, and `2+`), as in the example under [Where to read it](#where-to-read-it).

A tick is counted against the frame whose `Game::Update` ran it. If the `1` frames are the slow ones and the `0` frames meet the display's period, a tick is what pushes a frame past its deadline, and `sim_tick` with its phases says by how much and where. The cost of one tick is the self time of `sim_tick` and its game-thread phases over the window, divided by `sim_tick`'s call count. The Performance tab shows the same split over the last second.

The hooks inside the engine's work are timed around the mod's part only, never around the engine call they wrap, so `engine_update` and `post_draw` lose exactly the time those slots gain. The pose-pass hooks and the solve gate run on whichever thread the engine updates the character on, so their time can land in `other` as well as in `game`; the UI capture hooks land in `other`.

The aim slot includes the engine's aim update it wraps, because the mod's grapple hook makes the engine's queued visibility checks immediate ray casts inside it.

## Where to read it

The log gets one line every ten seconds, target `perf`, next to the `VR frame-loop health` line:

```
perf: frame breakdown window_s=10.0 frames=720 frame_ms=13.89 frame_max_ms=31.20 | sim: ticks=300 0=420/12.10/20.30 1=300/16.40/31.20 | game: mod=1.10 openxr=0.45 engine=6.80 wait=5.30 untracked=0.24 | tail: mod=0.30 openxr=0.20 engine=0.00 wait=1.10 | other: mod=0.40 openxr=0.00 engine=0.00 wait=0.00 | slots: debug_ui=0.80/2.10 ...
```

A game frame longer than a second is also logged on its own, at warn level, as `perf: a game frame stalled frame_s=…`: averaged into its window, an 80-second frame reads as a few hundred milliseconds of `untracked` per frame and hides that it was one frame.

Every time is milliseconds. The per-thread numbers are per frame. Each slot is listed as `name=<self time per frame>/<longest single call>`, the latter including nested scopes. Slots that did not run in the window are left out.

The Performance tab's "Frame breakdown" section shows the same numbers over the last second, with the slots sorted by self time.

## Reading it

- `wait` dominating the game thread, with `vr_wait_frame` large, means the compositor is pacing the game: the game finishes early and waits for the next frame slot. A frame interval quantized to multiples of the display period (13.9, 27.8, 55.6 ms at 72 Hz) while `xrWaitFrame` is large means the runtime has given the app more than one period per frame. Look at the GPU (the profiler's GPU lane) or at what the frame missed by.
- `vr_lock` large means the game thread is waiting for the previous frame's tail, which is waiting on the draw task or the GPU (`tail_drain`).
- `engine` dominating with little `mod` means the game's own work is the frame: the sim, the render update, and the per-dispatch draw. That is the submission-bound picture in [performance.md](performance.md).
- `mod` large points at a slot by name. The longest-call figure separates a steady cost from a periodic spike.

A session that degrades over time shows as one slot growing from window to window. Comparing an early line with a late one names it.

## Cost

Each scope costs two clock reads (`QueryPerformanceCounter` through `Instant`), a few thread-local accesses, and four relaxed atomic adds. A frame has a few dozen scopes, and each sim tick adds about ten. Only the once-a-frame tick on the game thread touches a lock, and only once a second when a window closes. There is no per-draw or per-pass scope: the draw path runs tens of thousands of times a frame, and the profiler covers it.
