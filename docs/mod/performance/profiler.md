# The in-game profiler (issue #34)

The mod carries its own frame profiler because the engine's is dead: the
[`CpuProfiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::CpuProfiler), with one phase per [`CpuScopeId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::CpuScopeId), is never written (`docs/engine/performance/profiling.md` §1.1), the `CGPUProfiler` read-back is
stubbed (§2.2), and the on-screen frame graph is stripped (§1.4). What survives — the live D3D11
timestamp-query wrappers and the hookable frame-phase functions — is exactly enough to rebuild the
pipeline mod-side, which is what `payload/src/profiler/` does on top of
[puffin](https://github.com/EmbarkStudios/puffin).

Everything is compiled under the `profiler` cargo feature (default on). With the feature on but the
profiler idle (panel closed, no capture), each instrumented site costs a relaxed atomic load (plus
a few thread-local touches per dispatch for the lane labels and the type-run slot); with the
feature off, the hooks and scopes do not exist.

## Using it

- **Performance tab → "Profiler (issue #34)"**: a checkbox enables scope collection and shows
  puffin's live flame graph; a button captures ~5 s of frames. Two further checkboxes control the
  fine layers — the per-draw render-block scopes (off by default, because they inflate the
  draw-submission path they measure) and the GPU per-pass timestamps (on by default, because they
  are what separates GPU work from GPU starvation). The rolling GPU busy/starved/submit readout
  and the per-render-block-type draw counts sit under them.
- **F9** starts the same capture without the overlay (usable in-headset). Progress shows in the
  collapsible; the result is logged.
- Captures are written to the session's `profile/` directory
  (`sessions/<timestamp>/profile/jc3vr-profile-<stamp>.json`) in Chrome trace-event format — open
  in [ui.perfetto.dev](https://ui.perfetto.dev) (or `chrome://tracing`). Each puffin thread is a
  named lane ("game", "draw", "GPU"); timestamps are rebased to the capture start. Serialization
  runs on a background thread (a capture is tens of megabytes of JSON), so the game keeps rendering
  while the file is written.

## Taking a capture

1. Build with the feature. It is a default feature, so the ordinary build has it: `scripts/proton_run.sh --release` builds and injects the release payload. Capture in release; a debug payload's numbers describe the debug build. `cargo xwin build ... -p jc3vr_payload --no-default-features` builds without it, which is the comparison to make when the question is the profiler's own overhead.
2. In the game, get to the situation you want measured, then press F9. The log says `F9: profiler trace capture started` and `profiler: capturing 5.0s of frames`, and, once the file is written, the path. Hold the situation for the whole five seconds.
3. Leave the per-draw render-block scopes off unless the per-type timeline is the question (see below). The GPU per-pass timestamps are on by default and should stay on.
4. The capture lands in the session folder beside the payload DLL, `target/x86_64-pc-windows-msvc/release/sessions/<timestamp>/profile/jc3vr-profile-<stamp>.json`.
5. Summarize it with `uv run tools/analyze_profile.py <file>`, or open it in [ui.perfetto.dev](https://ui.perfetto.dev).

Take the capture alongside the always-on breakdown in the same log ([frame-timing.md](frame-timing.md)): the breakdown says which part of the frame grew, and the capture shows what is inside it.

## What is instrumented

CPU scopes, main thread: `CGame::Update` (the frame), `CGame::UpdateGame` (each sim tick) and, inside it, the game world's two update queues, the physics phase, and the streaming block's three calls, `UpdateRender`, the per-eye dispatch loop (one `Dispatch` scope per `CGame::Draw`, tagged with the
eye and ordinal), and the post-Draw drain (`WaitForCPUDraw + drain`). The two AI threads' bodies get a scope each on the fragment worker that runs them.

CPU scopes, draw thread: `RenderEngine::PreDraw` / `DrawGBuffer` / `Draw (scene)` /
`DrawPosteffects` / `PostDraw`, `DrawRenderPassRange` (with the pass range as data), and
`RenderPass::DoDraw` (named per pass via the engine's own `GetRenderPassName` table).

Inside each pass there is also one scope per render-block-type run, named by the type
(`CRenderPass::ChangeRenderBlockType` mirrors the engine's own compiled-out scope markers; see
`docs/engine/performance/profiling.md` §1.5) — but that layer is **off by default**. It is hundreds of scopes
per frame, each a lock and a stream push, sitting on the draw-submission path, so leaving it on
inflates the cost a capture is usually taken to measure. The checkbox next to the enable toggle
turns it on for the cases where the per-type timeline is actually the question.

What survives unconditionally is the cheap part of its signal: the same hook adds each finished
type run's block count to a per-type counter (a relaxed atomic add against a thread-memoized slot,
no lock and no allocation), and those totals go to the Performance tab and the periodic log line.
They undercount by the final run of each pass draw, which the engine closes at the pass tail rather
than through a type switch.

GPU lane: a synthetic "GPU" puffin thread built from the engine's own
`Graphics::CreateTimeStampQuery` / `SetTimeStampQuery` / disjoint-query wrappers. Each dispatch
wraps its seams (`PreDraw`, `DrawGBuffer`, `Draw (scene)`, `DrawPosteffects`, `PostDraw`) in
timestamp pairs under one disjoint query, and the resolved results are reported a few frames later
under a per-dispatch outer scope ("GPU eye 0" / "GPU eye 1" / "GPU far field"), which also carries
the dispatch's busy/starved/submit numbers as its scope data. GPU ticks are
mapped onto the CPU timeline via a CPU reference taken at the dispatch's start, with consecutive
dispatches serialized on the lane (the GPU executes them in order), so the lane aligns with — and
visibly trails — the CPU work that submitted it. Disjoint intervals are dropped rather than
reported. The lane also carries measured "GPU idle" scopes — the true starvation gap between one
dispatch's last timestamp and the next one's first — so the cost of the serialized dispatch
pipeline shows up as a number (the analyzer folds them into a GPU-utilization figure) rather than
an inference. Comparing ticks across disjoint brackets is out of contract for D3D11 but sound
under DXVK's monotonic timestamp clock; implausible gaps are discarded.

### Reading the GPU lane: work, not span

A dispatch's outer scope, and each seam inside it, is a *span*: the GPU's whole execution window
for that stretch, idle included. If the draw thread cannot record commands as fast as the GPU
retires them, the GPU drains **inside** the span and the scope still reads its full width — which
is how a capture can read "the GPU is the ceiling at 13 ms" for a frame that is actually
waiting on submission.

So each render pass is bracketed with its own timestamp pair inside its seam (from the
`RenderPass::DoDraw` hook). Summing those gives a **busy** figure; the holes between them are
reported as explicit **"GPU starved"** scopes and totalled per dispatch. Both are bounds, and it
matters which way: starvation finer than one pass — the GPU idling between individual draws — is
invisible to any timestamp granularity the mod can afford, and lands inside busy. **Busy is an
upper bound on real GPU work; starved is a lower bound on real GPU idle.**

The number to read them against is the **CPU submit span**: the wall time the draw thread spent
recording the dispatch, measured between the dispatch's begin and end and reported alongside. A GPU
span that tracks the submit span dispatch after dispatch is a pipeline being fed just in time
(submission-bound); a GPU span that outruns submission is real shading cost. The per-frame
averages of all four numbers go to the log every five seconds while collection is on, and to the
Performance tab, so a session can be read without opening a trace at all.

The per-pass subdivision costs two timestamp queries per pass per dispatch and has its own
checkbox; turning it off leaves the coarse seams, and busy degenerates to the whole span.

One known cosmetic effect: because a dispatch's GPU results land in the puffin frame
current at read-back time (~2-3 frames later), the live view's frame bars read wider than the true
frame time; the Chrome trace, being absolute-time, is unaffected. Trust the lane's durations, not
the frame bars.

## Structure

- `payload/src/profiler/mod.rs` — the switchboard: the per-frame `new_frame`, the enable state,
  and the dynamic-scope registry for engine-supplied names.
- `payload/src/profiler/gpu/` — the GPU timestamp ring, the busy/starved decomposition, and the
  puffin GPU-lane reporting.
- `payload/src/profiler/blocks.rs` — the per-render-block-type draw counters.
- `payload/src/profiler/capture.rs` — the 5 s capture state machine (a puffin frame sink).
- `payload/src/profiler/chrome_trace.rs` — puffin frames → Chrome trace-event JSON.
- `payload/src/profiler/ui.rs` — the Performance-tab collapsible.
- `payload/src/hooks/profiler.rs` — the profiler-only detours (`DrawGBuffer`,
  `RenderEngine::Draw`, `ChangeRenderBlockType`); seams the mod already hooks
  for other reasons are instrumented inline in those hooks under `#[cfg(feature = "profiler")]`,
  among them the sim tick and its phases in `payload/src/hooks/sim.rs`, which the frame breakdown times in every build.

## Deliberate omissions

- The engine's `Graphics::Begin/EndScopeMarker` nullsubs are not detoured, although their ~72
  named call sites survive: they are three-byte `ret` stubs, and a failed micro-detour would abort
  the whole hook library. The highest-value names they carried (the per-render-block-type runs)
  are recovered from `ChangeRenderBlockType` instead; the remaining post-effect-internal names are
  a possible future extension if the coarse seams prove too blunt.
- The engine's extern profiler object ([`cpu_profiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::cpu_profiler), `g_CpuProfiler`) is not read: it holds only zeros.
- The engine's `ProfilerThreadEnter` / `ProfilerThreadLeave` scope primitives ([`profiling.md` §1.2](../../engine/performance/profiling.md#12-sprofiler-scope-primitives-compiled-out)) are not used. They are not yet located, and the IDB functions carrying those names are unrelated code. Their read-back side is stripped, so scopes recorded through them would
  also need a raw read of the entry table, and they depend on the engine's global profiler state.
  puffin scopes on the same seams give the same attribution without either dependency.
- The stripped display paths (`CFrameProfiler`, the `CProfilerUtil` budget display (not yet located), and the
  `CGPUProfiler` read-back) are not revived. The rebuild on the live primitives is less work and
  does not depend on reconstructing stripped code.
- The engine's own `CGPUBufferedQuery` frame ring
  ([`profiling.md` §2.2](../../engine/performance/profiling.md#22-cgpubufferedquery--ring-buffered-gpu-queries-live))
  is not reused. It belongs to the stripped `CGPUProfiler`: the game only begins and ends a
  frame-level disjoint query with it, and the per-scope recording and read-back never run. The GPU
  lane issues its own queries through the `Graphics::*` wrappers instead.
- `CRenderPassGpuTimingQuery`
  ([`profiling.md` §2.3](../../engine/performance/profiling.md#23-crenderpassgputimingquery--live-but-a-single-pass-adaptive-quality-timer))
  is not repurposed. It times only the particle pass, and timing another pass with it needs a new
  instance and new begin/end call sites, which the mod's own timestamp ring already provides.
