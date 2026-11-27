# Performance

The mod's frame is measured with three tools, each answering a different question:

- The always-on frame breakdown ([below](#the-frame-breakdown)) runs in every build and logs every ten seconds whether a slow session is the mod, the engine, the OpenXR runtime, or a wait. Read it first, so the next measurement can be aimed.
- The in-game profiler ([below](#the-profiler)) captures a few seconds of CPU scopes and per-pass GPU timestamps on demand (F9), for what is inside the part of the frame that grew.
- The frame-time benchmark ([benchmark.md](../testing/benchmark.md)) plays a scene forward in each render mode under the capture harness and records every frame, for comparisons between modes, settings, and render sizes.

## What limits the frame

The benchmark's reference machine is a Ryzen 9 9950X3D (two dies with different L3 sizes) with an RTX 5090, under Proton, DXVK, and Monado's simulated headset at the Bigscreen Beyond 2e's 3584x3561 per eye. There, with the shipped settings, single-pass holds 72 Hz (13.9 ms) in every benchmark scene, and its median frame is 8.0 ms at the town and 8.7 to 9.4 ms elsewhere, against the 8.33 ms of 120 Hz. Every scene is GPU-bound, with under 0.4 ms of GPU idle a frame. A weaker GPU moves the render size at which the GPU takes over down, and a real headset adds its compositor's work.

No thread saturates. Two limits account for every measurement:

- The GPU. Its pixel work grows with the render size (about 0.25 ms per megapixel per eye for the scene passes and 0.23 ms for the post chain, FSR's 0.17 ms of it included).
- The serial CPU chain. The deferred frame tail cannot submit a frame until the draw task has recorded all of it, so the draw task is the floor of the frame. The draw task's time is flat across render sizes: the geometry passes are bound by draw submission, not by shading.

Most of the draw task is not the engine's walk of its render blocks but DXVK recording the engine's state changes (atomic reference counts on changed bindings and a free-list pointer chase), plus the uncontended fast path of the engine's context lock, which [`Graphics::AcquireThreadOwnership`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::draw##func-AcquireThreadOwnership) and its siblings take around each call into Direct3D. It is memory traffic, so it depends heavily on the cache it runs against. Three things shorten the chain, all on by default:

- Single-pass stereo cuts the submissions ([single-pass-stereo.md](../stereo/single-pass-stereo.md)). It still submits some families per eye (the decals, the terrain patches, the water, and the vegetation's hardware-instanced draws), about 0.3 ms of draw task at the capital; "What is still drawn twice" in [single-pass-render-blocks.md](../stereo/single-pass-render-blocks.md) has the work that would convert them. `PreDraw` (the shadow, reflection, and vegetation prepasses) runs once per dispatch and submits nothing per eye.
- `vr.large_cache_cores` ([`cpu_placement`](../../../crates/jc3vr-core/src/cpu_placement.rs)) narrows the process's affinity to the cores of the largest L3 when the L3 sizes differ, and leaves a processor with one size of L3 alone. The scheduler otherwise puts the draw task and DXVK's command-stream thread on the smaller-cache die most of the time; on the larger cache the draw task takes 56% of the time for the same submissions, and the GPU span falls with it, because the span had been the GPU waiting for commands. Threads created afterwards inherit the affinity; the ones the graphics driver created natively before it keep theirs.
- `vr.overlap_render_update` runs the next frame's `xrWaitFrame`, view location, and render update while the previous frame's draw task and tail are still running, and waits for the tail only at its `xrBeginFrame`. It takes the render update (3.5 ms at the capital) out of the chain. The frame order and what keeps it safe are in [threading.md](../threading.md#who-holds-what-across-a-frame). Unpaced, it ages the pose by 1 to 3.6 ms, since the views are located before the render update; under pacing the age is unchanged.

The remaining costs at the headset's size, largest first: FSR's 1.9 to 2.1 ms of GPU time ([fsr.md](../rendering/fsr.md)); the SSAO's 0.9 to 1.4 ms, screen-space over the double-wide target and unable to skip the foveation's dropped pixels; the engine's context lock, 1.3 to 1.4 ms of the draw task; the payload's per-draw work, about 1.7 ms of the draw task spread over many small sites with none above 0.15 ms; and the per-eye submissions. On a processor with one size of L3 the placement does nothing, and where the draw task limits the frame, the last three are the levers.

The 99th percentile sits 4 to 9 ms above the median under the null compositor, while the GPU's own 99th percentile is only 1 to 3 ms above its median: most of the tail is the runtime holding the next frame back in `xrWaitFrame` after a frame the simulation tick lengthened. A headset's compositor paces differently, so the tail needs measuring there before work on it.

## Measuring

- Measure the release payload (`scripts/proton_run.sh --release`). A debug payload's numbers describe the debug build.
- Leave the profiler's per-draw render-block scopes off unless the per-type timeline is the question: they put hundreds of scopes a frame on the draw-submission path they would measure.
- A GPU span does not by itself say the GPU limits a frame ([work, not span](#reading-the-gpu-lane-work-not-span)). A resolution sweep settles which limit applies better than any single capture: at a submission-bound scene the G-buffer's GPU span shrinks as the render size grows.
- Under a compositor that paces, a frame that misses the period costs whole periods. Even unpaced, Monado's application pacing (`u_pacing_app`) can block `xrWaitFrame` for several milliseconds with the GPU idle, and the same scene can land in either state from one session to the next, because it predicts each frame from filtered CPU, draw, and GPU times whose drift moves the wake-up. Compare the work inside the frame (the game thread's busy time, the draw task, the GPU spans), not only the frame interval.
- The game thread's median busy time mixes two populations: a frame that runs the 30 Hz simulation tick is busy about twice as long as one that does not, and the tick lands in more frames as the frame grows longer.
- The engine renders at the runtime's recommended size scaled by the current `vr.resolution_scale`, while the OpenXR swapchain keeps the size it was created at, and the eye-to-swapchain step is a scaling blit. A live change to the scale makes the two disagree, and then every frame pays a resample that looks like softness. Both sizes are in the log (`created the stereo swapchain` against the resize target).
- `--detour-timing` underestimates the stereo draw detours' own work: a clock-read pair costs 120 ns under Wine, so the timers hide most of what they measure. Sample the draw task instead (`--sample-draw-task`, [benchmark.md](../testing/benchmark.md)).

## The frame breakdown

`crates/jc3vr-core/src/frame_timing/` is the coarse, always-on counterpart of the profiler. It times a fixed set of slots (`frame_timing::Slot`, which documents each), each a region wrapped in a `frame_timing::scope`. Every scope records its self time, its length less the scopes that closed inside it on the same thread, so the [`Game::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Update) scope measures the engine's own work with the mod's hooks inside it taken out, and the self times on one thread add up without double counting. The frame interval is the time between successive `Game::Update` entries.

Each slot has a kind:

- `mod`: the mod's own CPU work, including its hooks inside the engine's work (the pose-pass hooks, the solve gate, the hand bodies' callback, the UI capture hooks, the body estimator's CPU side). Those hooks are timed around the mod's part only, so `engine_update` and `post_draw` lose exactly what those slots gain. The aim slot is the exception: it includes the engine's aim update it wraps, because the grapple hook makes the engine's queued visibility checks immediate ray casts inside it.
- `openxr`: OpenXR calls that do work rather than pace: `xrBeginFrame` with the view and head location, the action sync and reads, and `xrEndFrame`.
- `engine`: the sim tick and its phases, the rest of `Game::Update`, the render update, the game-thread draw per dispatch, and the draw task ([`GraphicsEngine::HandleDrawThreadTask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleDrawThreadTask)), whose time includes the stereo draw path's per-draw detours.
- `wait`: `xrWaitFrame`, the swapchain image wait, the VR runtime's locks (`vr_lock`, chiefly the submit side the deferred tail holds until it submits), and the drains that wait on the draw task.

Self time is also bucketed by thread role (`jc3vr_core::thread_role`): the game thread, which bounds the frame rate, the frame tail, and every other thread. The game thread's untracked time is the frame interval less everything the scopes covered there. Every slot and bucket is also summed into counters that are never reset (`frame_timing::cumulative`), which the benchmark differences once a frame.

### The sim tick

The sim tick is timed apart from the rest of `Game::Update` and split into its largest phases by hooks that time and count and change nothing (`crates/jc3vr-gameplay/src/hooks/sim.rs` and `online.rs`):

- `sim_tick`: each [`Game::UpdateGame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateGame) less the phases below.
- `sim_head` and `sim_tail`: the managers before the game world's pre-simulation queue and after the streaming block. Nothing marks their ends with a function of their own, so each is a `frame_timing::interval` of `sim_tick`, closed or opened at a neighbouring hook.
- `sim_world_pre` and `sim_world_post`: the game world's two update queues, where the characters' update passes and so the pose-pass hooks run.
- `sim_physics`: [`GameState::UpdatePhysics`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameState##func-UpdatePhysics), the step and its collision dispatch.
- `sim_streaming`: the time-budgeted streaming block, whose time per second grows with the tick rate when there is work.
- `input_job`: the input job's body, which the tick spins on, so a long job also lengthens `sim_head`.
- `ai_thread_1` and `ai_thread_2`: the AI threads' bodies, normally on fragment workers. The first is joined inside the tick; the second overlaps the render update.
- `sim_steam`, `sim_stats`, `sim_online`, `sim_ops`, `sim_login`, and the `osuite_*` parts of [`OnlineSuite::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::OnlineSuite##func-Update): the online managers, which run on the game thread. A Steam update longer than `online.slow_steam_callbacks_ms` (`crates/jc3vr-gameplay/tuning/gameplay.toml`) also logs a warning with its duration. `sim_logged_on` times the offline retry's first-party check, which the mod skips by default ([periodic-sim-tick-hitches.md](../../issues/periodic-sim-tick-hitches.md#the-remedy-the-offline-retrys-logged-on-check)); its call count says whether the check is running.

A tick is counted against the frame whose `Game::Update` ran it. If the frames that ran a tick are the slow ones and the others meet the period, the tick is what pushes a frame past its deadline.

### Where to read it

The log gets one line every ten seconds, target `perf`:

```
perf: frame breakdown window_s=10.0 frames=720 frame_ms=13.89 frame_max_ms=31.20 | sim: ticks=300 0=420/12.10/20.30 1=300/16.40/31.20 | game: mod=1.10 openxr=0.45 engine=6.80 wait=5.30 untracked=0.24 | tail: ... | other: ... | slots: debug_ui=0.80/2.10 ...
```

Times are milliseconds per frame. `sim:` splits the frames by how many ticks each ran, as `<ticks>=<frames>/<mean ms>/<max ms>`. Each slot reads `name=<self time per frame>/<longest single call>`. A game frame longer than a second is also logged on its own at warn level (`perf: a game frame stalled frame_s=…`), since averaged into its window it would hide as untracked time, and the window procedure logs activation changes on the `window` target, so a stall can be placed against losing the foreground. Rendering > Previews and performance shows the same numbers over the last second.

Reading it: `wait` dominating with `vr_wait_frame` large means the compositor paces the game, and a frame interval quantized to multiples of the display period means the runtime gave the app more than one period. `vr_lock` large means the game thread waits for the previous frame's tail, which waits on the draw task or the GPU. `engine` dominating is the submission-bound picture above. A large `mod` names its slot, and the longest call separates a steady cost from a periodic spike. A session that degrades over time shows as one slot growing from window to window.

Each scope costs two clock reads, a few thread-local accesses, and four relaxed atomic adds; only the game thread's once-a-frame tick touches a lock, once a second. There is no per-draw scope.

## The profiler

The engine's own profiler is dead: the [`CpuProfiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::CpuProfiler) is never written, the `CGPUProfiler` read-back is stubbed, and the on-screen frame graph is stripped ([profiling.md](../../engine/performance/profiling.md)). The live D3D11 timestamp-query wrappers and the hookable frame-phase functions survive, and the mod rebuilds the pipeline on them with [puffin](https://github.com/EmbarkStudios/puffin). The engine's scope markers are three-byte `ret` stubs too small to detour, so the per-render-block-type names are recovered from `CRenderPass::ChangeRenderBlockType` instead.

Everything is under the payload's `profiler` cargo feature (default on). Idle, each instrumented site costs a relaxed atomic load; with the feature off, the hooks and scopes do not exist, which is the comparison for the profiler's own overhead.

### Taking a capture

The debug UI's Rendering > Previews and performance feature has a profiler collapsible: a checkbox for collection and puffin's live flame graph, a button to capture 5 s, and checkboxes for the per-draw render-block scopes (off by default) and the GPU per-pass timestamps (on by default). F9 starts the same capture without the overlay, so it works in the headset. Hold the situation for the five seconds.

The capture lands in the session folder as `profiles/<stamp>.json` ([sessions.md](../sessions.md#the-sessions-root)), Chrome trace-event JSON with one lane per puffin thread ("game", "draw", "GPU"), serialized on a background thread. Summarize it with `uv run tools/analysis/analyze_profile.py <file>`, or open it in [ui.perfetto.dev](https://ui.perfetto.dev). Take it alongside the frame breakdown: the breakdown says which part grew, the capture what is inside it.

CPU scopes cover the frame, each sim tick and its phases, `UpdateRender`, each dispatch (tagged with the eye), the post-draw drain, and the AI threads on the game side; and `PreDraw`, `DrawGBuffer`, the scene, the post effects, `PostDraw`, and each `RenderPass::DoDraw` (named from the engine's `GetRenderPassName` table) on the draw side. The render-block-type hook always adds each finished run's block count to a per-type counter, which goes to the readout and the log, undercounting by each pass's final run.

### The GPU lane

The GPU lane is a synthetic puffin thread built from the engine's own `Graphics::CreateTimeStampQuery`, `SetTimeStampQuery`, and disjoint-query wrappers. Each dispatch brackets its seams (`PreDraw`, `DrawGBuffer`, the scene, the post effects with the mod's `FSR` seam inside, and `PostDraw`) with timestamp pairs under one disjoint query; the results resolve a few frames later under a per-dispatch scope ("GPU eye 0", "GPU eye 1"). Each post effect the payload hooks and the foveation's two passes get a named detail interval that the busy and starved split does not count. GPU ticks are mapped onto the CPU timeline from a reference taken at the dispatch's start, which is sound under DXVK's monotonic timestamp clock; disjoint intervals and implausible gaps are dropped. The live view's frame bars read wider than the true frame time because results land two to three frames late; the trace uses absolute time.

The queries also run without puffin while the benchmark records: the benchmark registers as the lane's dispatch sink in `jc3vr_render::profiler::gpu_samples`, so the lane names nothing of it.

### Reading the GPU lane: work, not span

A dispatch's scope, and each seam inside it, is a span: the GPU's whole execution window, idle included. A GPU drained by a draw thread that cannot keep up still reads its full width, so a capture can read as GPU-bound at 13 ms for a frame waiting on submission. Each render pass is therefore bracketed with its own timestamp pair; their sum is busy, and the holes between them are "GPU starved" scopes. Starvation between individual draws lands inside busy, so busy is an upper bound on GPU work and starved a lower bound on GPU idle. Read them against the CPU submit span: a GPU span that tracks the submit span dispatch after dispatch is submission-bound, and one that outruns it is real shading cost. All four go to the readout and, every five seconds while collecting, to the log.

### Where the code lives

- `crates/jc3vr-core/src/profiler/`: the frame and enable state, the dynamic-scope registry, the per-type block counters, the capture state machine, and the Chrome trace writer.
- `crates/jc3vr-render/src/profiler/gpu/` and `gpu_samples.rs`: the timestamp ring, the pass scopes, the busy and starved split, and the samples a recording run receives.
- `crates/jc3vr-render/src/hooks/profiler.rs`: the profiler-only detours. Seams the mod already hooks are instrumented inline under `#[cfg(feature = "profiler")]`.
- `crates/jc3vr-ui/src/profiler.rs`: the collapsible.

The core files call into neither the render nor the UI half.
