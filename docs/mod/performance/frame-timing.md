# The always-on frame breakdown

The profiler ([profiler.md](profiler.md)) shows where a frame goes in detail, but only while it collects, and a capture has to be taken on purpose. `payload/src/frame_timing/` is the coarse version that always runs, in every build: it says whether a slow session is the mod, the engine, the OpenXR runtime, or a wait, so the next capture can be aimed.

## What it measures

A fixed set of slots (`frame_timing::Slot`), each a region of the frame wrapped in a `frame_timing::scope`. Every scope records its self time: its length minus the scopes that closed inside it on the same thread. The engine's [`Game::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Update) scope therefore measures the engine's own work with the mod's hooks inside it taken out, and the self times on one thread add up to that thread's covered time without double counting.

Each slot has a kind:

- `mod`: the mod's own CPU work. The debug UI, the input tick and the controller bindings, the physical hands, the aim update, the render-update hook's glue, the submit's blit, the desktop mirror, the frame tail's glue, and the post-draw overlays on the draw task.
- `openxr`: OpenXR calls that do work rather than pace. `xrBeginFrame` with the view and head location, the controllers' action sync and reads, and `xrEndFrame`.
- `engine`: the engine's own work. The rest of `Game::Update` (the sim tick), the engine's render update, and its game-thread draw per dispatch.
- `wait`: blocking. `xrWaitFrame`, the swapchain image wait, the VR runtime lock (which the deferred tail holds until it submits the previous frame), and the drains that wait on the draw task.

Self time is also bucketed by the thread it ran on, from its role in `crate::thread_role`: the game thread, the frame tail, and every other thread (the draw task and the fragment workers). The game thread is the one that bounds the frame rate. Its untracked time is the frame interval minus everything the scopes covered there, which is the engine's main loop outside `Game::Update` plus timer overhead.

The frame interval is the time between successive `Game::Update` entries.

The aim slot includes the engine's aim update it wraps, because the mod's grapple hook makes the engine's queued visibility checks immediate ray casts inside it.

## Where to read it

The log gets one line every ten seconds, target `perf`, next to the `VR frame-loop health` line:

```
perf: frame breakdown window_s=10.0 frames=720 frame_ms=13.89 frame_max_ms=31.20 | game: mod=1.10 openxr=0.45 engine=6.80 wait=5.30 untracked=0.24 | tail: mod=0.30 openxr=0.20 engine=0.00 wait=1.10 | other: mod=0.40 openxr=0.00 engine=0.00 wait=0.00 | slots: debug_ui=0.80/2.10 ...
```

Every time is milliseconds. The per-thread numbers are per frame. Each slot is listed as `name=<self time per frame>/<longest single call>`, the latter including nested scopes. Slots that did not run in the window are left out.

The Performance tab's "Frame breakdown" section shows the same numbers over the last second, with the slots sorted by self time.

## Reading it

- `wait` dominating the game thread, with `vr_wait_frame` large, means the compositor is pacing the game: the game finishes early and waits for the next frame slot. A frame interval quantized to multiples of the display period (13.9, 27.8, 55.6 ms at 72 Hz) while `xrWaitFrame` is large means the runtime has given the app more than one period per frame. Look at the GPU (the profiler's GPU lane) or at what the frame missed by.
- `vr_lock` large means the game thread is waiting for the previous frame's tail, which is waiting on the draw task or the GPU (`tail_drain`).
- `engine` dominating with little `mod` means the game's own work is the frame: the sim, the render update, and the per-dispatch draw. That is the submission-bound picture in [performance.md](performance.md).
- `mod` large points at a slot by name. The longest-call figure separates a steady cost from a periodic spike.

A session that degrades over time shows as one slot growing from window to window. Comparing an early line with a late one names it.

## Cost

Each scope costs two clock reads (`QueryPerformanceCounter` through `Instant`), a few thread-local accesses, and four relaxed atomic adds. A frame has a few dozen scopes. Only the once-a-frame tick on the game thread touches a lock, and only once a second when a window closes. There is no per-draw or per-pass scope: the draw path runs tens of thousands of times a frame, and the profiler covers it.
