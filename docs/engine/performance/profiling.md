# Performance-profiling machinery

The Apex engine ships several timing systems. Some are still live (constructed and called); others are compiled out.

The addresses, offsets, and singletons of the bound items are in the pyxis defs, chiefly the [`cpu_profiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/cpu_profiler), [`graphics_engine::gpu_profiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::gpu_profiler), and [`steering_frame_profiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/steering_frame_profiler) modules. Where a claim rests on indirect evidence rather than a decompiled function, it is flagged **[UNVERIFIED]**.

## Summary: live versus dead

| Machinery | Status | What it provides |
|---|---|---|
| `CpuProfiler` (the `cpu_profiler` extern object, 15 fixed frame phases) | **Dead** — `Update` compiled to a nullsub, no phase brackets write the counters; consumers read zeros (§1.1) | — |
| `Graphics::BeginScopeMarker` / `EndScopeMarker` | **Compiled out, but ~72 call sites and their name strings survive** (§1.5) | Named begin/end scope pairs across the whole draw thread, still called with their names into two empty functions |
| `SProfiler` scope primitives (`ProfilerThreadEnter`/`Leave`/`Add`/`AddBudget`) | Compiled out; the IDB symbols with these names are mislabelled (§1.2) | — |
| `Graphics::*TimeStamp*` / `*Disjoint*` query wrappers | **Live**, thin D3D11 wrappers | Raw GPU timestamp + frequency queries |
| `CGPUBufferedQuery` (ring-buffered GPU queries) | **Live** | N-frame-latency timestamp / occlusion / frequency read-back |
| `CRenderPassGpuTimingQuery` | **Live but wired to one pass** (particles) | GPU ms for the particle pass only, drives adaptive quality |
| `CProfilerUtil` singleton (budget store) | Not located; the IDB symbols with its method names are mislabelled (§1.3) | No budget table and no on-screen display |
| `CFrameProfiler` (on-screen frame graph) | **Dead** — no methods present | — |
| Dev console / cvar commands (`debug_DumpProfilerTrace`, `debug_fprof`, `frame_profiler`) and the `Dev\|Performance\|*` debug menu | **Dead** — strings and handlers stripped | — |
| `CSteeringFrameProfiler` (the frame profiler's input steering) | Live, but it only binds the frame profiler's input actions (§1.6) | The `"frameprofiler"` input action map; no timing |

---

## 1. CPU side

### 1.1 `CpuProfiler` — the fixed 15-phase frame breakdown (dead)

The profiler is the statically allocated object [`cpu_profiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::cpu_profiler), not a pointer to one. Its location is pinned by the `this` pointer passed to `GetScopeName` in `BorkReport::WriteMetrics`, and by the direct `m_Time` reads in `CBorkReport::UpdateMetricsData`. The machinery is dead. [`CpuProfiler::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::CpuProfiler##func-Update) is compiled to an empty function, which is still called at the top of [`Game::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Update). No surviving code writes [`CpuProfiler::m_Counters`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::CpuProfiler##field-m_Counters): the phase brackets that `CGame::UpdateCPUProfiler` and the game states would place are compiled out. The only interior reference is [`GraphicsEngine::DispatchDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-DispatchDraw), which forwards the never-incremented [`CpuProfiler::m_Index`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::CpuProfiler##field-m_Index) to the draw task. [`BorkReport`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/bork_report::BorkReport) still serialises the per-phase values into telemetry, and all of them are zero. The profiler therefore holds no timing data. The frame phases run in `Game::Update`, [`Game::UpdateGame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateGame), [`Game::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateRender), [`Game::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Draw), and [`GraphicsEngine::WaitForCPUDrawToFinish`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-WaitForCPUDrawToFinish) on the main thread, and in the draw-thread seams.

The engine keeps a single global `CpuProfiler` (`g_CpuProfiler`) that accumulates wall-clock time for a fixed set of 15 frame phases, the [`CpuScopeId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::CpuScopeId) enum:

```
CPU_SCOPE_ID_FRAME, DRAW, UPDATE_ALL, RENDER_ALL, PREUPDATE,
PRESIM_SYSTEMS, PRESIM_OBJECTS, PHYSICS_UPDATE, POSTSIM_UPDATE, POSTSIM_SYSTEMS,
WAIT_FRAME, RENDER_UPDATE, RENDER_SYSTEMS, WAIT_FLIP, WAIT_UI
```

The struct layout is bound as [`CpuProfiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::CpuProfiler): the last frame's milliseconds per phase, a 30-frame peak per phase, the ring position, the triple-buffered QPC begin/end counters, and the running local peak.

`CpuProfiler::Update` converts the QPC counter deltas to milliseconds (`1000.0 / QueryPerformanceFrequency`) into `m_Time[]` and rolls a 30-frame peak into `m_Peak[]`. The binary retains the scope-name accessor and the QPC accumulator:

- [`CpuProfiler::GetScopeName`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/cpu_profiler::CpuProfiler##func-GetScopeName) returns the phase's entry in a static 15-entry `const char*[]` scope-name table.

The consumers are [`BorkReport::WriteMetrics`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/bork_report::BorkReport##func-WriteMetrics) and [`BorkReport::WriteTotalEntry`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/bork_report::BorkReport##func-WriteTotalEntry), part of Avalanche's crash and telemetry report. Both call `GetScopeName` to serialise the per-phase CPU times into the report, and they serialise zeros. **[UNVERIFIED]** `CGame::UpdateCPUProfiler` is not yet located. It is reachable from `Game::Update` and from the `GetScopeName` call sites.

### 1.2 `SProfiler` scope primitives (compiled out)

Underneath `CpuProfiler` is the general Apex scope profiler `SProfiler`. It is a per-thread, hierarchical enter/leave timer with named entries (`SProfilerEntryData { volatile int m_ID; float m_Budget; const char* m_Name; }`) that are lazily allocated and registered per module. Its free-function entry points are:

| Function | Role |
|---|---|
| `ProfilerThreadEnter(entryId, startQPC)` | push a scope on the current thread |
| `ProfilerThreadLeave(entryId, endQPC)` | pop the scope |
| `ProfilerThreadAdd(entryId, deltaQPC)` | add a raw duration to an entry |
| `ProfilerThreadAddBudget(entryId, deltaQPC, budget)` | add with a budget comparison |
| `ProfilerGetProfilerThreadID()` | current thread's profiler id |
| `ProfilerInitThread(name)` | register a thread with the profiler |

These primitives are compiled out. The IDB places each of these names on a function, but those bodies are unrelated code (a sound-resource critical section, a pose-producer blender update, an AI event ring, and similar). Two further symbols that look like identical-code-folded copies of `ProfilerThreadLeave` and `ProfilerThreadAddBudget` are also mislabelled. None of these functions is bound, and none may be called as a profiler entry point.

The strings `"CGame::UpdateCPUProfiler"` and `"ProfilerInitThread"` survive in `.data` records that reference them. **[UNVERIFIED]** These records do not match the `SProfilerEntryData` layout, nothing references them, and their purpose is not established. The read-back and display side is stripped (§1.3).

### 1.3 `CProfilerUtil` — not located

`CProfilerUtil` is the `Base::CSingle<CProfilerUtil>` singleton for the whole developer-facing performance UI: a budget table (`SBudgetEntry m_Budget[64]`, loaded from `settings/budgetscopes.txt`), an on-screen budget/hitch display, a captured-trace writer to `budget_violations.json`, a pointer to a `CFrameProfiler`, and a `CDumpProfilerTraceCmd` console command. Its per-frame entry points, called from `CGame`, are `CProfilerUtil::PreUpdate` / `Update` / `Render` / `SetDisplayBudgetScopes`.

**[UNVERIFIED]** No part of `CProfilerUtil` is confirmed. The IDB labels three functions with its names, and all three are mislabelled:

- The function labelled `CProfilerUtil::LoadBudget` is a recursive runtime-container walk that builds objects, called from [`resource_loader::CreateObjectsAsync`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/resource_loader##func-CreateObjectsAsync). It does not load a budget table.
- The function labelled `~CProfilerUtil`, called from [`Game::Destructor`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Destructor), unsubscribes eight `NEvent` handlers of an unrelated singleton.
- The function labelled `Base::CSingle<CProfilerUtil>::Release` releases that same unrelated singleton.

What is **stripped**: `PreUpdate`, `Update`, `Render`, `UpdateBudgetsOnScreen[QA]`, `HandleUserInput`, `SetDisplayBudgetScopes`, `DoCapture`, `HitchCallback`, and the `CDumpProfilerTraceCmd` handler — none appear in the IDB. The corroborating string evidence: the `"Dev|Performance|Main Thread Budgets"` / `"Dev|Performance|Show Budget Scopes"` debug-menu paths, the `"debug_DumpProfilerTrace"` / `"debug_fprof"` / `"frame_profiler"` console-command names, and `"settings/budgetscopes.txt"` are **all absent** from the IDB's string table (which *does* still contain `"CGame::UpdateCPUProfiler"`, `"ProfilerInitThread"`, and the `PROFILER_*` input-action names, so the strings are being identified — the missing ones are genuinely compiled out).

The game has no budget table load, no budget comparison, no interactive display, no console command, and no debug menu.

### 1.4 `CFrameProfiler` and the dev console — dead

`CFrameProfiler` is the classic on-screen frame-time graph (category rows, history buffer, sort modes, an `m_AsyncGPUContext` GPU-timing lane). **No `CFrameProfiler` methods exist in the binary** — it is fully stripped. The developer console / cvar commands that would toggle it are likewise gone (the command-name strings above are absent). There is nothing to flip that would bring the on-screen frame graph back.

### 1.5 `Graphics::BeginScopeMarker` / `EndScopeMarker` — compiled out, call sites intact

The renderer is instrumented throughout with named scope markers: [`BeginScopeMarker`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-BeginScopeMarker) (`Graphics::BeginScopeMarker(HContext_t*, const char*)`) opens a named scope on a graphics context and [`EndScopeMarker`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-EndScopeMarker) (`Graphics::EndScopeMarker(HContext_t*)`) closes the innermost one. The RAII wrapper [`ScopeMarker`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gpu_profiler::ScopeMarker) (`Graphics::CScopeMarker`) calls the pair from its [constructor](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gpu_profiler::ScopeMarker##func-Constructor) and [destructor](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gpu_profiler::ScopeMarker##func-Destructor). Both entry points are compiled to empty functions — but, unlike the `CFrameProfiler` display path, the *call sites and their name strings survive*: ~72 sites covering the frame stages in `CGraphicsEngine::HandleDrawThreadTask` ("Frame setup", "CopyEffectTextures", "Debug UI"), the whole post-effect stack (SMAA/FXAA, depth of field, motion blur, lens flare, glare, tone-mapping histogram), deferred lighting, SSAO, SSR, environment reflections, water, terrain restores, UI and video off-screen rendering — and the per-render-block-type runs inside every pass: [`RenderPass::ChangeRenderBlockType`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-ChangeRenderBlockType) closes the previous type's scope and opens the next named by the type's `GetTypeName()`, with [`RenderPass::DoDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-DoDraw) closing the last run. Every surviving site still executes its call each frame and passes its scope name to the empty bodies. Both markers and the timestamp wrappers are captured in `graphics_engine.pyxis`.

### 1.6 `CSteeringFrameProfiler` — the frame profiler's input steering

One piece of the frame profiler survives with live code: [`SteeringFrameProfiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/steering_frame_profiler::SteeringFrameProfiler) (`CSteeringFrameProfiler`). It is an input steering, not a profiler: it binds the `"frameprofiler"` input action map, whose actions navigate, sort, pause, and close the on-screen frame profiler. It does not time anything. [`SteeringFrameProfiler::LoadActionMap`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/steering_frame_profiler::SteeringFrameProfiler##func-LoadActionMap) creates the action map (the sole code reference to that string), and [`GetFrameProfilerInputs`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/steering_frame_profiler##func-GetFrameProfilerInputs), [`FrameProfilerOverridePlayerInputs`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/steering_frame_profiler##func-FrameProfilerOverridePlayerInputs), and the [constructor](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/steering_frame_profiler::SteeringFrameProfiler##func-Constructor) are all present. The `CFrameProfiler` display that would consume these inputs is stripped (§1.4).

---

## 2. GPU side

### 2.1 `Graphics::*` timestamp and disjoint-query wrappers (live)

The engine ships thin, live wrappers over D3D11 timestamp queries. `CreateTimeStampQuery` builds a `D3D11_QUERY_DESC` with `Query = 2` (`D3D11_QUERY_TIMESTAMP`) and calls the device's `CreateQuery` (device vtable `+192`); the disjoint variant uses `Query = 3` (`D3D11_QUERY_TIMESTAMP_DISJOINT`). The wrappers are bound in the [`graphics_engine::graphics_engine`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine) module:

| Function | Notes |
|---|---|
| [`CreateTimeStampQuery`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-CreateTimeStampQuery) | |
| [`DestroyTimeStampQuery`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-DestroyTimeStampQuery) | |
| [`SetTimeStampQuery`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-SetTimeStampQuery) | records a timestamp on a context |
| [`QueryTimeStamp`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-QueryTimeStamp) | reads the u64 tick |
| [`CreateTimeStampDisjointQuery`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-CreateTimeStampDisjointQuery) | |
| [`DestroyTimeStampDisjointQuery`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-DestroyTimeStampDisjointQuery) | |
| [`BeginTimeStampDisjointQuery`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-BeginTimeStampDisjointQuery) | |
| [`EndTimeStampDisjointQuery`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-EndTimeStampDisjointQuery) | identical-code-folded with `SetTimeStampQuery` (both bodies are `ID3D11DeviceContext::End` under the context mutex), so the two share an address; the IDB places this name on an unrelated 3-byte stub, which must not be called |
| [`QueryTimeStampFrequency`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-QueryTimeStampFrequency) | reads freq + `Disjoint` flag |

### 2.2 `CGPUBufferedQuery` — ring-buffered GPU queries (live)

[`GPUBufferedQuery`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gpu_profiler::GPUBufferedQuery) (`NGraphicsEngine::CGPUBufferedQuery`) is the engine's general N-frame-latency query ring, built on the wrappers above. It is fully present: `Create`, `Destroy`, `SetTimestamp`, `SetFence`, `SetOcclusionBegin` / `SetOcclusionEnd`, `SetFrequencyBegin`, `GetOcclusionResult`, and `GetFrequencyResult` are all bound on the type.

It holds `m_Timestamps[]` and `m_Frequency[]` arrays sized `m_NumBuffers * m_NumTimestamps`, so a caller can record many timestamps per frame and read them back `m_NumBuffers` frames later without a GPU stall.

The engine's own query ring belongs to `NGraphicsEngine::CGPUProfiler`, bound as the singleton [`GPUProfiler`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gpu_profiler::GPUProfiler). It is the engine's full developer GPU profiler (per-scope GPU + CPU time, occlusion, draw-call counts; scope types frame/render-pass/render-block/user; a rolling 10-frame capture mode; per-category results over the 30 `ERenderPassCategory` groups). What survives: the [constructor](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gpu_profiler::GPUProfiler##func-Constructor), called from [`GraphicsEngine::InitializeSystem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-InitializeSystem), [`GPUProfiler::GetScopeResultCount`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gpu_profiler::GPUProfiler##func-GetScopeResultCount), and a frame-level disjoint-query begin/end in `HandleDrawThreadTask`. `CGPUProfiler::Init` is not yet located; the function the IDB labels with that name is [`ShadowManager::InitializeSystem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::ShadowManager##func-InitializeSystem). What is stripped: the per-scope `BeginScope`/`EndScope` call sites (the scope-marker nullsubs of §1.5 are their entry points), the results processing, and the read-back — [`GPUProfiler::GetScopeResult`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gpu_profiler::GPUProfiler##func-GetScopeResult) is compiled to `xor eax, eax; ret`, so `CBorkReport`'s per-category GPU metrics loop exits immediately. Flipping state does not revive the class. Several IDB symbols in this area are misplaced by identical-code folding, and their names are not reliable. The body labelled `BeginScope` is [`CopySurfaceToTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-CopySurfaceToTexture), per [`rendering.md` §12, "Per-eye output routing"](../rendering/rendering.md#12-per-eye-output-routing). The body labelled `EndScope` is [`SkinningPaletteData::UpdateDeleteQueue`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::skinning_palette::SkinningPaletteData##func-UpdateDeleteQueue), the skinning palette's deferred vertex-buffer delete queue.

### 2.3 `CRenderPassGpuTimingQuery` — live, but a single-pass adaptive-quality timer

There *is* a per-pass GPU timer in the render engine — but it is dedicated to **one** pass. [`RenderPassGpuTimingQuery`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::gpu_profiler::RenderPassGpuTimingQuery) (`NGraphicsEngine::CRenderPassGpuTimingQuery`) triple/quad-buffers timestamp-start/end + frequency queries and computes a rolling average (`m_ParticleGPUTimingHistory[10]`). `Create`, `Destroy`, `BeginGPUTimingQuery`, `EndGPUTimingQuery`, and `GetAverageGpuTiming` are all bound on the type.

The engine owns exactly one instance, [`RenderEngine::m_ParticlePassGpuTimingQuery`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_ParticlePassGpuTimingQuery), on the `RenderEngine` singleton. It is driven from [`RenderEngine::BeginParticlePassGpuTimingQuery`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-BeginParticlePassGpuTimingQuery), which is called from [`RenderPass::DoDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-DoDraw) and pairs with an `EndGPUTimingQuery` at the tail of the particle draw. `GetAverageGpuTiming`'s result adapts [`RenderEngine::m_TessFactorScale`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_TessFactorScale) and sets [`RenderEngine::m_ParticlePassGpuIntense`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_ParticlePassGpuIntense) against the thresholds those fields document — i.e. this is an **adaptive particle-quality** system, not a general profiler. It measures only particles and its output is a quality flag, not a readable per-pass table.

Timing another pass with the class requires a new instance and new begin/end call sites.

---

## 3. Per-pass structure

The pass system and its coarse pass groups are documented in
[`rendering.md` §3, "Render passes / stages (ordered)"](../rendering/rendering.md#3-render-passes--stages-ordered),
and the discrete calls that draw each group (`DrawGBuffer`, `Draw`, `DrawPosteffects`, and
`PostDraw`) in [`rendering.md` §1.4, "Body of `HandleDrawThreadTask`"](../rendering/rendering.md#14-body-of-handledrawthreadtask--per-dispatch).
There is **no live general per-pass GPU or CPU bookkeeping** to read (the only per-pass GPU
timer is the particle one in §2.3).
