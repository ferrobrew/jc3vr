# JC3 (Apex engine) per-frame rendering pipeline — authoritative reference

Where a claim could not be confirmed it is flagged **[UNVERIFIED]**. The concrete addresses, struct offsets, singletons and globals all live in the pyxis-defs (the `graphics_engine/`, `camera/`, `ui/` modules); this doc is the wiring — what each thing is, what reads what, and the frame order. Reference everything here by name and follow the name into the defs for the exact address/offset.

For the shader side — extracting the DXBC from the bundles, disassembling it, finding a specific effect, and how the engine creates and reloads shaders — see [`shaders.md`](shaders.md). How the mod patches shaders in memory is in [`shader-patching.md`](../../mod/rendering/shader-patching.md).

Key singletons:
- [`GraphicsEngine`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine) and [`RenderEngine`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine): two separate singletons. `GraphicsEngine` owns the scene render targets, the main render context, and the render camera; `RenderEngine` owns the pass array and the global shader-constant staging blocks.
- [`CameraManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera_manager::CameraManager) (engine-side) instance.
- [`GameCameraManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::game_camera_manager::GameCameraManager) instance: a distinct singleton (`Base::CSingle<GameCameraManager>::Instance`), separate from the engine-side `CameraManager` it drives.
- [`Clock`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock) instance.

---

## 1. Threading & frame structure

### 1.1 Two threads

- Main / sim thread runs `Game::Update` -> `Game::UpdateRender` -> `Game::Draw` -> `GraphicsEngine::Draw`.
- Render / draw worker thread runs `GraphicsEngine::HandleDrawThreadTask`, which builds and submits the actual GPU command stream for the frame.

The split is decided in `GraphicsEngine::DispatchDraw`:

```c
if ( CpuPrimaryCount() <= 1 )
    return HandleDrawThreadTask(a1, &param);                   // inline, same thread
// else: post a fragment job "CallProxy" to the worker pool
v2 = HashString("CallProxy");
CpuFragmentCall(v2, v4 /* {fn=HandleDrawThreadTaskProxy, ctx, signal} */, 24, &v5, 0);
```

So with >1 CPU primary, `DispatchDraw` enqueues a `CpuFragmentCall` whose function, [`HandleDrawThreadTaskProxy`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-HandleDrawThreadTaskProxy), tail-calls `HandleDrawThreadTask` on a worker; with a single primary it runs inline. The work performed is identical either way — only the thread differs.

### 1.2 What `WaitForCPUDrawToFinish` drains

```c
if ( a1->m_DrawFragmentSignal ) CpuFragmentWaitUntilSignalIsNonZero();  // wait on the shared_ptr<SCallProxyKeepAlive> end-draw signal
if ( !a1->m_CPUDrawFinished )                                            // the "draw done" event-set flag
    WaitForSingleObject(a1->m_CPUFinishedDrawingEvent, INFINITE);        // wait on the draw-done Win32 event
```

`GraphicsEngine::WaitForCPUDrawToFinish` blocks until the previous dispatched `HandleDrawThreadTask` has finished (the end-of-draw `SetEvent` at the tail of `HandleDrawThreadTask` sets [`GraphicsEngine::m_CPUFinishedDrawingEvent`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_CPUFinishedDrawingEvent) and its flag [`GraphicsEngine::m_CPUDrawFinished`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_CPUDrawFinished); the fragment signal is [`GraphicsEngine::m_DrawFragmentSignal`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DrawFragmentSignal)). It is the fence between the render worker and the next frame's sim work. `Game::Draw` calls it at entry and `GraphicsEngine::Draw` re-checks the same two conditions in its prologue.

### 1.3 Prologue of `GraphicsEngine::Draw` — per-real-frame-once state

Executed once per real frame, in order:

| Operation | Notes |
|---|---|
| wait on the fragment signal + draw-done event | drain previous frame |
| `m_RenderContext.m_Dt = dt` ([`RenderContext::m_Dt`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_Dt)) | store the frame dt for the worker |
| `render_frame_counters.m_FrameIndex = render_frame_counters.m_Counter++` | **RenderFrameCounter / parity source**: `m_FrameIndex` = previous value of `m_Counter`, then `m_Counter` increments |
| [`render_frame_index`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::render_frame_index) `= m_Counter % 3` | a `%3` ring index |
| [`previous_render_context_frame_index`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::previous_render_context_frame_index) `= m_RingIndex` | save previous |
| `render_frame_counters.m_RingIndex = m_FrameIndex % 3` | the per-frame `%3` ring used by RBI/CB indices |
| `++m_CurrBackBufferCopy`, wrapping to 0 at `m_BackBufferCopyCount` | [`RenderEngine::m_CurrBackBufferCopy`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_CurrBackBufferCopy) is the current slot of [`RenderEngine::m_BackBufferCopy`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_BackBufferCopy), the ring of scene-colour copies that serves as the screen-space refraction source. It is not a ring of presentable buffers; those are owned by the DXGI swapchain (§7). |
| `TextureCachePreUpdate` | texture streaming bookkeeping |
| `CGameStateRun::UpdatePostEffectEdit` | post-effect-edit (dev) sync |
| [`GraphicsEngine::TextureCachePlatformUpdate(engine, masterCtx)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-TextureCachePlatformUpdate) | **the camera copy + render-camera setup happens here — see §2** |
| `Clock::Update` | **the per-frame clock tick** (the slow-mo hazard, see §5.4) |
| `CConstantBufferPool::HandBackBuffers` | CB pool ring rotation |
| `CRenderEngine::CalculateConstantBufferIndices` | per-RBI CB draw indices |
| `DispatchDraw` | hand the frame to the worker |

The present (Flip) is in this prologue, between `TextureCachePlatformUpdate` and `Clock::Update`: `if (!engine->m_FirstFrame) GraphicsEngine::Flip(engine); engine->m_FirstFrame = 0;` — [`GraphicsEngine::Flip`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-Flip) does `AcquireThreadOwnership; graphics_flip(device); ReleaseThreadOwnership` (verified: it calls `graphics_flip` in its body). So the prologue presents the previous frame's back buffer, guarded one-shot by the [`GraphicsEngine::m_FirstFrame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_FirstFrame) flag (set elsewhere to skip the very first frame). The present is pipelined: Flip-previous, then `DispatchDraw` the current frame. The mod's `BLOCK_FLIP` suppresses `graphics_flip` — hooking it directly is correct since this prologue reaches it through `GraphicsEngine::Flip`.

### 1.4 Body of `HandleDrawThreadTask` — per-dispatch

High-level order of [`GraphicsEngine::HandleDrawThreadTask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleDrawThreadTask):

1. `QueryPerformanceCounter` -> [`draw_start_time`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::draw_start_time).
2. get master context (`GetMasterContext`) -> `m_RenderContext.m_Context` ([`RenderContext::m_Context`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_Context)).
3. `ShadowManager::CommitRenderPassSettings` (if [`shadow_system_enabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::shadow_system_enabled), i.e. shadows enabled).
4. `TextureCacheGpuUpdate`.
5. `RenderPass::SetupRenderContext(&m_RenderContext, 0, ctx)` — [`GraphicsEngine::m_RenderContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_RenderContext) is the main `RenderContext`.
6. `RenderPass::SetRenderContextCamera(&m_RenderContext, CameraManager.m_RenderCamera)` — feeds the render camera (see §2) into the render context.
7. `RenderEngine::SetGlobalShaderConstants(&m_RenderContext)` — uploads per-view CB.
8. `SetAllGlobalShaderProgramConstants`.
9. If [`GraphicsEngine::m_DrawScene`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DrawScene) is set:
   - UI off-screen textures and render kick-off ([`UIManager::RenderOffScreenTextures`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-RenderOffScreenTextures), then [`UIManager::StartRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-StartRender), both virtual calls on the `GetIUIManager` instance).
   - `PreDraw`.
   - `RenderEngine::DrawGBuffer` (passes `RP_Z_OCCLUDERS..RP_LAST_GBUFFER`).
   - **EffectInfo slot machine** (see §5.1).
   - `RenderEngine::Draw` (passes `RP_REFLECTIVE_WATER_PLANES..RP_PARTICLE_ONSCREEN`: lighting/SSR/reflection/main).
   - `PostEffectsManager::ApplyWorldFilters(dt, ...)`.
   - `RenderEngine::DrawPosteffects` (pass `RP_POSTEFFECTS`).
   - else (no main draw): just UI submit.
10. `PostEffectsManager::ApplyGlobalFilters(dt, ctx)`.
11. `RenderEngine::PostDraw`.
12. GPU-profiler `EndFrame` (one of two countdown branches), screenshot countdown [`GraphicsEngine::m_FramesUntilScreenshot`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_FramesUntilScreenshot).
13. UI render block (clear, then [`UIManager::RenderStaticBackGround`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-RenderStaticBackGround), [`UIManager::SyncRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-SyncRender), and [`UIManager::Submit`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-Submit)).
14. `QuickDrawController::Draw` (debug overlays).
15. `Graphics::EndDraw(m_RenderContext.m_Context)` — submits the frame's recorded command work. **Not** a copy to the presentable surface; see §12 for the correction and the mislabelling behind it.
16. end-draw one-shot signal callbacks (`sig::Signal`) — self-removing.
17. `RenderEngine::EraseAllDeletedRenderBlocks`.
18. compute draw-time floats ([`GraphicsEngine::m_CPUDrawStallDuration`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_CPUDrawStallDuration), [`GraphicsEngine::m_DrawTime`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DrawTime)).
19. `m_CPUDrawFinished = 1; SetEvent(m_CPUFinishedDrawingEvent)` — signal draw-done.

### 1.5 Per-frame vs per-dispatch

- **Per real frame (once), in `GraphicsEngine::Draw` prologue:** frame counter/parity (`render_frame_counters`), `%3` ring, the scene-colour copy ring slot (`RenderEngine::m_CurrBackBufferCopy`), texture-cache pre/platform update, the active->render camera copy, `Clock::Update`, CB pool `HandBackBuffers`, CB indices.
- **Per dispatch (would run twice if you Draw twice), in `HandleDrawThreadTask`:** the entire render pass chain, the EffectInfo slot machine (§5.1), `ApplyWorldFilters`/`ApplyGlobalFilters` accumulators, screenshot countdown, profiler frame begin/end, the final copy + end-draw callbacks.

---

## 2. Camera data flow

### 2.1 Cast of cameras

- `GameCameraManager` holds `m_ControlContext` with four sub-contexts (Previous/Next x Camera/Render); these carry `m_CameraTransform` (a world transform, not a view matrix), FOV, blur factors.
- `CameraManager` (engine) holds:
  - [`m_ActiveCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera_manager::CameraManager##field-m_ActiveCamera): the active scene camera (a [`Camera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera)).
  - [`m_RenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera_manager::CameraManager##field-m_RenderCamera): the camera the render thread reads.
- A `Camera`'s matrix/flag layout is in the `Camera` def (`m_PreviousTransformF`, `m_TransformF`, `m_TransformT0`, `m_TransformT1`, `m_ProjectionF`, `m_ViewProjectionF`, `m_Projection`, `m_View`, `m_ViewProjection`, ...). The [`Camera::m_StateBitfield`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##field-m_StateBitfield) ([`CameraState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState)) carries [`m_ComputeView`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState##flag-m_ComputeView) (compute-view-from-transform), [`m_DirtyProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState##flag-m_DirtyProjection), and [`m_IsRenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState##flag-m_IsRenderCamera) (the SetupRenderCamera one-shot guard).

### 2.2 Sim path: where `m_ActiveCamera.m_View` is produced (once per frame)

In `GameCameraManager::UpdateRender`, runs in the sim path (under `CGameStateRun::UpdateRender`, before `Game::Draw`):

```
CCameraTree::UpdateRenderContexts(...)         // populate m_ControlContext contexts
CSpecialCameras::UpdateRender(...)
GameCameraManager::PushRenderContext(this)     // -> InitTransform: m_ActiveCamera.T0 = T1 = ctx transform
CameraManager::UpdateRender(dt, dtf)
```

`PushRenderContext` eventually calls [`CameraManager::InitTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera_manager::CameraManager##func-InitTransform):

```c
v2 = m_ActiveCamera;
memcpy(&v2->m_TransformT0, mat, 0x40);
memcpy(&v2->m_TransformT1, mat, 0x40);     // T0 = T1 = context transform
```

So the gameplay camera's *world transform* lands in `m_ActiveCamera.m_TransformT0 == m_TransformT1`.

Then `CameraManager::UpdateRender` iterates every camera in its list and calls [`Camera::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-UpdateRender). That function is where `m_View` is computed:

```c
m_PreviousView = m_View;  m_PreviousProj = m_Projection;  m_PreviousViewProjection = m_ViewProjection;
m_PreviousTransformF = m_TransformF;
Lerp(&m_TransformF, &m_TransformT0, &m_TransformT1, dtf);     // T0==T1 => constant
if ( m_StateBitfield & m_ComputeView )                        // SetComputeView(true)
    m_View = m_TransformF;
    CMatrix4f::Inverse(&m_View);                              // m_View = inverse(world transform)
...
Multiply4x4(&m_View, &m_Projection, &m_ViewProjection);
m_ProjectionF = m_Projection;  m_ViewProjectionF = m_ViewProjection;  ...
```

[`Camera::SetComputeView(true)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-SetComputeView) sets `CameraState::m_ComputeView`. The active gameplay camera has this set, so `m_View` is re-derived as `Inverse(m_TransformF)` every sim frame, from the T0/T1-Lerped transform. Because `InitTransform` set T0 == T1, the Lerp is a no-op constant and engine pose interpolation is effectively disabled for the active camera (independent of `m_InterpolationOverride`).

`m_View` is computed exactly once per real frame, in the sim path, here. It is NOT recomputed at Draw time.

### 2.3 Draw path: active -> render camera copy

At Draw time the prologue calls `GraphicsEngine::TextureCachePlatformUpdate`. Near its top:

```c
memcpy(&this->m_RenderCamera, CameraManager.m_ActiveCamera, sizeof(Camera));  // render-copy = *m_ActiveCamera (whole Camera)
Camera::SetupRenderCamera(&this->m_RenderCamera, 1);                           // derive VP, reverse-Z, jitter
CameraManager.m_RenderCamera = &this->m_RenderCamera;
```

So the render camera lives inside the `GraphicsEngine` object as [`GraphicsEngine::m_RenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_RenderCamera) and is a byte-for-byte copy of `m_ActiveCamera` (including its `m_View`), made once per frame.

[`Camera::SetupRenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-SetupRenderCamera) is guarded by `CameraState::m_IsRenderCamera`. The body (verified):

```c
result = m_StateBitfield;
if ( (result & m_IsRenderCamera) == 0 ) {     // check-AND-SET: skip entirely if already set
    m_StateBitfield |= m_IsRenderCamera;
    // reverse-Z fixup on m_Projection (z' = w - z): e[2]=e[3]-e[2]; e[6]=e[7]-e[6]; e[10]=e[11]-e[10]; e[14]=e[15]-e[14]
    // same reverse-Z on the secondary/previous projection:
    if ( a2 ) {                               // a2 = "apply jitter" (always 1 from the prologue call)
        ApplyJitterTransform(engine, m_Projection, ...);
        ApplyJitterTransform(engine, m_PreviousProj, ...);
    }
    Multiply4x4(m_PreviousView, m_PreviousProj, m_PreviousViewProjection);
    Multiply4x4(m_View,         m_Projection,   m_ViewProjection);
    m_ProjectionF     = m_Projection;
    m_ViewProjectionF = m_ViewProjection;
    // + the secondary projection / VP float-shadow copies
}
return result;                                 // if m_IsRenderCamera already set: NO-OP
```

The guard is check-and-set: the body runs only when `m_IsRenderCamera` is clear, and sets it as its first action. The active/source camera (`m_ActiveCamera`) never has `m_IsRenderCamera` set — `Camera::UpdateRender` (the sim-path per-frame update) asserts `m_IsRenderCamera == false`, so the active cam carries it clear. The Draw-prologue memcpy copies it verbatim, so the render copy (`GraphicsEngine::m_RenderCamera`) always lands with `m_IsRenderCamera` clear; `SetupRenderCamera` then runs its full body (reverse-Z + jitter + VP rebuild) and sets `m_IsRenderCamera` on the copy. `m_ViewProjection` / `m_ViewProjectionF` are therefore rebuilt from `m_View x m_Projection` on every Draw. A second call on the same object (a double dispatch reusing the copy) is a no-op, which constrains hook ordering (§2.5).

`SetupRenderCamera` reads `m_View` as an input and never writes it. It does not derive `m_View` from any transform. So whatever `m_View` was in the copied active camera is what the render camera uses. Corollary for VR: a hook that writes the render-cam `m_View` *after* `SetupRenderCamera` has already run (`m_IsRenderCamera` set) will leave a stale `m_ViewProjection`/`m_ViewProjectionF`, because the re-call is a no-op and nothing else rebuilds VP on the render cam. You must then rebuild VP yourself (§2.6).

`ApplyJitterTransform` -> [`AntiAliasingEffect::ApplySubsampleJitter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AntiAliasingEffect##func-ApplySubsampleJitter) only fires when the AA mode is [`AAMode::AA_SMAA_T2X`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AAMode##variant-AA_SMAA_T2X); it post-multiplies a sub-pixel clip-space translation onto the projection (`proj = proj · jitterMat`).

### 2.4 What the scene render actually reads (camera-relative)

In `HandleDrawThreadTask`:

```c
SetRenderContextCamera(&engine->m_RenderContext, CameraManager.m_RenderCamera)   // = &engine->m_RenderCamera
SetGlobalShaderConstants(&engine->m_RenderContext)
```

The main scene is rendered **camera-relative** (for large-world float precision). The opaque-geometry transform is

```
clip = (objectWorld - CameraPosition) x OffsetViewProjection
```

where `OffsetViewProjection` is the view-projection with the view's **translation row zeroed**, and `CameraPosition` is the camera's **world position** supplied separately. The camera translation lives in the per-object subtraction, not in the matrix.

[`RenderPass::SetRenderContextCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SetRenderContextCamera) (non-shadow path) reads the live render camera (`GraphicsEngine::m_RenderCamera`) and fills the render context (`GraphicsEngine::m_RenderContext`):

- ctx ViewMatrix <- camera `m_View`
- ctx ProjectionMatrix <- camera `m_ProjectionF`
- ctx **CameraPosition** <- the translation row of `m_TransformF` (the camera world position)
- ctx <- the full `m_TransformF` world-transform rows
- `CalculateOffsetViewProjectionMatrix` is called twice, current + previous: it copies `m_View`, **zeros its translation row** (`row3 = {0,0,0,1}`), multiplies by the projection, and writes the translation-free **OffsetViewProjection** into the context. This is the VP opaque geometry actually uses.

`CRenderEngine::SetGlobalShaderConstants` uploads the **global per-view CB**: it calls `CameraManager::GetRenderCamera` then `memcpy(&this->m_VPGlobalConstData[0], RenderCamera.m_ViewProjectionF, 0x40)` ([`RenderEngine::m_VPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_VPGlobalConstData)) — the *full*, translation-bearing `m_ViewProjectionF`; the camera-position constants come from `m_TransformF`'s translation. This per-view block drives **screen-space / non-geometry** work (post-effects, billboards, the camera-position constant); it is **not** what positions opaque geometry vertices.

**Consequence for VR (the key correction):** on the geometry path, `m_View`'s translation is deleted by the OffsetVP zeroing. A per-eye lateral offset must move the **camera world position** — `m_TransformF`'s translation, hence `CameraPosition` — not `m_View`'s translation. (Confirmed at runtime: writing only `m_View`'s translation gave two divergent `m_ViewProjectionF`s on the render camera but byte-identical geometry between eyes; only the muzzle flash, which rides the per-view CB, shifted.) See §2.5.

### 2.5 Per-eye view injection

Because the geometry is camera-relative (§2.4), the per-eye offset must move the camera **world position** (`m_TransformF`'s translation), not `m_View`'s translation (which the OffsetVP zeros). It targets the **render camera** (`GraphicsEngine::m_RenderCamera`), not `m_ActiveCamera` — the sim recomputes the active camera's `m_View = Inverse(m_TransformF)` (`m_ComputeView`) and the prologue memcpy copies it verbatim, so any per-eye divergence has to be applied to the render-camera copy between dispatches.

**Recipe.** Hook `Camera::SetupRenderCamera` and act only on the main render camera (`this == &engine->m_RenderCamera`), or equivalently hook right after the active->render memcpy in `GraphicsEngine::TextureCachePlatformUpdate`. Per eye:

1. Offset `m_TransformF`'s translation by +/- IPD/2 along the camera's right axis (the first basis row of `m_TransformF`). This moves `CameraPosition`, so the camera-relative geometry diverges.
2. Re-derive `m_View = Inverse(m_TransformF)` so the view stays consistent with the moved camera (the OffsetVP's rotation comes from `m_View`; the per-view CB's `m_ViewProjectionF` uses the full `m_View`).
3. Rebuild `m_ViewProjection` / `m_ViewProjectionF` from `m_View x m_Projection` (§2.6) so the per-view CB matches.

Pitfalls:

- Offsetting only `m_View`'s translation does nothing to geometry — the OffsetVP zeros it (§2.4). It shifts only the per-view CB (post / camera-position constant), which is the muzzle-flash-moves-but-world-doesn't symptom.
- Offsetting only `m_TransformF` without re-deriving `m_View` desyncs `CameraPosition` from the OffsetVP rotation and the per-view CB.

### 2.6 The VP-rebuild recipe

The engine's `Matrix4` (`CMatrix4f`) is D3D-style — row-major, row-vector (`clip = p · M`) — and `Matrix4::Multiply4x4(a, b, dest)` writes `dest = a · b`; the full convention (basis-in-rows, the glam bridge) is documented on the `Matrix4` def. `SetupRenderCamera` calls `Multiply4x4(m_View, m_Projection, m_ViewProjection)` ⇒ `m_ViewProjection = m_View · m_Projection`, `clip = p · View · Projection`, and `Camera::UpdateRender` uses the identical order. So to rebuild a render camera after writing a custom `m_View` and/or `m_Projection`:

```c
Camera *cam = &engine->m_RenderCamera;                       // == CameraManager.m_RenderCamera
// (write cam->m_View and/or cam->m_Projection first)
Multiply4x4(&cam->m_View, &cam->m_Projection, &cam->m_ViewProjection);
memcpy(&cam->m_ProjectionF,     &cam->m_Projection,     0x40);
memcpy(&cam->m_ViewProjectionF, &cam->m_ViewProjection, 0x40);
```

The `*F` float-shadow copies (`m_ProjectionF`, `m_ViewProjectionF`) are what the render context and global CB actually read (§2.4) — you MUST update them too, not just `m_ViewProjection`.

### 2.7 Reverse-Z / depth convention + the per-eye projection wedge bug

The base projection built by `Camera::RecalcProjection` (via `PerspectiveFov` / `PerspectiveOffCenter` / `Ortho`) is a standard (non-reversed) projection. The reverse-Z remap `z' = w - z` is applied exactly once per render camera, gated by `m_IsRenderCamera`:

- [`RecalcProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-RecalcProjection) applies it only when `m_IsRenderCamera` is set — i.e. only when called on a render cam.
- `SetupRenderCamera` applies it **unconditionally to whatever is in `m_Projection`** when its `m_IsRenderCamera`-guarded body runs (§2.3). There is no "already reversed?" check.

Result: reverse-Z is applied once, by whichever of {`RecalcProjection`-on-render-cam, `SetupRenderCamera`} first sees `m_IsRenderCamera` clear. Depth convention: reverse-Z, NDC z in [0,1] with near=1, far=0 (MainDepth is `D32FS8`), enabling far-plane precision / infinite-far style projections.

**The wedge bug (VR):** if you write a per-eye off-axis projection into the render cam's `m_Projection` **before** `SetupRenderCamera` runs, `SetupRenderCamera` will reverse-Z it. So:

- **Preferred:** supply a standard (non-reversed) off-axis projection into `m_Projection` *before* `SetupRenderCamera`, and let the engine apply reverse-Z + jitter once. Matches engine convention; you also get TAA jitter for free.
- **Alternative:** supply an already-reverse-Z'd projection *after* `SetupRenderCamera` (when `m_IsRenderCamera` is set, so it won't re-reverse), then rebuild VP/`*F` yourself (§2.6). With this path you must apply jitter yourself if `AA_SMAA_T2X` is active, and maintain `m_PreviousProj`/`m_PreviousViewProjection` for velocity. Do not feed an already-reversed projection into the pre-`SetupRenderCamera` window — that double-applies the remap (the wedge artifact).

`AntiAliasingEffect::ApplySubsampleJitter` only fires when the AA mode is `AA_SMAA_T2X`; it post-multiplies a sub-pixel clip-space translation (`m[12]=jx`, `m[13]=jy`, `j = ±offset/width,height`) onto the projection: `proj = proj · jitterMat`.

### 2.8 Vehicle camera path

Vehicles take a different path through `GameCameraManager::PushRenderContext`: an `IsInDrivingVehicleState` branch routes the transform differently — a raw matrix, bypassing the jitter freeze — so vehicle head-look behaves differently from on-foot. See `docs/mod/body/head-and-body.md`'s RE notes for the mod-side validation this motivates (logging the cockpit head-bone position against character world position, confirming it isn't clipped to seat or world origin).

### 2.9 Projection construction — exact formulas, real near/far, and consume-vs-rebuild (decompiled)

Decompiled to settle the depth convention for the VR off-axis projection (the claims in §2.7 named the flow; this is the line-level confirmation, plus the numbers).

**Where `m_Projection` is built.** `Camera::RecalcProjection` builds it once, dispatching on `m_StateBitfield`: [`m_Ortho`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState##flag-m_Ortho) → `CMatrix4f::Ortho`, [`m_UseOffCenter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState##flag-m_UseOffCenter) (off-center tiling) → `CMatrix4f::PerspectiveOffCenter`, else → `CMatrix4f::PerspectiveFov`. All three read the camera's `m_Near` / `m_Far` scalars; the FOV comes from `m_FOV`, the aspect from `m_AspectRatio`. There is no infinite-far formulation — the far plane is a finite field value.

**`CMatrix4f::PerspectiveOffCenter(this, x_min, x_max, y_min, y_max, z_min=near, z_max=far)`** writes the row-major `e[]` (D3D row-vector, `clip = p·M`) as:

```
e[0]  = 2·near / (x_max − x_min)          e[5]  = 2·near / (y_max − y_min)
e[8]  = (x_min + x_max) / (x_max − x_min)  e[9]  = (y_min + y_max) / (y_max − y_min)
e[10] = far / (near − far)                 e[11] = −1
e[14] = near·far / (near − far)            e[15] = 0   (all others 0)
```

`RecalcProjection` passes **near-plane extents** (`x = near·tan θ`, computed via the engine's polynomial sin/cos approximations of `m_FOV`), so `near` cancels from `e[0]`/`e[5]`/`e[8]`/`e[9]` and the matrix depends only on the FOV tangents and the depth column. This is exactly `DirectXMath`'s `PerspectiveOffCenterRH` (standard depth, NDC z in `[0, 1]`, near → 0, far → 1). `PerspectiveFov` is the symmetric special case.

**Reverse-Z is `z' = w − z`, applied in place.** In the row-major layout that is `e[row·4+2] = e[row·4+3] − e[row·4+2]` for all four rows (`e[2]=e[3]−e[2]; e[6]=e[7]−e[6]; e[10]=e[11]−e[10]; e[14]=e[15]−e[14]`). It maps near → 1, far → 0 (MainDepth is `D32FS8`). Two sites apply it, each gated so it happens **exactly once**:

- `RecalcProjection` applies it at its tail **only if** `m_IsRenderCamera` is already set.
- `Camera::SetupRenderCamera` applies it under its check-and-set `m_IsRenderCamera` guard.

**Consume vs rebuild — settled: `SetupRenderCamera` CONSUMES.** Decompiled, its guarded body does, in order: (1) the four-element reverse-Z remap **on whatever is already in `m_Projection`**, then the same on `m_PreviousProj`; (2) `ApplyJitterTransform` on both (when `a2`); (3) `Multiply4x4` to rebuild `m_ViewProjection` / `m_PreviousViewProjection`; (4) `memcpy` the `*F` float-shadow copies. It **never calls `PerspectiveFov`/`PerspectiveOffCenter` and never reads `m_FOV`/`m_Near`/`m_Far`** — it transforms the existing matrix in place. So a `m_Projection` written *before* `SetupRenderCamera` flows through and receives reverse-Z (and jitter) exactly once; the engine does **not** build the reversed form from scratch, it remaps whatever standard matrix it finds. This is the verified basis for the mod's preferred `EnginePreReverseZ` path.

**Real near/far.** The `Camera` constructor writes `m_Near` = [`Camera::DEFAULT_NEAR_PLANE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera::DEFAULT_NEAR_PLANE) (0.1) and `m_Far` = [`Camera::DEFAULT_FAR_PLANE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera::DEFAULT_FAR_PLANE) (38400.0). No gameplay-wide override was found — only auxiliary cameras diverge (`CWaterPatchManager` reflection/top-down cameras, shadow/RSM light cameras at near `0.1`; `CCollisionModule` nudges *near* when the camera clips geometry). So the on-foot/vehicle scene camera renders a **finite-far reverse-Z frustum, near `0.1`, far `38400`** (~38 km — matching the open world's draw distance; reverse-Z is what keeps `D32` precision usable at that range). The VR off-axis builder adopts these as its default near/far so its frustum matches the engine and the horizon does not clip.

---

## 3. Render passes / stages (ordered)

The pass system is a flat enum `ERenderPass` of ~180 `RP_*` values (ordered, contiguous; names recovered from the pass-name switch). Passes are drawn by index range via [`RenderEngine::DrawRenderPassRange(ctx, renderSetup, first, last)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawRenderPassRange), which walks [`RenderEngine::m_RenderPasses`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_RenderPasses) (a fixed array of per-pass `CRenderPass*` lists, created at init) and vtable-dispatches each pass's render blocks. The per-frame render-block instances live in each `CRenderPass`'s double-buffered `m_Lists[2]` (two `CRBILists`); see §11.

Ordered stages per dispatch:

1. **Shadow atlas** — committed in `CShadowManager::CommitRenderPassSettings` and rendered as the `RP_SHADOW_0..7` / `RP_STATIC_SHADOW_0..7` / `RP_SHADOW_REFLECTIVE_*` passes. Shadow data is **parity-buffered** by `render_frame_counters.m_FrameIndex & 1`: `CRenderPass::SetRenderContextCamera` and the shadow-matrix reads index shadow storage by `(m_FrameIndex & 1) << 8` (a 256-byte stride per parity). So shadows ping-pong between two parity buffers each real frame. Running Draw twice in one real frame uses the same parity twice (parity only advances in the `GraphicsEngine::Draw` prologue), which is benign for the second eye but means the second eye sees the same parity slot.

2. **GBuffer fill** — [`RenderEngine::DrawGBuffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawGBuffer) -> `DrawRenderPassRange(setup, RP_Z_OCCLUDERS, RP_LAST_GBUFFER)`. The depth/velocity prefix is `RP_Z_OCCLUDERS`, `RP_Z_COARSE_PASS`, `RP_Z_PASS`, `RP_Z_AND_VELOCITY_PASS` (this is where the velocity buffer is written using the previous-frame VP), then static/dynamic models, then `RP_DECALS` / `RP_SCREEN_SPACE_DECALS` / `RP_SCREEN_SPACE_ROAD_DECALS`, terminating at `RP_LAST_GBUFFER`. Before drawing it binds two global textures on fragment texture slots 40 and 41.

3. **Lighting / reflections / main** — [`RenderEngine::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-Draw) -> `DrawRenderPassRange(setup, RP_REFLECTIVE_WATER_PLANES, RP_POSTEFFECTS)`. In order: `RP_REFLECTIVE_WATER_PLANES`, `RP_AO_VOLUMES`, `RP_SSAO`, `RP_SCREEN_SPACE_REFLECTIONS`, `RP_GLOBAL_ILLUMINATION`, `RP_SCREEN_SPACE_SUBSURFACE_SKIN`, `RP_DEFERRED_LIGHTS` (these resolve lighting into MainColor), then opaque/`RP_LAST_OPAQUE`, environment (`RP_STARS`/`RP_SUN`/`RP_MOON`/`RP_SKYBOX`), water, transparency, ending just before `RP_POSTEFFECTS`. Reflection-proxy passes (`RP_REFLECTION_*`) and `RP_ENVREFLECTION` are also in this block.

4. **Post-effects (world)** — `CPostEffectsManager::ApplyWorldFilters` enqueues the world post-effect block, then [`RenderEngine::DrawPosteffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawPosteffects) -> `DrawRenderPassRange` over the one-pass range starting at `RP_POSTEFFECTS` runs that pass. The actual HDR chain executes inside `CRenderBlockPostEffects::Draw`, in order:
   1. `CToneMappingEffect::Apply` (publishes the applied exposure into the frame context; exposure-weighted histogram)
   2. `CSunHaloEffect::PreApply`
   3. blur: bokeh path (`CDownScale2x2PackFocus::Apply` -> `CBlurEffectBokeh::Apply`) if `CPostEffectsManager::IsBokehActive`, else `CBlurEffect::Apply`
   4. `CGlareEffect::Apply`
   5. `CDepthOfFieldEffect::Apply`
   6. `CMotionBlurEffect::Apply` (gated by motion-blur active / AA-mode==3 / heat-haze)
   7. `CToneMappingEffect::GenerateHistogramForFinalScene` (the raw final-scene histogram; **the HDR->LDR tonemap composite itself is `CDepthOfFieldEffect::Apply`**, step 4's DoF composite, which multiplies the scene by the published exposure as fragment constant c2.x)
   8. `CPlayerDamageEffect::Apply` (if damage flag set)
   9. `CAntiAliasingEffect::Apply`
   10. `CSunHaloEffect::Apply` + additive sun blend (`SetBlendFunc`)
   11. `CFadeEffect::Apply` (final fade)

5. **Global filters** — `CPostEffectsManager::ApplyGlobalFilters` enqueues `RP_POSTEFFECTS_GLOBAL` work (screen fade alpha, heat-haze, sun-direction accumulation — see §5).

6. **PostDraw / UI / debug / final copy** — `CRenderEngine::PostDraw`, UI render block, `QuickDrawController::Draw`, then `CopySurfaceToTexture` (the present-target blit).

### 3.1 The HDR->LDR composite

The HDR→LDR conversion — applying the published exposure and the tone-map operator to turn the R11G11B10F HDR MainColor into the LDR target — happens in `CDepthOfFieldEffect::Apply` (the DoF composite step of the same block; exposure rides in as fragment constant c2.x from `PostEffectContext::m_Exposure`). Step 1 publishes that exposure and meters the exposure-weighted histogram; step 7 meters the raw final scene.

### 3.2 Ping-pong slot scheme (why per-eye intermediate captures alias)

`CRenderBlockPostEffects::Draw` threads a single integer "current result-texture slot" through the effects: it reads the current slot index, and each of DoF/MotionBlur/PlayerDamage/AntiAliasing returns the new slot index which is written back, so the chain hops between the three temp textures. The temp arrays are `m_FullscreenSrgbTempTexture[3]` / `m_FullscreenLinearTempTexture[3]` with `m_RenderSetups[3]` (count [`s_FullscreenTempTextureCount`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager::FULLSCREEN_TEMP_TEXTURE_COUNT)). The rotation idiom is: render into `m_RenderSetups[(current+1)%3]`, sample from temp `[current]`, then `current = (current+1)%3`. The [`PostEffectsManager::RESULT_TEXTURE_SLOT_BASE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager::RESULT_TEXTURE_SLOT_BASE) convention is the result-texture-index offset each effect exposes via `GetResultTexture`; effects publish their output texture pointer into the next consumer's input slot.

Because the slot index advances per *dispatch* (and the `%3` ring `render_frame_counters.m_RingIndex` advances per *real frame*), two dispatches in one real frame share the same `%3` ring value but advance the intra-chain slot index twice — so a per-eye capture taken "from slot N" can alias the other eye's intermediate. For VR captures, snapshot from the final composite/back-buffer copy (`CopySurfaceToTexture` output) rather than from an intermediate temp slot.

---

## 4. Buffers & how the frame comes together

Render targets (created in `CreateRenderSetups`):

| Buffer | Name string | Format | Role |
|---|---|---|---|
| MainDepth | `"MainDepthBuffer"` | `SURFACEFORMAT_D32FS8` | scene depth+stencil (reverse-Z) |
| MainColor | `"MainColorBuffer"` | `SURFACEFORMAT_R11G11B10F` | **HDR** scene color |
| GBuffer0 | `"GBuffer0"` | `SURFACEFORMAT_ABGR32` | albedo/material; sRGB+linear aliases |
| GBuffer1 | `"GBuffer1"` | `SURFACEFORMAT_A2R10G10B10` **[UNVERIFIED]** | normals |
| GBuffer2 | `"GBuffer2"` | `SURFACEFORMAT_ABGR32` | material; sRGB alias |
| GBuffer3 | `"GBuffer3"` | `SURFACEFORMAT_ABGR32` | misc/material |
| DownsampledDepth | `"DownsampledDepth"` | half-res depth (D32FS8 + D32F alias) | SSAO/SSR/soft particles |
| Velocity | `"motion_blur_velocity_buffer"` | `SURFACEFORMAT_ABGR32` | screen-space velocity (written in `RP_Z_AND_VELOCITY_PASS`) |
| BackBufferLinear | `"BackBufferLinear"` | ABGR32 (alias of BACK_BUFFER) | final LDR present target (`m_BackBufferLinear`; used as `m_BackBufferRenderSetup` color) |

These RT pointers live on the `GraphicsEngine` singleton (`m_GBufferTexture[4]`, `m_VelocityBufferTexture`, `m_MainDepthTexture`, `m_MainColorBuffer`, `m_DownSampledDepthTexture`, `m_BackBufferLinear`). Reflection-proxy RTs created alongside: `reflection_proxy_water_plane_texture`, `reflection_proxy_depth_texture`, `reflection_proxy_normal_gloss_texture`, `ao_volume_texture`, and 5x `VfxDepthCopy_%d` (the EffectInfo depth-copy slots, §5.1). **[Formats UNVERIFIED.]**

Frame assembly path:
```
GBuffer fill (MainDepth + GBuffer0..3 + Velocity)
  -> lighting/SSR/GI/deferred lights resolve into MainColor (HDR, R11G11B10F)
  -> environment/water/transparency composited into MainColor
  -> post chain (blur/glare/DoF-composite tonemap HDR->LDR/motionblur),
     AA, damage, sun halo, fade  -> intermediate fullscreen temp textures (3-slot ring)
  -> global filters (screen fade, heat haze) -> RP_POSTEFFECTS_GLOBAL
  -> PostDraw + UI + debug
  -> EndDraw (command submission; there is no final copy -- the composite already
     wrote into BackBufferLinear, which aliases swapchain buffer 0)
  -> graphics_flip (present; suppressed by BLOCK_FLIP in VR)
```

### 4.1 The depth-reconstruction passes (a closed set)

Some passes recover world or view position from the depth buffer rather than from interpolated vertex
data. They all do it the same way: rebuild a clip-to-view inverse with
[`CMatrix4f::PerspectiveFovInverse`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/types::math::Matrix4##func-PerspectiveFovInverse), then multiply by the render context's camera
transform. Cross-referencing that one function enumerates every such pass in the engine — the set is
closed, and there are seven call sites:

| Pass | Notes |
|---|---|
| [`RenderBlockDeferredLighting::DrawClustered`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockDeferredLighting##func-DrawClustered) | the shaded lighting path |
| [`RenderBlockDeferredLighting::DrawPassThrough`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockDeferredLighting##func-DrawPassThrough) | **wireframe only** — [`Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockDeferredLighting##func-Draw) branches on `IsWireframeEnabled` |
| [`RenderBlockAtmosphericScattering::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockAtmosphericScattering##func-Draw) | reconstructs the whole screen, sky included; ray-marches the sun cascade and aerial perspective |
| [`RenderBlockSSAO::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockSSAO##func-Draw) | carries a temporal history advanced per invocation |
| [`RenderBlockScreenSpaceReflection::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockScreenSpaceReflection##func-Draw) | ray-marches an earlier scene-colour capture |
| [`RenderBlockScreenSpaceSubSurfaceSkin::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockScreenSpaceSubSurfaceSkin##func-Draw) | calls it twice, once per blur axis |
| [`DOFUtil::GetViewProjInverse`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::post_effects##func-GetViewProjInverse) | post-effect utility, not a render block |

The rebuild encodes only a *symmetric* frustum (see the `PerspectiveFovInverse` def for the exact
matrix), so any consumer that needs an asymmetric one has to substitute the matrix rather than adjust
its inputs. Two of the seven sample the sun shadow cascade over the reconstructed positions —
`DrawClustered` and `AtmosphericScattering::Draw` — so shadow behaviour that depends on the
reconstruction basis is governed by those two together, and fixing one alone leaves the other
painting over it.

### 4.2 Draw entry points, and the D3D11 slots they reach

Every render block submits its geometry through one of the `Graphics::` draw wrappers (the `graphics_engine::draw` module of the defs), and each wrapper is a thin cover over a single `ID3D11DeviceContext` vtable slot. IDA's names for these wrappers are shuffled, and IDA misreports their argument counts and types as well, so the IDB names cannot be trusted on their own. The names bound in the defs are matched by body: the D3D11 method each wrapper calls, whether it loops, and whether it takes the context mutex. The `NoMutex` variants skip the mutex because their callers already hold the context. The bound names are the ones used here:

| Wrapper | `ID3D11DeviceContext` method | Vtable slot |
|---|---|---|
| [`Graphics::DrawIndexed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexed) | `DrawIndexed` | 12 |
| [`Graphics::DrawIndexedWithBoundState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedWithBoundState) | `DrawIndexed` | 12 |
| [`Graphics::DrawIndexedNoMutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedNoMutex) | `DrawIndexed` | 12 |
| [`Graphics::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-Draw) | `Draw` | 13 |
| [`Graphics::DrawIndexedInstanced`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedInstanced) | `DrawIndexedInstanced` | 20 |
| [`Graphics::DrawIndexedInstancedNoMutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedInstancedNoMutex) | `DrawIndexedInstanced` | 20 |
| [`Graphics::DrawInstanced`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawInstanced) | `DrawInstanced` | 21 |
| [`Graphics::DrawIndexedInstancedIndirect`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedInstancedIndirect) | `DrawIndexedInstancedIndirect` | 39 |
| [`Graphics::DrawIndexedInstancedIndirectNoMutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedInstancedIndirectNoMutex) | `DrawIndexedInstancedIndirect` | 39 |
| [`Graphics::DrawIndexedInstancedMultiIndirect`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedInstancedMultiIndirect) | `DrawIndexedInstancedIndirect`, once per draw in a loop | 39 |
| [`Graphics::DrawInstancedIndirect`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawInstancedIndirect) | `DrawInstancedIndirect` | 40 |

The two indirect slots are how the GPU-driven geometry submits: [`RenderBlockTerrainPatch::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainPatch##func-Draw)'s near tessellating passes (`RP_TERRAINPATCH_BASEMESH_TESSELLATE_NEAR` and `RP_TERRAINPATCH_BASEMESH_NEAR`) go through `DrawInstancedIndirect` (slot 40), and [`RenderBlockFoliage::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockFoliage##func-Draw)'s dominant grass path through `DrawIndexedInstancedIndirectNoMutex` (slot 39). Both blocks also have plain `DrawIndexed` paths for their other passes, so a block is not characterized by one entry point (see `docs/mod/stereo/single-pass-render-blocks.md` for the per-block routing).

`Graphics::Draw` (slot 13) is not exclusively the fullscreen-pass entry point, though that is its dominant use. Several render blocks submit ordinary world geometry non-indexed through it — evidenced so far in `RP_ROAD_LAYERS` (`CRenderBlockSplineRoad`), `RP_DECALS` (the four `CRenderBlockDecal*` blocks), `RP_SKIDMARKS`, and `RP_WINDOW_DECALS`. A fullscreen triangle and a decal box are indistinguishable at the draw call; only the pass tells them apart.

### 4.3 How shaders address the screen

Three idioms are in use, and they agree with one another in the stock game only because it renders one view into a target the size of the display.

1. **Absolute texel fetch** — `ftoi SV_Position.xy` followed by `ld`. The deferred and forward G-buffer readers use this, as does the clustered light-grid lookup (`tile = ftoi(SV_Position.xy) >> 6`). `SV_Position` in a pixel shader is the absolute render-target coordinate: `RSSetViewports`' `TopLeftX`/`TopLeftY` are already included. This addresses the **target**.
2. **Normalized by the inverse display size** — `mul rN.xy, v0.xyxx, cb0[8].zzwz`, where `cb0[8].zw = (1/DisplayWidth, 1/DisplayHeight)` is staged into [`RenderEngine::m_FPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_FPGlobalConstData) by `SetGlobalShaderConstants` from `Graphics::SDeviceInfo` and **not** from the bound render setup or the viewport (see `lighting-shadow-pipeline.md` §5.3). `deferred_clusteredlighting_full`'s reflection and SSR lookups build their UV this way, and so does the WaveWorks `nvwater*` family. This also addresses the **target**.
3. **A CPU-staged projective UV** — the legacy, non-WaveWorks `Water*` / `WaterBox*` family (selected at the lower water-quality settings) receives a world-to-screen-UV matrix on vertex `cb1` from its block type's `Setup`: the render context's view-projection post-multiplied by an NDC-to-UV bias, so the `x·0.5 + w·0.5` is folded in on the CPU. The vertex shader passes the result on as a projective `TEXCOORD1` and the pixel shader divides by `w`. That UV is normalized over the **viewport**, not over the target. [`WaterHighEndRenderBlockType::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::WaterHighEndRenderBlockType##func-Setup) stages it at `cb1[1..4]` and [`WaterBoxRenderBlockType::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::WaterBoxRenderBlockType##func-Setup) at `cb1[4..7]`. Exactly 9 of the 961 pixel shaders in `Shaders_F` bind `RefractionMap`, and they are this family.

The distinction is invisible while the viewport covers the whole target and load-bearing as soon as it does not.

---

## 5. Per-frame-once / single-use state (double-dispatch hazards)

For the double-dispatch VR hack, these advance/consume exactly once per dispatch and so double-step if `HandleDrawThreadTask` (or `Game::Draw`) runs twice per presented frame.

### 5.1 EffectInfo reflection-proxy history (slot machine)

[`GraphicsEngine::m_EffectInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_EffectInfo) = 5 [`EffectInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::EffectInfo) slots on the `GraphicsEngine`; [`m_EffectInfoIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_EffectInfoIndex) is the current-index int. The slot machine runs in `HandleDrawThreadTask` between `DrawGBuffer` and `RenderEngine::Draw`. Each slot stores an `m_FrameIndex` state byte, a depth texture (`m_DepthTexture`, a `VfxDepthCopy_%d`), and an `m_Transform` VP matrix. Per dispatch:

- Loop the 5 slots: state `0` -> remember as free slot; state `2` -> remember; state `3` -> write its index into `m_EffectInfoIndex` (current read slot); state `1` -> increment to `2`.
- If a free slot found: `CopySurfaceToTexture(slot.m_DepthTexture, m_ReflectionProxyDepthSurface)` (capture this frame's proxy depth) and set its state to `1`.
- If a state-2 slot found: promote to `3`.
- `memcpy(slot[current].m_Transform, RenderCamera.m_ViewProjectionF, 0x40)` then set `slot[current].m_FrameIndex = 0`.

This is an N-frame depth+VP history ring for reflection-proxy / reprojection. **Double dispatch:** every slot's state advances twice, so captures age twice as fast; the state-0 slot written by eye 0 is re-aged past its 0->1 dwell by eye 1; and the VP `memcpy` overwrites the same current slot with the same-frame VP. Net: the stored VP no longer matches the depth in the slot -> reflection-proxy ghosting. **Mitigation:** snapshot the 5 `m_FrameIndex` state bytes plus `m_EffectInfoIndex` before eye 0 and restore before eye 1.

### 5.2 dt-driven accumulators in `ApplyWorldFilters` / `ApplyGlobalFilters`

- [`PostEffectsManager::ApplyWorldFilters(dt)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager##func-ApplyWorldFilters): `dt` flows only into [`PostEffectsManager::ApplyWorldFadeFilter(dt)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager##func-ApplyWorldFadeFilter) — the world fade accumulator. The rest is just texture/setup wiring.
- [`PostEffectsManager::ApplyGlobalFilters(dt)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager##func-ApplyGlobalFilters): advances the screen fade alpha (`+= / -= (1/fade_duration)*dt`, clamped [0,1]) per dispatch; advances a sun-direction / heat-haze accumulator; calls the DoF update hook. The fade alpha, its duration, and the accumulator are fields of `PostEffectsManager` that are not yet bound in the defs. These step twice on double dispatch.

**Mitigation:** the mod sets the render context `m_Dt = 0` for the second eye, so dt-driven accumulators do not advance twice. (Auto-exposure is a *separate*, frame-counted hazard that `m_Dt = 0` does **not** touch — see §5.4.)

### 5.3 Shadow parity (`render_frame_counters.m_FrameIndex & 1`)

Parity advances only in the `GraphicsEngine::Draw` prologue, not per dispatch. Two dispatches in one prologue share the same parity — eye 1 reads the same shadow parity slot eye 0 wrote. Benign but means shadows are not independently double-buffered per eye.

### 5.4 Exposure smoother / clock

- `Clock::Update` ticks once in the prologue (per real frame) — keep it there; the slow-mo hazard is calling it twice, so it is gated to once per real frame.
- Auto-exposure is **frame-counted, not dt-driven** — `m_Dt = 0` does **not** touch it. The stereo darkening (~0.74x) comes from a **second histogram meter run on both eyes**; gating it to one eye removes it.

  **How it works.** `CToneMappingEffect::Update` runs **once per real frame** (`CPostEffectsManager::UpdateRender`); it sets the auto-exposure target to `key / m_Histogram2.m_HistogramMidPoint` — key over the *raw* scene brightness — then adapts `m_ExposureBrightPoint` (the applied exposure) toward it. There are **two** histograms, both metered via `PopulateHistogram` → per-bucket occlusion queries over a fixed 320x180 RT (a correct read totals 57,600 pixels):
  - [`m_Histogram`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::tone_mapping::ToneMappingEffect##field-m_Histogram): the **exposure-weighted** meter, run by `CToneMappingEffect::Apply` per dispatch (fed `m_ExposureBrightPoint` through the `"LuminanceToDepthWithExposure"` shader; that exposure-weighting is the feedback loop). The mod gates it to eye 0.
  - [`m_Histogram2`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::tone_mapping::ToneMappingEffect##field-m_Histogram2): the **un-weighted** raw-brightness meter — the target's divisor — run by `GenerateHistogramForFinalScene`. That function just calls `PopulateHistogram` with a fixed exposure of `1.0`; it also runs per dispatch.

  **The hazard and the gate.** Gating only the exposure-weighted meter (`Apply`) to eye 0 leaves the raw meter (`GenerateHistogramForFinalScene`) running on *both* dispatches in stereo: `m_Histogram2`'s occlusion-query ring inflates and corrupts (total 57,600 → ~265,000, with irregular buckets), its mid-point reads ~1.35x high, and the exposure divides by a too-large brightness → the frame darkens ~0.74x. The gate is the symmetry: the raw meter is gated on eye 1 too (`hooks::graphics_engine::tone_mapping::generate_final_histogram`, under `config.exposure.gate`), so `m_Histogram2` meters once per real frame. The per-frame `ExposureInternals` trace confirms it — with the gate, stereo `divisor`/`exposure` match non-stereo and `hist2`'s total is 57,600. (Gating the exposure-weighted meter harder does nothing, precisely because it feeds `m_Histogram`, not the divisor; the functions are all in `graphics_engine/tone_mapping.pyxis`.)

### 5.5 Screenshot countdown / profiler / end-draw callbacks

- `GraphicsEngine::m_FramesUntilScreenshot`, the screenshot frames-until countdown, decrements per dispatch (two branches in `HandleDrawThreadTask`). Gate to eye 0.
- GPU profiler `EndFrame` fires per dispatch — extra profiler entries (harmless).
- End-draw `sig::Signal` one-shot callbacks self-remove after first fire; gate to the last (eye 1) dispatch so they fire once.

### 5.6 Draw-done completion event

At the tail of `HandleDrawThreadTask`: `engine->m_CPUDrawFinished = 1; SetEvent(engine->m_CPUFinishedDrawingEvent)` — the draw-done signal that `WaitForCPUDrawToFinish` fences on. It fires on **every** dispatch. If you run two dispatches per frame, eye 0 sets the event and the sim side's next `WaitForCPUDrawToFinish` could be released by eye 0's signal before eye 1 finishes. **Mitigation:** suppress the `SetEvent`/`m_CPUDrawFinished = 1` on eye 0; let only the final (eye 1) dispatch signal. Also `m_CPUDrawStallDuration`/`m_DrawTime` (CPU draw-time floats) are last-writer-wins — harmless.

### 5.7 Jitter phase / frame-counter ring (TAA)

The TAA jitter offset comes from a 2-phase table [`jittered_samples`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::jittered_samples) (4 floats) indexed by `render_frame_counters.m_FrameIndex & 1` (set in the Draw prologue), scaled by [`jitter_scale_factor`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::jitter_scale_factor) and divided by RT width/height. The phase + the `%3` CB ring (`m_RingIndex`) advance **per real frame in the prologue**, NOT per dispatch — so both eyes in one frame share the same jitter phase and CB indices **if you call only the dispatch body twice** (recommended). If you instead call the whole `GraphicsEngine::Draw` twice, the counter double-steps and the two eyes get opposite jitter phases + mismatched CB slots → TAA history mismatch / flicker. **Per-eye TAA:** the velocity buffer's `m_PreviousViewProjection` is snapshotted only in the sim-path `Camera::UpdateRender`, not per dispatch; for correct per-eye velocity, give each eye its own previous-frame VP (or disable `AA_SMAA_T2X` / motion blur during bring-up). For FSR, mod code handles this: the velocity-decode pass re-anchors each eye's vectors at its own previous pose using per-eye VP snapshots (`stereo::VpHistory`; the stereo MV correction in `docs/mod/rendering/fsr.md` — the fix for issue #10's per-eye shadow-edge flicker). A shared TAA history RT reprojected with two different cameras in one frame is the primary flicker source — use per-eye history or disable temporal AA initially. **[UNVERIFIED]** exact TAA history RT binding.

The mod currently takes the disable path: `config.stereo.force_smaa_1x` (default on) drops the resolve mode [`AntiAliasingEffect::m_Mode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AntiAliasingEffect##field-m_Mode) from `AA_SMAA_T2X` to `AA_SMAA` (SMAA 1x) on stereo, and skips the TAA jitter with it, removing the cross-eye temporal ghost. Restoring per-eye T2X would mean giving each eye its own history ping-pong pair on `AntiAliasingEffect` (a pair of history textures, a pair of render setups, and a pair of history indices, none of which is bound in the defs yet), allocated to match [`AntiAliasingEffect::CreateRenderTargetResources`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AntiAliasingEffect##func-CreateRenderTargetResources) — the full allocation recipe (the `SCreate2DTextureParams` values and the `CreateRenderSetup` call, mislabeled `LockVolumeTexture` in the IDB) is in the per-eye-T2X task. Low priority: T2X ghosts in VR head-motion regardless of per-eye history.

---

## 6. Quick injection cheat-sheet (VR)

- **Per-eye view divergence (camera-relative — see §2.4/§2.5):** the geometry is camera-relative, so offset the render camera's **`m_TransformF` translation** along the camera right axis — NOT `m_View`'s translation, which the OffsetVP zeros — then re-derive `m_View = Inverse(m_TransformF)` and rebuild `m_ViewProjection`/`m_ViewProjectionF` (§2.6). Hook `Camera::SetupRenderCamera` on the main render camera (`this == &engine->m_RenderCamera`), or right after the active->render memcpy in `GraphicsEngine::TextureCachePlatformUpdate`. For a per-eye off-axis *projection*, write a standard (non-reverse-Z) `m_Projection` before `SetupRenderCamera` so it reverse-Z's once (§2.7). Writing only `m_View`'s translation moves the per-view CB (post / muzzle flash) but not the world.
- **Per-eye pose (gameplay-correct):** hook `GameCameraManager::PushRenderContext` to write the head pose into the control contexts; the engine then derives a consistent `m_View`. Combine with the render-cam offset for true stereo.
- **Once-per-frame state to protect when double-dispatching:** clock (gate), EffectInfo slot bytes (snapshot/restore), eye-1 `m_Dt=0`, screenshot countdown (gate eye 0), end-draw callbacks (gate eye 1). Shadow parity is shared (benign).

---

## 7. Present / swapchain

- **Present** happens in [`graphics_flip`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-graphics_flip). Verified: it takes the context critical section (the immediate-context mutex — see §8), does GPU-timestamp bookkeeping (a 4-deep `&3` query ring), then issues the actual present as `device->m_SwapChain->Present(device->m_DeviceInfo.m_DisplayPresentationInterval /*sync interval*/, 0)` ([`Device::m_SwapChain`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Device##field-m_SwapChain), [`DeviceInfo::m_DisplayPresentationInterval`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::DeviceInfo##field-m_DisplayPresentationInterval)). The engine-side `GraphicsEngine::Flip` wraps it with `AcquireThreadOwnership`/`ReleaseThreadOwnership`.
- **Swapchain model:** classic **BitBlt `DXGI_SWAP_EFFECT_DISCARD`** (NOT flip-model), created via `IDXGIFactory::CreateSwapChain`. `DXGI_SWAP_CHAIN_DESC`: `Flags=2` (`ALLOW_MODE_SWITCH`), `BufferUsage=0x70` (RENDER_TARGET_OUTPUT | SHADER_INPUT), `SampleDesc.Count=1`. **`BufferCount = ((createFlags>>8)&3) - 1`**, asserted `>=2` → typically **2 back buffers** (double-buffered). Sync interval = `m_DisplayPresentationInterval` (from `CGraphicsEngine::m_FlipInterval`; 0 = no vsync). The swapchain owns the presentable buffers; the engine wraps back-buffer 0 as a `Graphics::HTexture_t` (RTV+SRV) and exposes it via `GetDeviceSurface(BACK_BUFFER)`.
- `RenderEngine::m_CurrBackBufferCopy` indexes `RenderEngine::m_BackBufferCopy`, the ring of scene-colour copies used as the screen-space refraction source (§1.3). It is not a history of presentable buffers; those live in the DXGI swapchain.
- **VR present suppression:** hooking `graphics_flip` and skipping the inner present vtable call is the correct point (the existing `BLOCK_FLIP` does this). The timestamp/query bookkeeping in the wrapper is safe to keep. Grab eye textures before suppressing.

## 8. D3D11 context model

- **The "master context" (`m_RenderContext.m_Context`) is a `Context` (`Graphics::HContext_t` wrapper) around the D3D11 *immediate* context.** At device init, `D3D11CreateDevice` writes the immediate context into [`Context::m_Context`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Context##field-m_Context); [`GetMasterContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-GetMasterContext) returns it. `HandleDrawThreadTask` fetches it each dispatch (`m_RenderContext.m_Context = GetMasterContext(...)`).
- **Deferred contexts exist** (`CreateDeferredContext`) for command-buffer recording; `FinishCommandList`/`ExecuteCommandList` (`Graphics::EndCommandBuffer`/`ApplyCommandBuffer`) record and replay onto the immediate context. `HandleDrawThreadTask` calls `ApplyCommandBuffer(m_RenderContext.m_Context)` early, then issues GBuffer/lighting/post/UI/resolve **directly on the immediate context**. So the render thread ultimately drives the immediate context.
- **Mutex:** every immediate-context op is guarded by [`Context::m_Mutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Context##field-m_Mutex) (a Win32 `CRITICAL_SECTION`), created at device init. Pattern: `ThreadMutexLock`/`Unlock` (`Graphics::CScopedCriticalSection`). `graphics_flip`, `CopyTexture`, `BeginDraw`, viewport sets all take it.
- **VR implications:** for `CreateTexture`/`CopyResource`/per-eye RT injection, use the **immediate context** (`Context::m_Context`) and **take `Context::m_Mutex`** around any immediate-context call to stay coherent with engine draws. Resource *creation* (`CreateTexture2D` on the `ID3D11Device`) is free-threaded (driver reports concurrent creates) and needs no context lock. `CRenderEngine::PostDraw` itself does not take the mutex — the existing mod's "context mutex" is this `Context::m_Mutex`.

## 9. Viewport / render resolution

- **Viewport is set per render-setup, not from a global.** `Graphics::SetRenderSetup`, when binding RTs, sets the viewport to the **bound color/depth target's own dimensions** (`vp = {0,0,target->m_Width, target->m_Height, 0, 1}; RSSetViewports(...)`). So per-pass viewport = bound RT size. (`Graphics::SetViewport` -> `RSSetViewports` exists for explicit sets.)
- **Not every pass draws at scene resolution.** Several passes redirect their draws to a reduced-resolution off-screen target: a shared quarter-resolution buffer (half per axis) that the low-resolution clouds, the low-resolution particles, and the volumetric spot-light cones all render into, plus the downsampled depth buffer beside it. Their composes then stretch the whole low-resolution texture over the whole scene target with a baked UV, and blend rather than overwrite. So the viewport in force mid-frame is not always the scene's, and anything deriving a screen region has to follow the bound target rather than the scene.
- **Render resolution source:** every scene RT in `CreateRenderSetups` is sized from its `SDeviceInfo` *parameter*'s `m_DisplayWidth`/`m_DisplayHeight`, which `ApplyResize` fills from `device->m_DeviceInfo`. Some derived RTs are `>>1` (half-res). `m_BackBufferLinear` is the exception: it is not sized at all, but built as a format alias of swapchain buffer 0, so it inherits the swapchain's dimensions (§7, and `docs/engine/rendering/render-setups-reinit.md` §4). The two coincide in the stock game because the same `ApplyResize` drives both.
- **No dynamic resolution / render-scale** found — RTs are allocated at full display resolution; no scale multiplier between display size and RT size.
- **Per-eye resolution:** because all scene RT sizes flow from the device info through `CreateRenderSetups`, setting the device display size to the per-eye render resolution and re-running `CreateRenderSetups` resizes the whole scene chain, and viewports follow automatically from the bound RT sizes — no per-pass viewport patching. `m_BackBufferLinear` and the two back-buffer render setups do not follow, because they derive from the swapchain rather than the device info; the mod substitutes its own (`docs/mod/stereo/swapchain-ownership.md`).

## 10. UI / HUD pipeline

- **Global instance:** [`GetIUIManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/ui::ui_manager##func-GetIUIManager) returns the [`CUIManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager) singleton — a concrete `CUIManager` (Scaleform GFx) singleton (only one instance), vtable `??_7CUIManager@@6B@`.
- **Resolved virtual calls** (the calls in `HandleDrawThreadTask`, modelled in the `UIManager` vtable in the defs):
  - `UIManager::StartRender` — kicks off the async UI render fragment.
  - `UIManager::SyncRender` — barrier (wait for UI render thread).
  - `UIManager::Submit` — locks master ctx, flushes UI draws (`m_RenderHAL->Submit`).
  - `UIManager::RenderOffScreenTextures` — in-world Scaleform screens (not HUD).
  - `UIManager::RenderStaticBackGround` — pause/menu background.
  - Mapping: the early block (before `PreDraw`) calls `RenderOffScreenTextures` then `StartRender`; the main UI block calls `Graphics::Clear(ctx, CLEAR_STENCILBUFFER, ...)` then `RenderStaticBackGround`, `SyncRender`, `Submit`, then `ResetContext`.
- **Which RT the HUD draws into:** **directly into `m_BackBufferLinear`** (the linear alias of the DXGI back buffer). [`CUIManager::InitPlatformRT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-InitPlatformRT) binds Scaleform's `m_RenderBuffer` to the RTV of `m_BackBufferLinear` + `m_MainDepthSurface` DSV; `CUIManager::Render` does `SetRenderTarget(m_RenderBuffer)` -> `HAL::Draw`. The HUD is composited **on top of the final LDR image**, into the same surface the composite pass just wrote. There is no separate HUD RT for the main overlay, and no copy afterwards (§12).
- **`HandleDrawThreadTask` runs once per frame, not per eye** — so the main UI block is a single HUD emission per real frame already. (If the VR layer calls only the *dispatch body* twice, you must decide which eye gets the UI block; if you call it per eye, gate UI to one.)
- **World-to-screen (markers / distance labels):** `CUIManager::Convert3DCoords` projects a world point to a HUD pixel: `world·vp`, divide by `|w|`, aspect-correct, map NDC→pixels with `m_ViewWidth`/`m_ViewHeight`, return `w > 0`. **The VP is a parameter**, so feeding a chosen per-eye VP relocates where every projected marker lands. Marker placement + off-screen edge-clamp: `CUIManager::Get2DInfo` → `CUIManager::ClampToScreen`; xref `Get2DInfo` to find each gameplay callsite's VP source. The default VP is the render camera's `m_ViewProjectionF`.
- **VR UI compositing:** for desktop dial-in we render the HUD into our own texture (hook `CUIManager::InitPlatformRT`, substitute our offscreen RTV for the surface RTV it binds) and draw it as an **in-scene quad per eye** in the stereo render — so it shows up in the side-by-side preview and we can tune distance/size/follow-lag without a headset. World markers are reprojected with the live per-eye VP rather than baked into the lagging panel (see `docs/mod/hud.md`). An `XrCompositionLayerQuad` is the sharper endpoint once in-headset. Simpler fallback: gate the `StartRender`/`Submit` pair to one eye and copy the HUD region into a layer.

## 11. Render-block / draw-list lifecycle across dispatches

The per-pass draw is **non-destructive**, but the per-frame list **rotation runs inside `GraphicsEngine::Draw`'s prologue** — so running the whole `GraphicsEngine::Draw` twice (as the mod does) rotates the lists twice and the second eye draws an **empty** buffer. This is the "eye 1 has no scene geometry" failure (measured: eye 0 = 194 indexed draws, eye 1 = 0; eye 1 still issues its ~53 fullscreen/non-RBI draws + 8 compute dispatches, which is why it isn't pure black).

- **Pass array:** `RenderEngine::m_RenderPasses` is the fixed array of per-pass lists (created at init), iterated by `DrawRenderPassRange` over pass categories (gbuffer `RP_Z_OCCLUDERS`..`RP_LAST_GBUFFER`, scene `RP_REFLECTIVE_WATER_PLANES`..`RP_POSTEFFECTS`, ...). Each `CRenderPass` owns a double-buffered `m_Lists[2]` (two [`CRBILists`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RBILists)) followed by [`RenderPass::m_CurrentAddList`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_CurrentAddList) / [`m_CurrentDrawList`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_CurrentDrawList).
- **`CRBILists` layout (from [`CRBILists::Add`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RBILists##func-Add)):** `m_List` (array of [`RenderInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderInstance) entries), `m_ListSize` (u16 capacity), `m_NumElements` (volatile u32; `Add` does `InterlockedExchangeAdd(m_NumElements,1)`). On overflow (`count >= cap`) `Add` spills to a global overflow list ([`render_block_overflow_count`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::render_block_overflow_count) plus its entries). The overflow list's capacity and entry size are not yet confirmed.
- **Population (sim, once/frame):** sim systems append blocks into `m_CurrentAddList` via `CRBILists::Add`, dispatched as worker jobs. [`CRenderPass::SetupRenderFrameData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SetupRenderFrameData) is one such per-batch *build* appender — it does **not** swap; it appends `count` items.
- **Rotation / swap (in EVERY `GraphicsEngine::Draw` prologue):** `GraphicsEngine::Draw` calls `CKeep1000Frames::CKeep1000Frames` in its prologue (IDA's label for the inlined `CRenderPass::SetupRenderFrameData` merged with the profiling constructor). It: (1) toggles a global 1-bit parity [`current_add_buffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::current_add_buffer) (`parity = (parity-1)&1`); (2) loops all passes (via a sub mislabeled `std::map::operator[]`) calling each pass's virtual [`CRenderPass::SaveRenderFrameData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SaveRenderFrameData): `m_CurrentAddList = &m_Lists[parity]`, `m_CurrentDrawList = &m_Lists[(parity-1)&1]`, and **zeroes the new add-list's `m_NumElements`**; (3) flushes the overflow global back into the lists and resets its count.
- **Drawing does NOT consume:** the virtual [`CRenderPass::DoDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-DoDraw) loads `m_CurrentDrawList`, draws `min(m_ListSize, m_NumElements)` blocks via a local cursor, and **never writes `m_NumElements`**. A buffer can be redrawn any number of times.
- **Why eye 1 is empty (running the whole `Draw` twice without saving the parity):** eye 0's rotation points every `m_CurrentDrawList` at the sim-populated buffer → 194 draws. Eye 1's rotation toggles parity *back*, re-points `m_CurrentDrawList` to the buffer eye 0's rotation had just zeroed **and zeroes eye 0's buffer in turn** → 0 draws. The geometry is not consumed by drawing; it is wiped by the *second* rotation. Saving/restoring `current_add_buffer` between eyes prevents this.
- **`CRenderEngine::EraseAllDeletedRenderBlocks` (called from `HandleDrawThreadTask`):** a *separate* deletion list; does not touch pass draw lists. Benign across dispatches.
- **Sort task (`m_SortTaskProxy`):** each dispatch creates a fresh sort proxy that sorts each `m_CurrentDrawList` in place; the dispatch spinwaits before `EndDraw`. Sorting an already-sorted stable list is idempotent → safe across two dispatches (serialized by the single render thread).

**Two correct fixes:**
1. **Save/restore `current_add_buffer` between eyes** — what the mod does: snapshot the parity before eye 0's `Draw` and restore it before eye 1's `Draw`. Eye 1's `CKeep1000Frames` then toggles it to the same value as eye 0, so `SaveRenderFrameData` sets the same list pointers and zeroes the same add-list. Eye 1 draws from the same populated draw list as eye 0. The function runs on both eyes, so the overflow list is processed and the external render camera is updated on both eyes too — no side effects from skipping.
2. **Run only the dispatch body twice** (`DispatchDraw`/`HandleDrawThreadTask`) under a single `GraphicsEngine::Draw` prologue — the rotation then runs once and both dispatches read the same populated buffer. More invasive; the mod instead runs the whole `Draw` twice and saves/restores the per-dispatch side effects.

Either way, **no geometry snapshot/restore is needed** — the draw is non-destructive; you only have to stop the *second rotation* from wiping the lists.

## 12. Per-eye output routing

**There is no final resolve.** The `EndDraw` call in `HandleDrawThreadTask` (§1.4 step 15) is not `Graphics::CopySurfaceToTexture`, an MSAA resolve of the composite into the device's presentable surface, as its IDB name suggests: two mislabelled symbols in the IDB swap a pair of `Graphics::` functions (`docs/mod/stereo/swapchain-ownership.md` §2):

- The function labelled `Graphics::CopySurfaceToTexture` is really **[`Graphics::EndDraw(ctx)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-EndDraw)** — it reads no argument beyond the context, takes the context critical section, and submits the frame's two prebuilt command objects from the command-slot ring. This is the call in §1.4 step 15.
- The function labelled `NGraphicsEngine::CGPUProfiler::BeginScope` is really **[`Graphics::CopySurfaceToTexture(ctx, dst, src)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-CopySurfaceToTexture)** — a whole-resource `CopyResource`. Its one call in `HandleDrawThreadTask` is the VFX depth copy of the EffectInfo slot machine (§5.1), nothing to do with the back buffer.

Nothing copies to the back buffer because there is nothing to copy: `m_BackBufferLinear` is a **format alias** of swapchain buffer 0 (`CreateSurfaceAlias` memcpys the source handle, `AddRef`s the same `ID3D11Resource`, and builds fresh views), so the composite pass bound to `m_BackBufferRenderSetup` writes *directly into* the presentable surface. `Graphics::Flip` then presents it.

**Consequence for per-eye routing.** `CopyResource` `m_BackBufferLinear`'s texture into the per-eye texture after the composite — what the mod does in the `PostDraw`-adjacent path — is correct: it reads the composite target, with no resolve to copy after. There is no resolve to suppress or retarget. Do the copy on the **immediate context under `Context::m_Mutex`**, once per eye dispatch.

---

## 13. To dispatch one correct stereo eye — checklist

Two viable structures (§11): **(A)** run the whole `GraphicsEngine::Draw` twice and gate/snapshot the per-frame + per-dispatch side effects (what the mod does), or **(B)** run only the **dispatch body** twice under a single prologue. The lists below assume **(A)**; under **(B)** the prologue items (frame counter, jitter phase, CB ring, clock, **list rotation**) run once and need no gating.

**SET (per eye), on the render camera `GraphicsEngine::m_RenderCamera` — hook `Camera::SetupRenderCamera` (`this == &engine->m_RenderCamera`) or right after the active->render memcpy in `GraphicsEngine::TextureCachePlatformUpdate`. The scene is camera-relative (§2.4), so:**
- `m_TransformF` translation: lateral IPD/2 offset along the camera right axis (the camera *world position* — this is what camera-relative geometry subtracts). Do NOT just offset `m_View`'s translation; the OffsetVP zeros it.
- Re-derive `m_View = Inverse(m_TransformF)`, then rebuild `m_ViewProjection`/`m_ViewProjectionF` from `m_View x m_Projection` (§2.6) so the per-view CB matches.
- Per-eye off-axis *projection* (optional): write a standard (non-reverse-Z) `m_Projection` before `SetupRenderCamera` so it reverse-Z's + jitters once (§2.7).
- After the dispatch: `WaitForCPUDrawToFinish`, then `CopyResource(eyeN_tex, m_BackBufferLinear)` under `Context::m_Mutex` (§12).

**SNAPSHOT / RESTORE around the eye pair (so once-per-dispatch state doesn't double-step):**
- EffectInfo slot bytes (`m_EffectInfo[0..4].m_FrameIndex`) + `m_EffectInfoIndex` (§5.1).
- Per-eye previous-frame VP for velocity if you keep motion blur / `AA_SMAA_T2X` (§5.7); else disable them.
- (Optional) restore the sort proxy state to skip the eye-1 re-sort (perf only, §11).

**GATE to a single eye:**
- **Render-list parity `current_add_buffer` (in the `Draw` prologue) -> save/restore between eyes** — eye 1's `CKeep1000Frames` must toggle to the same value as eye 0's so `SaveRenderFrameData` sets the same list pointers. Without this, eye 1's rotation flips every `m_CurrentDrawList` to the buffer eye 0 just zeroed and draws no scene geometry (§11). This is the core geometry fix under structure (A); not needed under (B).
- Draw-done `SetEvent`/`m_CPUDrawFinished = 1` -> **eye 1 (last) only** (§5.6).
- Screenshot countdown `m_FramesUntilScreenshot` -> eye 0 only (§5.5).
- End-draw `sig::Signal` one-shot callbacks -> eye 1 only (§5.5).
- Main UI block (§1.4 step 13) -> one eye (or redirect to a UI layer RT, §10).
- Eye-1 render-context `m_Dt = 0` so dt accumulators (fade/exposure/heat-haze) don't double-step (§5.2).

**LEAVE ALONE (do not touch / shared is fine):**
- Draw-list *contents* — non-destructively drawn, so no re-population/snapshot needed (§11). The per-frame *rotation* runs on both eyes but the parity is saved/restored so both eyes see the same list pointers under structure (A).
- Shadow parity (`render_frame_counters.m_FrameIndex & 1`) — shared slot, benign (§5.3).
- `Clock::Update` — runs once in the sim prologue; do not call per dispatch (§5.4).
- Frame counter / jitter phase / `%3` CB ring — advanced once in the prologue; keep both eyes on the same values (§5.7).
- `graphics_flip` — suppressed once per frame via `BLOCK_FLIP`; present via OpenXR.

---

## Open questions / unverified

1. **GBuffer1/2/3 and aux-RT exact formats** beyond GBuffer0 (`ABGR32`): GBuffer1 as `A2R10G10B10` is unverified (out of scope per the brief — best-effort).
2. **Exposure adaptation EMA** exact location/decay (inferred to be in the tonemapping effect; not line-traced). Neutralized by eye-1 `m_Dt=0` regardless.
3. **`render_frame_index` / `previous_render_context_frame_index` consumers** (the `%3` ring values): not traced; presumed triple-buffered per-frame resources. Advanced once/frame in the prologue, so benign for double-dispatch.
4. **TAA history RT binding** (§5.7): the camera-side snapshot chain is verified, and the EffectInfo VP history is `EffectInfo::m_Transform`; the downstream history RT names were not traced.
5. **Reflection-proxy RT formats / `VfxDepthCopy_%d` count**: names confirmed, formats not.
6. **Which draw entry point WaveWorks uses** (§4.2). [`CNvWaterHighEndRenderBlock::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::NvWaterHighEndRenderBlock##func-Draw) hands a raw `ID3D11DeviceContext` to `GFSDK_WaveWorks_Quadtree_DrawD3D11`, an import from `gfsdk_waveworks.win64.dll`, so which of slots 12/20/39/40/21 the water geometry lands on is not visible in the IDB. A per-slot counter keyed to the water pass ids, or a frame capture, settles it in one run.
7. **Whether WaveWorks' savestate restore stomps the viewport.** `GFSDK_WaveWorks_Savestate_RestoreD3D11` restores rasterizer state around the water draws through the same context the engine draws on; what exactly it puts back is not determinable from the IDB.
8. **Whether `UVScale` is ever non-unity.** The fullscreen reconstruction vertex shaders build their G-buffer UV as `o1 = (v0 · 0.5 + 0.5) · cb1[5].xy` (`UVScale`), which is only a no-op while that constant is `(1, 1)`. The constant is [`RenderContext::m_UVScale`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_UVScale), which `SetupRenderContext` copies each pass from [`GraphicsEngine::m_UVScale`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_UVScale). Nothing has been found that writes the engine field otherwise, but its writer was not traced.
9. **Camera-relative per-object combine site (§2.4)**: the mechanism is established from `SetRenderContextCamera` storing a translation-free OffsetVP (`CalculateOffsetViewProjectionMatrix`) alongside a separate `CameraPosition` (from `m_TransformF`'s translation) — there is no other reason to keep both. The literal per-object `world.row3 -= CameraPosition` multiply in the model render block was not line-verified (a claimed address was a misattribution); not needed for the camera-side fix.
