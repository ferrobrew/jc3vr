# The in-game profiler

The mod carries its own frame profiler because the engine's is dead: the [`CpuProfiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::CpuProfiler), with one phase per [`CpuScopeId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::CpuScopeId), is never written ([profiling.md](../../engine/performance/profiling.md) §1.1), the `CGPUProfiler` read-back is stubbed (§2.2), and the on-screen frame graph is stripped (§1.4). The live D3D11 timestamp-query wrappers and the hookable frame-phase functions survive, and they are enough to rebuild the pipeline on the mod's side on top of [puffin](https://github.com/EmbarkStudios/puffin).

Everything is compiled under the payload's `profiler` cargo feature (default on). With the feature on but the profiler idle (panel closed, no capture), each instrumented site costs a relaxed atomic load, plus a few thread-local accesses per dispatch for the lane labels and the type-run slot. With the feature off, the hooks and scopes do not exist.

## Using it

The debug UI's Rendering > Previews and performance feature has a "Profiler (issue #34)" collapsible. A checkbox enables scope collection and shows puffin's live flame graph, and a button captures 5 s of frames. Two further checkboxes control the fine layers: the per-draw render-block scopes (off by default, because they inflate the draw-submission path they measure) and the GPU per-pass timestamps (on by default, because they separate GPU work from GPU starvation). The rolling GPU busy, starved, and submit readout and the per-render-block-type draw counts sit under them.

F9 starts the same capture without the overlay, so it works in the headset. Progress shows in the collapsible and the result is logged.

## Taking a capture

1. Build with the feature. It is a default feature, so `scripts/proton_run.sh --release` builds and injects a release payload with it. Capture in release; a debug payload's numbers describe the debug build. Building `jc3vr_payload` with `--no-default-features` leaves it out, which is the comparison to make when the question is the profiler's own overhead.
2. In the game, reach the situation to measure, then press F9. The log says `F9: profiler trace capture started` and `profiler: capturing 5.0s of frames`, and, once the file is written, the path. Hold the situation for the whole five seconds.
3. Leave the per-draw render-block scopes off unless the per-type timeline is the question. Leave the GPU per-pass timestamps on.
4. The capture lands in the session folder as `profiles/<stamp>.json` ([sessions.md](../sessions.md#the-sessions-root)); the startup log and the Status page name the session folder. It is Chrome trace-event JSON: each puffin thread is a named lane ("game", "draw", "GPU"), and timestamps are rebased to the capture start. Serialization runs on a background thread, since a capture is tens of megabytes, so the game keeps rendering while the file is written.
5. Summarize it with `uv run tools/analysis/analyze_profile.py <file>`, or open it in [ui.perfetto.dev](https://ui.perfetto.dev) or `chrome://tracing`.

Take the capture alongside the always-on breakdown in the same log ([frame-timing.md](frame-timing.md)): the breakdown says which part of the frame grew, and the capture shows what is inside it.

## What is instrumented

CPU scopes on the main thread: [`Game::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Update) (the frame); [`Game::UpdateGame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateGame) (each sim tick) and, inside it, the game world's two update queues, the physics phase, and the streaming block's three calls; `UpdateRender`; the per-eye dispatch loop (one `Dispatch` scope per `Game::Draw`, tagged with the eye and ordinal); and the post-draw drain (`WaitForCPUDraw + drain`). The two AI threads' bodies get a scope each on the fragment worker that runs them.

CPU scopes on the draw thread: `RenderEngine::PreDraw`, `DrawGBuffer`, `Draw (scene)`, `DrawPosteffects`, and `PostDraw`; `DrawRenderPassRange`, with the pass range as data; and `RenderPass::DoDraw`, named per pass from the engine's own `GetRenderPassName` table.

Inside each pass the profiler can also open one scope per render-block-type run, named by the type. `CRenderPass::ChangeRenderBlockType` stands in for the engine's compiled-out scope markers ([profiling.md](../../engine/performance/profiling.md) §1.5). That layer is off by default: it is hundreds of scopes per frame, each a lock and a stream push, on the draw-submission path, so it inflates the cost a capture is usually taken to measure. The same hook always adds each finished type run's block count to a per-type counter (a relaxed atomic add against a thread-memoized slot, with no lock and no allocation), and those totals go to the readout and the periodic log line. They undercount by the final run of each pass draw, which the engine closes at the pass tail rather than through a type switch.

### The GPU lane

The GPU lane is a synthetic "GPU" puffin thread built from the engine's own `Graphics::CreateTimeStampQuery`, `SetTimeStampQuery`, and disjoint-query wrappers. Each dispatch wraps its seams (`PreDraw`, `DrawGBuffer`, `Draw (scene)`, `DrawPosteffects`, and `PostDraw`) in timestamp pairs under one disjoint query. The results resolve a few frames later and are reported under a per-dispatch outer scope ("GPU eye 0", "GPU eye 1") that carries the dispatch's busy, starved, and submit numbers as its scope data. The mod's FSR resolve has its own seam (`FSR`) nested inside `DrawPosteffects`. Each post effect the payload hooks and the foveation's two passes get a named detail interval (`profiler::gpu::detail_interval`): they show on the lane beside the passes, but the busy and starved split below does not count them.

GPU ticks are mapped onto the CPU timeline through a CPU reference taken at the dispatch's start, with consecutive dispatches serialized on the lane because the GPU executes them in order, so the lane aligns with, and visibly trails, the CPU work that submitted it. Disjoint intervals are dropped. The lane also carries measured "GPU idle" scopes, the gap between one dispatch's last timestamp and the next one's first, which the analyzer folds into a GPU-utilization figure. Comparing ticks across disjoint brackets is outside D3D11's contract but sound under DXVK's monotonic timestamp clock; implausible gaps are discarded.

The queries also run without the puffin lane while the frame-time benchmark records ([benchmark.md](../testing/benchmark.md)). The benchmark registers itself at startup as the lane's dispatch sink in `jc3vr_render::profiler::gpu_samples`, arms the lane, and receives each resolved dispatch's span, idle gap, submit span, and seam times, so the lane names nothing of the benchmark.

### Reading the GPU lane: work, not span

A dispatch's outer scope, and each seam inside it, is a span: the GPU's whole execution window for that stretch, idle included. If the draw thread cannot record commands as fast as the GPU retires them, the GPU drains inside the span and the scope still reads its full width. A capture can then read as GPU-bound at 13 ms for a frame that is waiting on submission.

So each render pass is bracketed with its own timestamp pair inside its seam, from the `RenderPass::DoDraw` hook. Their sum is the busy figure, and the holes between them are reported as explicit "GPU starved" scopes and totalled per dispatch. Starvation finer than one pass (the GPU idling between individual draws) is invisible at any timestamp granularity the mod can afford and lands inside busy. Busy is therefore an upper bound on GPU work, and starved a lower bound on GPU idle.

Read them against the CPU submit span: the wall time the draw thread spent recording the dispatch. A GPU span that tracks the submit span dispatch after dispatch is a pipeline fed just in time (submission-bound); a GPU span that outruns submission is real shading cost. The per-frame averages of all four numbers go to the log every five seconds while collection is on, and to the readout, so a session can be read without opening a trace.

The per-pass subdivision costs two timestamp queries per pass per dispatch and has its own checkbox; with it off, only the coarse seams remain and busy equals the whole span.

A dispatch's GPU results land in the puffin frame current at read-back time, two to three frames later, so the live view's frame bars read wider than the true frame time. The Chrome trace uses absolute time and is unaffected. Read the lane's durations, not the frame bars.

## Structure

- `crates/jc3vr-core/src/profiler/mod.rs`: the per-frame `new_frame`, the enable state, and the dynamic-scope registry for engine-supplied names.
- `crates/jc3vr-core/src/profiler/blocks.rs`: the per-render-block-type draw counters.
- `crates/jc3vr-core/src/profiler/capture.rs`: the capture state machine, a puffin frame sink.
- `crates/jc3vr-core/src/profiler/chrome_trace.rs`: puffin frames to Chrome trace-event JSON.
- `crates/jc3vr-render/src/profiler/gpu/`: the GPU timestamp ring, the render-pass scopes, the busy and starved decomposition, and the GPU lane's reporting. It registers its own shutdown cleanup at its first dispatch.
- `crates/jc3vr-render/src/profiler/gpu_samples.rs`: what the lane reports of each dispatch to a recording run, in every build.
- `crates/jc3vr-render/src/hooks/profiler.rs`: the profiler-only detours (`DrawGBuffer`, `RenderEngine::Draw`, and `ChangeRenderBlockType`). Seams the mod already hooks for other reasons are instrumented inline in those hooks under `#[cfg(feature = "profiler")]`, among them the sim tick and its phases in `crates/jc3vr-gameplay/src/hooks/sim.rs`, which the frame breakdown times in every build.
- `crates/jc3vr-ui/src/profiler.rs`: the debug UI's collapsible.

The core files do not call into the render or UI halves; each of those depends on the core only.

## Engine facilities not used

- The engine's `Graphics::BeginScopeMarker` and `EndScopeMarker` are not detoured, although many named call sites survive: they are three-byte `ret` stubs, and a failed micro-detour would abort the whole hook library. The per-render-block-type names they carried are recovered from `ChangeRenderBlockType` instead.
- The engine's extern profiler object ([`cpu_profiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::cpu_profiler)) is not read: it holds only zeros.
- The engine's `ProfilerThreadEnter` and `ProfilerThreadLeave` scope primitives ([profiling.md §1.2](../../engine/performance/profiling.md#12-sprofiler-scope-primitives-compiled-out)) are not used. They are not located, their read-back side is stripped, and they depend on the engine's global profiler state. puffin scopes on the same seams give the same attribution without either dependency.
- The stripped display paths (`CFrameProfiler`, the `CProfilerUtil` budget display, which is not located, and the `CGPUProfiler` read-back) are not revived. Rebuilding on the live primitives is less work and does not depend on reconstructing stripped code.
- The engine's `CGPUBufferedQuery` frame ring ([profiling.md §2.2](../../engine/performance/profiling.md#22-cgpubufferedquery--ring-buffered-gpu-queries-live)) is not reused. It belongs to the stripped `CGPUProfiler`: the game only begins and ends a frame-level disjoint query with it, and the per-scope recording and read-back never run.
- `CRenderPassGpuTimingQuery` ([profiling.md §2.3](../../engine/performance/profiling.md#23-crenderpassgputimingquery--live-but-a-single-pass-adaptive-quality-timer)) is not repurposed. It times only the particle pass, and timing another pass with it needs a new instance and new call sites, which the mod's own timestamp ring already provides.
