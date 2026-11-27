# JC3 per-frame rendering pipeline

The wiring of Just Cause 3's frame: what each piece is, what reads what, and the frame order. Addresses, offsets, singletons, and globals live in the pyxis defs (the `graphics_engine/`, `camera/`, and `ui/` modules); follow a name there. Claims not confirmed are marked unverified. The shader side (extracting, disassembling, and finding shaders, and how the engine creates them) is in [`shaders.md`](shaders.md).

The key singletons are [`GraphicsEngine`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine) (the scene render targets, the main render context, and the render camera), [`RenderEngine`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine) (the pass array and the global shader-constant staging blocks), the engine-side [`CameraManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera_manager::CameraManager), the separate [`GameCameraManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::game_camera_manager::GameCameraManager) (`Base::CSingle<GameCameraManager>::Instance`) that drives it, and [`Clock`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/clock::Clock).

## 1. Threading & frame structure

### 1.1 Two threads

The main thread runs `Game::Update` -> `Game::UpdateRender` -> `Game::Draw` -> `GraphicsEngine::Draw`. The draw worker runs `GraphicsEngine::HandleDrawThreadTask`, which builds and submits the frame's GPU work. `GraphicsEngine::DispatchDraw` chooses: with one CPU primary (`CpuPrimaryCount() <= 1`) it calls `HandleDrawThreadTask` inline; otherwise it posts a `CpuFragmentCall` ("CallProxy") whose function, [`HandleDrawThreadTaskProxy`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-HandleDrawThreadTaskProxy), tail-calls it on a worker. The work is identical either way.

### 1.2 What `WaitForCPUDrawToFinish` drains

`GraphicsEngine::WaitForCPUDrawToFinish` is the fence between the draw worker and the next frame's sim work. It waits on the end-draw fragment signal ([`GraphicsEngine::m_DrawFragmentSignal`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DrawFragmentSignal), a `shared_ptr<SCallProxyKeepAlive>`) if set, then, unless [`GraphicsEngine::m_CPUDrawFinished`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_CPUDrawFinished) is set, on the Win32 event [`GraphicsEngine::m_CPUFinishedDrawingEvent`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_CPUFinishedDrawingEvent), which the tail of `HandleDrawThreadTask` sets. `Game::Draw` calls it at entry, and `GraphicsEngine::Draw` re-checks the same two conditions in its prologue.

### 1.3 Prologue of `GraphicsEngine::Draw` — per-real-frame-once state

In order:

| Operation | Notes |
|---|---|
| wait on the fragment signal and the draw-done event | drains the previous frame |
| `m_RenderContext.m_Dt = dt` ([`RenderContext::m_Dt`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_Dt)) | the frame dt for the worker |
| `render_frame_counters.m_FrameIndex = render_frame_counters.m_Counter++` | the frame counter and parity source |
| [`render_frame_index`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::render_frame_index) `= m_Counter % 3`; [`previous_render_context_frame_index`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::previous_render_context_frame_index) `= m_RingIndex`; `m_RingIndex = m_FrameIndex % 3` | the three-slot ring used by render-block and constant-buffer indices |
| `++m_CurrBackBufferCopy`, wrapping at `m_BackBufferCopyCount` | [`RenderEngine::m_CurrBackBufferCopy`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_CurrBackBufferCopy) selects the slot of [`RenderEngine::m_BackBufferCopy`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_BackBufferCopy), the ring of scene-colour copies that is the screen-space refraction source, not a ring of presentable buffers (§7) |
| `TextureCachePreUpdate`, `CGameStateRun::UpdatePostEffectEdit` | texture streaming; the dev post-effect edit sync |
| [`GraphicsEngine::TextureCachePlatformUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-TextureCachePlatformUpdate) | the active-to-render camera copy (§2.3) |
| `GraphicsEngine::Flip` unless [`m_FirstFrame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_FirstFrame) | presents the previous frame (§7); `m_FirstFrame` is then cleared |
| `HandleModeChange` | services a pending resize ([render-setups-reinit.md](render-setups-reinit.md)) |
| `Clock::Update` | the per-frame clock tick |
| `CConstantBufferPool::HandBackBuffers`, `CRenderEngine::CalculateConstantBufferIndices` | constant-buffer pool rotation and per-block draw indices |
| `DispatchDraw` | hands the frame to the worker |

The prologue also rotates the render-pass draw lists (§11). Presenting is pipelined: flip the previous frame, then dispatch the current one. [`GraphicsEngine::Flip`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-Flip) is `AcquireThreadOwnership; graphics_flip(device); ReleaseThreadOwnership`.

### 1.4 Body of `HandleDrawThreadTask` — per-dispatch

[`GraphicsEngine::HandleDrawThreadTask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleDrawThreadTask), in order:

1. `QueryPerformanceCounter` into [`draw_start_time`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::draw_start_time); `GetMasterContext` into [`RenderContext::m_Context`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_Context).
2. `ShadowManager::CommitRenderPassSettings` if [`shadow_system_enabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::shadow_manager::shadow_system_enabled); `TextureCacheGpuUpdate`.
3. `RenderPass::SetupRenderContext` on [`GraphicsEngine::m_RenderContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_RenderContext), then `RenderPass::SetRenderContextCamera` with `CameraManager.m_RenderCamera` (§2.4).
4. `RenderEngine::SetGlobalShaderConstants` (the per-view constant buffer) and `SetAllGlobalShaderProgramConstants`.
5. If [`GraphicsEngine::m_DrawScene`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DrawScene): [`UIManager::RenderOffScreenTextures`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-RenderOffScreenTextures) and [`UIManager::StartRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-StartRender); `PreDraw`; `RenderEngine::DrawGBuffer` (`RP_Z_OCCLUDERS..RP_LAST_GBUFFER`); the EffectInfo slot machine (§5.1); `RenderEngine::Draw` (`RP_REFLECTIVE_WATER_PLANES..RP_PARTICLE_ONSCREEN`); `PostEffectsManager::ApplyWorldFilters(dt)`; `RenderEngine::DrawPosteffects` (`RP_POSTEFFECTS`). Otherwise only the UI submit.
6. `PostEffectsManager::ApplyGlobalFilters(dt)`, then `RenderEngine::PostDraw`.
7. GPU-profiler `EndFrame` and the screenshot countdown [`GraphicsEngine::m_FramesUntilScreenshot`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_FramesUntilScreenshot) (two branches).
8. The UI block: a stencil clear, then [`UIManager::RenderStaticBackGround`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-RenderStaticBackGround), [`SyncRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-SyncRender), [`Submit`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-Submit), and `ResetContext`.
9. `QuickDrawController::Draw` (debug overlays).
10. `Graphics::EndDraw`, which submits the frame's command work; it is not a copy to the presentable surface (§12).
11. The end-draw one-shot `sig::Signal` callbacks, which remove themselves; `RenderEngine::EraseAllDeletedRenderBlocks`.
12. The draw-time floats [`GraphicsEngine::m_CPUDrawStallDuration`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_CPUDrawStallDuration) and [`GraphicsEngine::m_DrawTime`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DrawTime), then `m_CPUDrawFinished = 1; SetEvent(m_CPUFinishedDrawingEvent)`.

### 1.5 Per-frame vs per-dispatch

Once per real frame, in the `Draw` prologue: the frame counters and parity, the three-slot ring, the scene-colour copy slot, the texture-cache updates, the camera copy, the draw-list rotation, `Clock::Update`, and the constant-buffer pool and indices. Once per dispatch, in `HandleDrawThreadTask`: the whole pass chain, the EffectInfo slot machine, the filters' dt accumulators, the screenshot countdown, the profiler frame, the end-draw callbacks, and the draw-done event. §5 lists what each advances.

## 2. Camera data flow

### 2.1 Cast of cameras

`GameCameraManager` holds `m_ControlContext` with four sub-contexts (previous and next, camera and render), each carrying a world transform `m_CameraTransform` (not a view matrix), the FOV, and blur factors. `CameraManager` holds [`m_ActiveCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera_manager::CameraManager##field-m_ActiveCamera), the active scene [`Camera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera), and [`m_RenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera_manager::CameraManager##field-m_RenderCamera), the camera the draw thread reads. A `Camera` carries `m_PreviousTransformF`, `m_TransformF`, `m_TransformT0`, `m_TransformT1`, `m_ProjectionF`, `m_ViewProjectionF`, `m_Projection`, `m_View`, `m_ViewProjection`, and their previous-frame copies. [`Camera::m_StateBitfield`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##field-m_StateBitfield) ([`CameraState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState)) holds [`m_ComputeView`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState##flag-m_ComputeView), [`m_DirtyProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState##flag-m_DirtyProjection), and [`m_IsRenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState##flag-m_IsRenderCamera).

### 2.2 Sim path: where `m_ActiveCamera.m_View` is produced (once per frame)

`GameCameraManager::UpdateRender` runs on the sim path, under `CGameStateRun::UpdateRender` and before `Game::Draw`:

```
CCameraTree::UpdateRenderContexts(...)         // fill the m_ControlContext contexts
CSpecialCameras::UpdateRender(...)
GameCameraManager::PushRenderContext(this)     // -> CameraManager::InitTransform
CameraManager::UpdateRender(dt, dtf)           // -> Camera::UpdateRender for every camera
```

[`CameraManager::InitTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera_manager::CameraManager##func-InitTransform) copies the context transform into both `m_ActiveCamera.m_TransformT0` and `m_TransformT1`. [`Camera::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-UpdateRender) then computes the view:

```c
m_PreviousView = m_View;  m_PreviousProj = m_Projection;  m_PreviousViewProjection = m_ViewProjection;
m_PreviousTransformF = m_TransformF;
Lerp(&m_TransformF, &m_TransformT0, &m_TransformT1, dtf);
if ( m_StateBitfield & m_ComputeView ) { m_View = m_TransformF; CMatrix4f::Inverse(&m_View); }
Multiply4x4(&m_View, &m_Projection, &m_ViewProjection);
m_ProjectionF = m_Projection;  m_ViewProjectionF = m_ViewProjection;  ...
```

The active gameplay camera has `m_ComputeView` set ([`Camera::SetComputeView`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-SetComputeView)), so `m_View = Inverse(m_TransformF)` every sim frame. Because `InitTransform` set T0 equal to T1, the lerp is constant and pose interpolation is effectively off for the active camera, whatever `m_InterpolationOverride` says. `m_View` is computed once per real frame, here, and never at draw time.

### 2.3 Draw path: active -> render camera copy

Near the top of `TextureCachePlatformUpdate`:

```c
memcpy(&this->m_RenderCamera, CameraManager.m_ActiveCamera, sizeof(Camera));
Camera::SetupRenderCamera(&this->m_RenderCamera, 1);
CameraManager.m_RenderCamera = &this->m_RenderCamera;
```

The render camera is [`GraphicsEngine::m_RenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_RenderCamera), a byte copy of the active camera, `m_View` included, made once per frame. [`Camera::SetupRenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-SetupRenderCamera):

```c
if ( (m_StateBitfield & m_IsRenderCamera) == 0 ) {   // check-and-set: a no-op once set
    m_StateBitfield |= m_IsRenderCamera;
    // reverse-Z on m_Projection, then on m_PreviousProj (§2.7)
    if ( a2 ) {                                     // apply jitter; 1 from the prologue
        ApplyJitterTransform(engine, m_Projection, ...);
        ApplyJitterTransform(engine, m_PreviousProj, ...);
    }
    Multiply4x4(m_PreviousView, m_PreviousProj, m_PreviousViewProjection);
    Multiply4x4(m_View,         m_Projection,   m_ViewProjection);
    m_ProjectionF = m_Projection;  m_ViewProjectionF = m_ViewProjection;
    // and the secondary projection and view-projection float copies
}
```

`Camera::UpdateRender` asserts `m_IsRenderCamera` is clear, so the active camera never has it, the copy lands with it clear, and the full body runs on every draw, rebuilding the view-projections from `m_View · m_Projection`. A second call on the same copy is a no-op. `SetupRenderCamera` reads `m_View` and never writes or derives it, so a write to the render camera's `m_View` after it has run leaves `m_ViewProjection` and `m_ViewProjectionF` stale until rebuilt by hand (§2.6).

`ApplyJitterTransform` reaches [`AntiAliasingEffect::ApplySubsampleJitter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AntiAliasingEffect##func-ApplySubsampleJitter), which acts only in [`AAMode::AA_SMAA_T2X`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AAMode##variant-AA_SMAA_T2X): it post-multiplies a sub-pixel clip-space translation (`m[12] = jx`, `m[13] = jy`, each the offset over the target width or height) onto the projection, `proj = proj · jitterMat`. The current and previous projections take the same offset, this frame's phase, so the velocity pass's two positions both carry it and the motion it encodes does not. Only the scene render camera is jittered: a pass's external camera (the reflection and shadow cameras) is set up without the flag by [`RenderPass::GetSortCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-GetSortCamera) and [`RenderPass::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-Draw), and the visibility cull and the sun-shadow fit read the unjittered active camera. Every pass that draws with the render camera's constants, depth reconstruction included, sees the jittered projection, so they agree with the jittered depth.

### 2.4 What the scene render actually reads (camera-relative)

The main scene renders camera-relative, for large-world precision: `clip = (objectWorld - CameraPosition) · OffsetViewProjection`, where `OffsetViewProjection` is the view-projection with the view's translation row zeroed and `CameraPosition` is the camera's world position, supplied separately.

[`RenderPass::SetRenderContextCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SetRenderContextCamera) (the non-shadow path) fills the render context from the render camera: the view matrix from `m_View`, the projection from `m_ProjectionF`, `CameraPosition` from `m_TransformF`'s translation row, the full `m_TransformF` rows, and, through `CalculateOffsetViewProjectionMatrix` called for the current and previous frames, the offset view-projection: `m_View` with its translation row set to `{0,0,0,1}`, times the projection. That is the matrix opaque geometry uses.

`CRenderEngine::SetGlobalShaderConstants` copies the render camera's full, translation-bearing `m_ViewProjectionF` into [`RenderEngine::m_VPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_VPGlobalConstData), with the camera-position constants from `m_TransformF`. That per-view block drives screen-space and non-geometry work (post effects, billboards, the camera-position constant), not opaque vertices.

So `m_View`'s translation never reaches geometry. Moving the viewpoint means moving `m_TransformF`'s translation and re-deriving `m_View` from it; changing only `m_View`'s translation diverges the per-view block's `m_ViewProjectionF` (the muzzle flash moves) while the geometry stays byte-identical.

### 2.6 The VP-rebuild recipe

The engine's `Matrix4` (`CMatrix4f`) is D3D-style, row-major and row-vector (`clip = p · M`), and `Matrix4::Multiply4x4(a, b, dest)` writes `dest = a · b`; the full convention (basis in rows, the glam bridge) is on the `Matrix4` def. `SetupRenderCamera` and `Camera::UpdateRender` both compute `m_ViewProjection = m_View · m_Projection`. Rebuilding a render camera after writing `m_View` or `m_Projection`:

```c
Multiply4x4(&cam->m_View, &cam->m_Projection, &cam->m_ViewProjection);
memcpy(&cam->m_ProjectionF,     &cam->m_Projection,     sizeof(CMatrix4f));
memcpy(&cam->m_ViewProjectionF, &cam->m_ViewProjection, sizeof(CMatrix4f));
```

The `*F` copies are what the render context and the global constants read (§2.4), so they must be updated too.

### 2.7 Reverse-Z / depth convention + the per-eye projection wedge bug

`Camera::RecalcProjection` builds a standard projection (§2.9). The reverse-Z remap `z' = w - z` is applied once per render camera, by whichever of two sites first sees `m_IsRenderCamera` clear: [`RecalcProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-RecalcProjection) applies it at its tail only when `m_IsRenderCamera` is already set, and `SetupRenderCamera` applies it, under its guard, to whatever `m_Projection` holds, with no check for an already reversed matrix. The depth convention is reverse-Z with NDC z in [0, 1], near at 1 and far at 0, on a `D32FS8` MainDepth.

So a projection written into the render camera's `m_Projection` before `SetupRenderCamera` runs is reverse-Z'd and jittered exactly once, and must be a standard one; an already reversed matrix written there is remapped twice, which renders as a wedge. A projection written after `SetupRenderCamera` (flag set) must already be reversed, and then the view-projections (§2.6), the jitter, and `m_PreviousProj` and `m_PreviousViewProjection` for velocity are the writer's to maintain.

### 2.8 Vehicle camera path

`GameCameraManager::PushRenderContext` routes a driving vehicle's transform differently: its `IsInDrivingVehicleState` branch passes a raw matrix and bypasses the rotation jitter filter, so the vehicle camera behaves differently from on foot.

### 2.9 Projection construction — exact formulas, real near/far, and consume-vs-rebuild (decompiled)

`Camera::RecalcProjection` builds `m_Projection` by `m_StateBitfield`: [`m_Ortho`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState##flag-m_Ortho) selects `CMatrix4f::Ortho`, [`m_UseOffCenter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::CameraState##flag-m_UseOffCenter) (off-centre tiling) `CMatrix4f::PerspectiveOffCenter`, and otherwise `CMatrix4f::PerspectiveFov`. All read `m_Near` and `m_Far`, with `m_FOV` and `m_AspectRatio`. There is no infinite-far form; the far plane is a finite field.

`CMatrix4f::PerspectiveOffCenter(this, x_min, x_max, y_min, y_max, near, far)` writes, row-major (`clip = p·M`):

```
e[0]  = 2·near / (x_max − x_min)          e[5]  = 2·near / (y_max − y_min)
e[8]  = (x_min + x_max) / (x_max − x_min)  e[9]  = (y_min + y_max) / (y_max − y_min)
e[10] = far / (near − far)                 e[11] = −1
e[14] = near·far / (near − far)            e[15] = 0   (all others 0)
```

`RecalcProjection` passes near-plane extents (`x = near·tan θ`, through the engine's polynomial sin and cos of `m_FOV`), so `near` cancels from `e[0]`, `e[5]`, `e[8]`, and `e[9]`. This is `DirectXMath`'s `PerspectiveOffCenterRH` (standard depth, near to 0, far to 1); `PerspectiveFov` is the symmetric case. The reverse-Z remap is in place on the depth column, `e[row·4+2] = e[row·4+3] − e[row·4+2]` for all four rows.

`SetupRenderCamera` consumes rather than rebuilds: it never calls `PerspectiveFov` or `PerspectiveOffCenter` and never reads `m_FOV`, `m_Near`, or `m_Far`; it remaps, jitters, and multiplies whatever matrix it finds (§2.3).

The `Camera` constructor sets `m_Near` to [`Camera::DEFAULT_NEAR_PLANE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera::DEFAULT_NEAR_PLANE) and `m_Far` to [`Camera::DEFAULT_FAR_PLANE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera::DEFAULT_FAR_PLANE). No gameplay-wide override was found; only auxiliary cameras diverge (the `CWaterPatchManager` reflection and top-down cameras, and the shadow and RSM light cameras at the default near), and `CCollisionModule` nudges the near plane when the camera clips geometry. The scene camera therefore renders a finite-far reverse-Z frustum at the defaults, the far plane matching the world's draw distance.

## 3. Render passes / stages (ordered)

Passes are a flat, ordered enum `ERenderPass` of `RP_*` values (names from the pass-name switch), drawn by index range through [`RenderEngine::DrawRenderPassRange(ctx, renderSetup, first, last)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawRenderPassRange), which walks [`RenderEngine::m_RenderPasses`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_RenderPasses) and vtable-dispatches each pass's render blocks from its current draw list (§11). Per dispatch:

1. Shadow atlas: committed in `CShadowManager::CommitRenderPassSettings` and drawn as `RP_SHADOW_0..7`, `RP_STATIC_SHADOW_0..7`, and `RP_SHADOW_REFLECTIVE_*`, parity-buffered by `render_frame_counters.m_FrameIndex & 1` ([lighting-shadow-pipeline.md](lighting-shadow-pipeline.md) §3).
2. G-buffer: [`RenderEngine::DrawGBuffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawGBuffer) over `RP_Z_OCCLUDERS..RP_LAST_GBUFFER`: the depth and velocity prefix `RP_Z_OCCLUDERS`, `RP_Z_COARSE_PASS`, `RP_Z_PASS`, and `RP_Z_AND_VELOCITY_PASS` (the velocity buffer, from the previous frame's view-projection), then static and dynamic models, then `RP_DECALS`, `RP_SCREEN_SPACE_DECALS`, and `RP_SCREEN_SPACE_ROAD_DECALS`. It first binds two global textures on fragment texture slots 40 and 41.
3. Lighting, reflections, and the main scene: [`RenderEngine::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-Draw) from `RP_REFLECTIVE_WATER_PLANES` to just before `RP_POSTEFFECTS`: `RP_REFLECTIVE_WATER_PLANES`, `RP_AO_VOLUMES`, `RP_SSAO`, `RP_SCREEN_SPACE_REFLECTIONS`, `RP_GLOBAL_ILLUMINATION`, `RP_SCREEN_SPACE_SUBSURFACE_SKIN`, and `RP_DEFERRED_LIGHTS` (resolving into MainColor), then the opaques to `RP_LAST_OPAQUE`, the environment (`RP_STARS`, `RP_SUN`, `RP_MOON`, `RP_SKYBOX`), water, and transparency. The reflection-proxy passes (`RP_REFLECTION_*`) and `RP_ENVREFLECTION` are in this range.
4. World post effects: `CPostEffectsManager::ApplyWorldFilters` enqueues the block, and [`RenderEngine::DrawPosteffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-DrawPosteffects) draws the one-pass range at `RP_POSTEFFECTS`. `CRenderBlockPostEffects::Draw` runs, in order:
   1. `CToneMappingEffect::Apply`: publishes the applied exposure and meters the exposure-weighted histogram (§5.4).
   2. `CSunHaloEffect::PreApply` (§3.3).
   3. The blur: `CDownScale2x2PackFocus::Apply` then `CBlurEffectBokeh::Apply` if `CPostEffectsManager::IsBokehActive`, else `CBlurEffect::Apply`.
   4. `CGlareEffect::Apply` (§3.3).
   5. `CDepthOfFieldEffect::Apply`, which is the HDR-to-LDR composite (§3.1).
   6. `CMotionBlurEffect::Apply`, gated by motion blur, `AA_SMAA_T2X`, or the heat haze.
   7. `CToneMappingEffect::GenerateHistogramForFinalScene`: the raw final-scene histogram (§5.4).
   8. `CPlayerDamageEffect::Apply`, when the damage flag is set.
   9. `CAntiAliasingEffect::Apply`.
   10. `CSunHaloEffect::Apply` with an additive blend.
   11. `CFadeEffect::Apply`.
5. Global filters: `CPostEffectsManager::ApplyGlobalFilters` enqueues `RP_POSTEFFECTS_GLOBAL` work (the screen fade, the heat haze, the sun-direction accumulation; §5.2).
6. `CRenderEngine::PostDraw`, the UI block, and `QuickDrawController::Draw`.

### 3.1 The HDR->LDR composite

`CDepthOfFieldEffect::Apply` turns the R11G11B10F HDR MainColor into the LDR target, applying the published exposure (fragment constant `c2.x`, from `PostEffectContext::m_Exposure`) and the tone-map operator. It picks its fragment program from four families, created in `CDepthOfFieldEffect::Init` per tone-mapping operator: `UW_*` when the camera is underwater; otherwise `Dof_*`, or `HHDof_*` with the heat haze on, when [`PostEffectsManager::IsBokehActive`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager##func-IsBokehActive); and otherwise `Comp_*`, the exposure, grading, and tonemap without depth of field. `IsBokehActive` is the chain's one depth-of-field gate: it holds while the chain is enabled, any of three further enable flags on the manager is set (what sets them is unverified), and either focal range has a nonzero distance and blur, or the heat haze is on. Its only callers are the post block (choosing between the bokeh prepass and the plain blur, whose result the glare also reads) and the composite (twice: to choose its family and to upload the focal parameters).

### 3.2 Ping-pong slot scheme (why per-eye intermediate captures alias)

`CRenderBlockPostEffects::Draw` threads one "current result slot" integer through the chain: each of depth of field, motion blur, player damage, and anti-aliasing renders into `m_RenderSetups[(current+1)%3]`, samples temp `[current]`, and returns the advanced index. The temps are `m_FullscreenSrgbTempTexture[3]` and `m_FullscreenLinearTempTexture[3]` with `m_RenderSetups[3]` (count [`s_FullscreenTempTextureCount`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager::FULLSCREEN_TEMP_TEXTURE_COUNT)). Each effect exposes its output through `GetResultTexture`, offset by [`PostEffectsManager::RESULT_TEXTURE_SLOT_BASE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager::RESULT_TEXTURE_SLOT_BASE), into the next consumer's input. The slot index advances per dispatch, while the frame's three-slot ring advances per real frame, so two dispatches in one frame leave a given slot holding different intermediates; only the final composite target is stable.

### 3.3 The glare chain and the sun halo

[`GlareEffect::Apply`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::GlareEffect##func-Apply) builds the bloom from the blur step's half-resolution scene (the bokeh texture when bokeh is active). Every draw is one fullscreen triangle through the post manager's textured fullscreen vertex program, addressed by screen UV, through the effect's own bilinear sampler that clamps on every axis. The chain owns ping-pong pairs at a half, quarter, eighth, and sixteenth of the screen (each by shifting the screen size right), a half-resolution bright-pass copy, a half-resolution single-channel binary image, and a pair of half-resolution targets for the temporal blend. In order:

1. The bright pass (`highpass_rh`, `highpass_u2`, or `highpass_pg`, by the tone-mapping technique) scales the source by the exposure, subtracts a threshold, and raises it to the bloom contrast into the first half-resolution target, first copying the source to the second with `2dtexsimple` if it already is that target.
2. With the morphological opening on, `bloomerosion_3x3` loads the three-by-three neighbourhood (`ld` at integer coordinates, so past the edge reads zero) into the binary image, and `bloomdilation_3x3` gathers it around each texel and masks the bright pass; with it off, the plain copy moves the bright pass to the second target. The plain copy also writes the bright-pass copy.
3. `downscale2x2` box-downsamples half to quarter to eighth to sixteenth. Its four taps, from the destination's sample-offset table, sit half a source texel before and one and a half after the destination texel's centre, so a row's last texel reads one source texel past its footprint.
4. The blur. The default accumulated technique blurs the sixteenth level horizontally then vertically with `blur_1dgaussian_17sample` (nine bilinear taps over seventeen texels, one source texel per step through a constant), adds it into the eighth with the plain copy under additive blending, and repeats up to the half, then blurs the half; the maximum downsample power can stop short of the sixteenth. The separate technique blurs every level and sums them in `compositebloom`. With temporal smoothing on, `simpleblend` blends with the previous frame's result into the other temporal target.
5. The secondary bloom: `blur_1dsaw_17sample` blurs the eighth level three times each way, ending in its second target.

The composite (§3.1) reads the result as `BloomTexture` and the eighth level as `SecondaryBloomTexture`, both by screen UV through a bilinear sampler, and adds them, the second weighted by the lens dirt.

The sun halo is a per-screen sprite. [`SunHaloEffect::PreApply`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::SunHaloEffect##func-PreApply) runs only while the sun is below the effect's height threshold and in front of the camera. It projects a point along the sun direction through the render context's view and projection onto the scene texture's pixel size, masks a block of one of the post context's screen inputs around that pixel into its own small target (which input is unidentified), blurs the mask twice, draws the rings, and stores the centre. [`SunHaloEffect::Apply`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::SunHaloEffect##func-Apply) composites it additively there after anti-aliasing: one halo per screen, from the screen's single projection.

## 4. Buffers & how the frame comes together

Render targets created in `CreateRenderSetups`:

| Buffer | Name string | Format | Role |
|---|---|---|---|
| MainDepth | `"MainDepthBuffer"` | `SURFACEFORMAT_D32FS8` | scene depth and stencil (reverse-Z) |
| MainColor | `"MainColorBuffer"` | `SURFACEFORMAT_R11G11B10F` | HDR scene colour |
| GBuffer0 | `"GBuffer0"` | `SURFACEFORMAT_ABGR32` | albedo and material; sRGB and linear aliases |
| GBuffer1 | `"GBuffer1"` | `SURFACEFORMAT_A2R10G10B10` (unverified) | normals |
| GBuffer2 | `"GBuffer2"` | `SURFACEFORMAT_ABGR32` | material; sRGB alias |
| GBuffer3 | `"GBuffer3"` | `SURFACEFORMAT_ABGR32` | material |
| DownsampledDepth | `"DownsampledDepth"` | half-resolution `D32FS8` with a `D32F` alias | SSAO, SSR, soft particles |
| Velocity | `"motion_blur_velocity_buffer"` | `SURFACEFORMAT_ABGR32` | screen-space velocity, from `RP_Z_AND_VELOCITY_PASS` |
| BackBufferLinear | `"BackBufferLinear"` | `ABGR32` alias of the back buffer | the final LDR target, colour of `m_BackBufferRenderSetup` |

The pointers are on `GraphicsEngine` (`m_GBufferTexture[4]`, `m_VelocityBufferTexture`, `m_MainDepthTexture`, `m_MainColorBuffer`, `m_DownSampledDepthTexture`, `m_BackBufferLinear`). Created alongside, formats unverified: `reflection_proxy_water_plane_texture`, `reflection_proxy_depth_texture`, `reflection_proxy_normal_gloss_texture`, `ao_volume_texture`, and the five `VfxDepthCopy_%d` EffectInfo slots (§5.1).

The frame: the G-buffer fill (MainDepth, GBuffer0 to 3, velocity); lighting, SSR, GI, and the deferred lights into MainColor; environment, water, and transparency into MainColor; the post chain through the three temp slots, its composite writing the LDR image; the global filters; `PostDraw`, the UI, and debug; `EndDraw`, which submits, with no copy after, because the composite target `BackBufferLinear` aliases swapchain buffer 0 (§12); and `graphics_flip` at the next prologue.

### 4.1 The depth-reconstruction passes (a closed set)

Some passes recover position from depth rather than from vertex data, all the same way: they rebuild a clip-to-view inverse with [`CMatrix4f::PerspectiveFovInverse`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/types::math::Matrix4##func-PerspectiveFovInverse) and multiply by the render context's camera transform. Its seven call sites are every such pass:

| Pass | Notes |
|---|---|
| [`RenderBlockDeferredLighting::DrawClustered`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockDeferredLighting##func-DrawClustered) | the shaded lighting path |
| [`RenderBlockDeferredLighting::DrawPassThrough`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockDeferredLighting##func-DrawPassThrough) | wireframe only; [`Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockDeferredLighting##func-Draw) branches on `IsWireframeEnabled` |
| [`RenderBlockAtmosphericScattering::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockAtmosphericScattering##func-Draw) | the whole screen, sky included; ray-marches the sun cascade and aerial perspective |
| [`RenderBlockSSAO::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockSSAO##func-Draw) | temporal history advanced per invocation; the inverse feeds only the temporal reprojection |
| [`RenderBlockScreenSpaceReflection::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockScreenSpaceReflection##func-Draw) | ray-marches an earlier scene-colour capture |
| [`RenderBlockScreenSpaceSubSurfaceSkin::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockScreenSpaceSubSurfaceSkin##func-Draw) | twice, once per blur axis |
| [`DOFUtil::GetViewProjInverse`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::post_effects##func-GetViewProjInverse) | a post-effect utility |

The rebuild encodes only a symmetric frustum (the def has the matrix), so an asymmetric one needs the matrix substituted, not the inputs adjusted. `DrawClustered` and `AtmosphericScattering::Draw` both sample the sun cascade over their reconstructed positions, so shadow behaviour that depends on the reconstruction basis needs both changed together.

SSAO's occlusion does not use the inverse. Its generation draw (the `ssao` pixel shader, or `ssao_nvidia` on NVIDIA) reconstructs view-space position as `z * (uv * basis.xy + basis.zw)` from the packed linear depth, the basis built on the CPU from four terms of the render context's [`m_ProjectionF`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_ProjectionF): `(-2 / p00, -2 / p11, (1 - p02) / p00, (p12 + 1) / p11)`. The two shear terms are not among them, so an asymmetric projection reconstructs as the symmetric frustum of the same scale. Normals go to view space through the transpose of the inverse of [`m_View`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_View) without its translation. The taps sit on a spiral of isotropic screen-UV offsets scaled by half the inverse of [`CameraFOV`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-CameraFOV) over depth, at a depth mip chosen from the same radius; the spiral's rotation and a two-by-two quad parity come from `SV_Position`. The bilateral blur steps one target texel at a time through the screen-size constant, the temporal resolve reprojects through the render context's previous view-projection, and the composite multiplies the half-resolution result into the scene target's alpha with a bilinear upsample. The samplers clamp at the edge, except the temporal history lookup's border sampler.

### 4.2 Draw entry points, and the D3D11 slots they reach

Every render block submits through a `Graphics::` draw wrapper (the `graphics_engine::draw` module), each a thin cover over one `ID3D11DeviceContext` vtable slot. IDA's names for them are shuffled and their signatures misreported, so the defs bind them by body: the method called, whether it loops, and whether it takes the context mutex (the `NoMutex` variants' callers already hold it).

| Wrapper | `ID3D11DeviceContext` method | Vtable slot |
|---|---|---|
| [`Graphics::DrawIndexed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexed), [`DrawIndexedWithBoundState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedWithBoundState), [`DrawIndexedNoMutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedNoMutex) | `DrawIndexed` | 12 |
| [`Graphics::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-Draw) | `Draw` | 13 |
| [`Graphics::DrawIndexedInstanced`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedInstanced), [`DrawIndexedInstancedNoMutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedInstancedNoMutex) | `DrawIndexedInstanced` | 20 |
| [`Graphics::DrawInstanced`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawInstanced) | `DrawInstanced` | 21 |
| [`Graphics::DrawIndexedInstancedIndirect`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedInstancedIndirect), [`DrawIndexedInstancedIndirectNoMutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedInstancedIndirectNoMutex), [`DrawIndexedInstancedMultiIndirect`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawIndexedInstancedMultiIndirect) (once per draw in a loop) | `DrawIndexedInstancedIndirect` | 39 |
| [`Graphics::DrawInstancedIndirect`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-DrawInstancedIndirect) | `DrawInstancedIndirect` | 40 |

The indirect slots carry the GPU-driven geometry: [`RenderBlockTerrainPatch::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockTerrainPatch##func-Draw)'s near tessellating passes (`RP_TERRAINPATCH_BASEMESH_TESSELLATE_NEAR`, `RP_TERRAINPATCH_BASEMESH_NEAR`) through `DrawInstancedIndirect`, and [`RenderBlockFoliage::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockFoliage##func-Draw)'s dominant grass path through `DrawIndexedInstancedIndirectNoMutex`. Both blocks also use `DrawIndexed` for other passes. `Graphics::Draw` is mostly the fullscreen-pass entry, but some blocks draw world geometry non-indexed through it: `RP_ROAD_LAYERS` (`CRenderBlockSplineRoad`), `RP_DECALS` (the four `CRenderBlockDecal*` blocks), `RP_SKIDMARKS`, and `RP_WINDOW_DECALS`. Only the pass tells a fullscreen triangle from a decal box.

### 4.3 How shaders address the screen

Three idioms are in use; they agree only while one view fills a target the size of the display.

1. Absolute texel fetch, `ftoi SV_Position.xy` then `ld`: the deferred and forward G-buffer readers and the clustered light-grid lookup (`tile = ftoi(SV_Position.xy) >> 6`). A pixel shader's `SV_Position` is the absolute target coordinate, the viewport's `TopLeftX` and `TopLeftY` included, so this addresses the target.
2. Normalized by the inverse display size, `mul rN.xy, v0.xyxx, cb0[8].zzwz`, where `cb0[8].zw = (1/DisplayWidth, 1/DisplayHeight)` is staged into [`RenderEngine::m_FPGlobalConstData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##field-m_FPGlobalConstData) from `Graphics::SDeviceInfo`, not from the bound render setup or the viewport ([lighting-shadow-pipeline.md](lighting-shadow-pipeline.md) §5.3). `deferred_clusteredlighting_full`'s reflection and SSR lookups and the WaveWorks `nvwater*` family use it. This also addresses the target.
3. A CPU-staged projective UV: the legacy `Water*` and `WaterBox*` family (the lower water-quality settings) receives a world-to-screen-UV matrix on vertex `cb1` from its block type's `Setup`, the render context's view-projection post-multiplied by an NDC-to-UV bias; the vertex shader passes it as a projective `TEXCOORD1` and the pixel shader divides by `w`. That UV is normalized over the viewport, not the target. [`WaterHighEndRenderBlockType::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::WaterHighEndRenderBlockType##func-Setup) stages it at `cb1[1..4]` and [`WaterBoxRenderBlockType::Setup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::WaterBoxRenderBlockType##func-Setup) at `cb1[4..7]`. Exactly 9 of the 961 pixel shaders in `Shaders_F` bind `RefractionMap`, and they are this family.

## 5. Per-frame-once / single-use state (double-dispatch hazards)

What advances once per dispatch, and so twice if `HandleDrawThreadTask` or `Game::Draw` runs twice in a presented frame, and what advances once per real frame.

### 5.1 EffectInfo reflection-proxy history (slot machine)

[`GraphicsEngine::m_EffectInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_EffectInfo) is an array of [`EffectInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::EffectInfo) slots, each with an `m_FrameIndex` state byte, a depth texture (`m_DepthTexture`, a `VfxDepthCopy_%d`), and an `m_Transform` view-projection; [`m_EffectInfoIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_EffectInfoIndex) is the current slot. Between `DrawGBuffer` and `RenderEngine::Draw`, per dispatch:

- Walk the slots: remember a [`FRAME_INDEX_FREE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::EffectInfo::FRAME_INDEX_FREE) and a `FRAME_INDEX_READY` slot, write a `FRAME_INDEX_PICKED` slot's index into `m_EffectInfoIndex`, and advance `FRAME_INDEX_COPIED` to `FRAME_INDEX_READY`.
- Into a free slot, `CopySurfaceToTexture(slot.m_DepthTexture, m_ReflectionProxyDepthSurface)`, and mark it `FRAME_INDEX_COPIED`; promote a ready slot to `FRAME_INDEX_PICKED`.
- Copy the render camera's `m_ViewProjectionF` into the current slot's `m_Transform` and mark it `FRAME_INDEX_FREE`.

Nothing reads the stored depth or transform (see the [`EffectInfo::m_DepthTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::EffectInfo##field-m_DepthTexture) def). Run twice, every slot advances twice and the current slot is overwritten with the same frame's view-projection.

### 5.2 dt-driven accumulators in `ApplyWorldFilters` / `ApplyGlobalFilters`

In [`PostEffectsManager::ApplyWorldFilters(dt)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager##func-ApplyWorldFilters), `dt` reaches only [`ApplyWorldFadeFilter(dt)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager##func-ApplyWorldFadeFilter), the world fade accumulator. [`PostEffectsManager::ApplyGlobalFilters(dt)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::PostEffectsManager##func-ApplyGlobalFilters) steps the screen fade alpha by `(1/fade_duration)·dt`, clamped to [0, 1], advances a sun-direction and heat-haze accumulator, and calls the depth-of-field update hook; the fade alpha, its duration, and the accumulator are unbound fields of `PostEffectsManager`. All step per dispatch. Auto-exposure is not dt-driven (§5.4).

### 5.3 Shadow parity (`render_frame_counters.m_FrameIndex & 1`)

Parity advances only in the `Draw` prologue, so dispatches under one prologue read and write the same parity slot.

### 5.4 Exposure smoother / clock

`Clock::Update` ticks once per real frame in the prologue.

Auto-exposure is frame-counted, not dt-driven. `CToneMappingEffect::Update` runs once per real frame (from `CPostEffectsManager::UpdateRender`): it sets the target to `key / m_Histogram2.m_HistogramMidPoint`, key over the raw scene brightness, and adapts `m_ExposureBrightPoint`, the applied exposure, toward it. Two histograms are metered through `PopulateHistogram`, as per-bucket occlusion queries over a fixed small target, so a correct read totals its pixel count:

- [`m_Histogram`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::tone_mapping::ToneMappingEffect##field-m_Histogram), the exposure-weighted meter, run per dispatch by `CToneMappingEffect::Apply` through the `"LuminanceToDepthWithExposure"` shader, fed `m_ExposureBrightPoint` (a feedback loop).
- [`m_Histogram2`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::tone_mapping::ToneMappingEffect##field-m_Histogram2), the unweighted raw meter and the target's divisor, run per dispatch by `GenerateHistogramForFinalScene`, which calls `PopulateHistogram` with a fixed exposure of one.

Running `GenerateHistogramForFinalScene` twice in a frame corrupts `m_Histogram2`'s occlusion-query ring (the total reads several times the target's pixel count, with irregular buckets), its mid-point reads high, and the exposure darkens the frame. The exposure-weighted meter does not feed the divisor. The functions are in `graphics_engine/tone_mapping.pyxis`.

### 5.5 Screenshot countdown / profiler / end-draw callbacks

The screenshot countdown `m_FramesUntilScreenshot` decrements per dispatch (two branches). The GPU profiler's `EndFrame` fires per dispatch. The end-draw `sig::Signal` one-shot callbacks remove themselves after the first dispatch that fires them.

### 5.6 Draw-done completion event

`m_CPUDrawFinished = 1; SetEvent(m_CPUFinishedDrawingEvent)` runs at the tail of every dispatch, so with two dispatches the first one's signal can release `WaitForCPUDrawToFinish` before the second finishes. `m_CPUDrawStallDuration` and `m_DrawTime` are last-writer-wins.

### 5.7 Jitter phase / frame-counter ring (TAA)

The jitter comes from the two-phase table [`jittered_samples`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::jittered_samples) indexed by `render_frame_counters.m_FrameIndex & 1`, scaled by [`jitter_scale_factor`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::jitter_scale_factor) and divided by the target width and height. The phase and the three-slot constant-buffer ring advance per real frame in the prologue, so dispatches under one prologue share them; running the whole `GraphicsEngine::Draw` twice double-steps the counter, giving opposite jitter phases and mismatched constant-buffer slots. The velocity pass's `m_PreviousViewProjection` is snapshotted only in the sim-path `Camera::UpdateRender`, not per dispatch, and the T2X resolve keeps one history ping-pong pair on [`AntiAliasingEffect`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AntiAliasingEffect) (a pair of history textures, render setups, and indices, unbound), allocated in [`AntiAliasingEffect::CreateRenderTargetResources`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::post_effects::AntiAliasingEffect##func-CreateRenderTargetResources), whose `CreateRenderSetup` call is mislabelled `LockVolumeTexture` in the IDB. Which history target the resolve binds was not traced.

## 7. Present / swapchain

[`graphics_flip`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::graphics_engine##func-graphics_flip) takes the context critical section (§8), does GPU-timestamp bookkeeping over a small query ring, and presents with `device->m_SwapChain->Present(device->m_DeviceInfo.m_DisplayPresentationInterval, 0)` ([`Device::m_SwapChain`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Device##field-m_SwapChain), [`DeviceInfo::m_DisplayPresentationInterval`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::DeviceInfo##field-m_DisplayPresentationInterval), from `CGraphicsEngine::m_FlipInterval`; 0 is no vsync). `GraphicsEngine::Flip` wraps it in `AcquireThreadOwnership` and `ReleaseThreadOwnership`.

The swapchain is the BitBlt model, `DXGI_SWAP_EFFECT_DISCARD`, created by `IDXGIFactory::CreateSwapChain` with `Flags = 2` (`ALLOW_MODE_SWITCH`), `BufferUsage = 0x70` (render-target output and shader input), and `SampleDesc.Count = 1`. `BufferCount` is a bit field of the device create flags minus one, asserted at least two, so typically double-buffered. The engine wraps back buffer 0 as a `Graphics::HTexture_t` (RTV and SRV) returned by `GetDeviceSurface(BACK_BUFFER)`.

## 8. D3D11 context model

The master context, `m_RenderContext.m_Context`, is a `Context` (`Graphics::HContext_t`) around the D3D11 immediate context: `D3D11CreateDevice` writes it into [`Context::m_Context`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Context##field-m_Context), [`GetMasterContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-GetMasterContext) returns it, and `HandleDrawThreadTask` fetches it each dispatch. Deferred contexts exist (`CreateDeferredContext`), recorded and replayed through `Graphics::EndCommandBuffer` and `ApplyCommandBuffer` (`FinishCommandList`, `ExecuteCommandList`); `HandleDrawThreadTask` applies one early, then issues the G-buffer, lighting, post, and UI directly on the immediate context.

Every immediate-context operation is guarded by [`Context::m_Mutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Context##field-m_Mutex), a Win32 `CRITICAL_SECTION` created at device init and taken through `ThreadMutexLock`/`Unlock` (`Graphics::CScopedCriticalSection`): `graphics_flip`, `CopyTexture`, `BeginDraw`, and the viewport sets all take it, while `CRenderEngine::PostDraw` itself does not. Resource creation on the `ID3D11Device` is free-threaded (the driver reports concurrent creates) and needs no context lock.

## 9. Viewport / render resolution

`Graphics::SetRenderSetup` sets the viewport to the bound colour or depth target's own size (`{0, 0, target->m_Width, target->m_Height, 0, 1}`), so a pass's viewport is its target's size; `Graphics::SetViewport` sets one explicitly.

Not every pass draws at scene resolution. The low-resolution clouds, the low-resolution particles, and the volumetric spot-light cones render into a shared quarter-resolution target (half per axis) beside the downsampled depth, and their composes stretch the whole texture over the scene target with a baked UV and blend. The viewport mid-frame is therefore not always the scene's.

Every scene target in `CreateRenderSetups` is sized from its `SDeviceInfo` parameter's `m_DisplayWidth` and `m_DisplayHeight` (some at half), which `ApplyResize` fills from `device->m_DeviceInfo`. There is no dynamic resolution or render scale. `m_BackBufferLinear` is the exception: it is a format alias of swapchain buffer 0 and inherits the swapchain's size. Re-running `CreateRenderSetups` at another device size resizes the scene chain, with the viewports following, while `m_BackBufferLinear` and the two back-buffer render setups stay at the swapchain's size ([render-setups-reinit.md](render-setups-reinit.md) §4).

## 10. UI / HUD pipeline

[`GetIUIManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/ui::ui_manager##func-GetIUIManager) returns the one [`CUIManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager) (Scaleform GFx, vtable `??_7CUIManager@@6B@`). `HandleDrawThreadTask` calls its virtuals in two places (§1.4): before `PreDraw`, `RenderOffScreenTextures` (the in-world Scaleform screens) and `StartRender` (kicks off the UI render fragment); in the UI block, `RenderStaticBackGround` (the pause and menu background), `SyncRender` (waits for the fragment), and `Submit` (locks the master context and flushes the HAL). The update, capture, and render are in [ui-rendering.md](ui-rendering.md).

The HUD draws directly into `m_BackBufferLinear`: [`CUIManager::InitPlatformRT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-InitPlatformRT) binds Scaleform's `m_RenderBuffer` to its RTV with `m_MainDepthSurface` as the DSV, and `CUIManager::Render` sets that target and draws. The HUD lands on top of the final LDR image in the surface the composite wrote, with no separate target and no copy after. There is one UI block per dispatch.

`CUIManager::Convert3DCoords` projects a world point to a HUD position (`world·vp`, divide by `|w|`, aspect-correct, map NDC to the view size, return `w > 0`), with the view-projection as a parameter; `CUIManager::Get2DInfo` and `CUIManager::ClampToScreen` place and edge-clamp markers. The markers are in [ui-rendering.md](ui-rendering.md#points-of-interest).

## 11. Render-block / draw-list lifecycle across dispatches

Drawing a pass's list is non-destructive; the list rotation runs in the `Draw` prologue.

- `RenderEngine::m_RenderPasses` is a fixed array of per-pass lists created at init. Each `CRenderPass` owns a double-buffered `m_Lists[2]` of [`CRBILists`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RBILists), followed by [`RenderPass::m_CurrentAddList`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_CurrentAddList) and [`m_CurrentDrawList`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_CurrentDrawList).
- A `CRBILists` holds `m_List` (an array of [`RenderInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderInstance)), `m_ListSize` (a u16 capacity), and `m_NumElements` (a volatile u32). [`CRBILists::Add`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RBILists##func-Add) does `InterlockedExchangeAdd(m_NumElements, 1)` and, when full, spills to a global overflow list ([`render_block_overflow_count`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::render_block_overflow_count) and its entries; its capacity and entry size are unconfirmed).
- Population: sim systems append to `m_CurrentAddList` through `CRBILists::Add` from worker jobs, once per frame. [`CRenderPass::SetupRenderFrameData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SetupRenderFrameData) is one such batch appender; it does not swap.
- Rotation: the prologue calls what IDA labels `CKeep1000Frames::CKeep1000Frames` (the inlined rotation merged with a profiling constructor). It toggles the global parity [`current_add_buffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::current_add_buffer) (`parity = (parity-1)&1`); calls every pass's virtual [`CRenderPass::SaveRenderFrameData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SaveRenderFrameData) (through a sub mislabelled `std::map::operator[]`), which sets `m_CurrentAddList = &m_Lists[parity]` and `m_CurrentDrawList = &m_Lists[(parity-1)&1]`, zeroes the new add list's `m_NumElements`, and copies an external camera into the pass's render camera; and flushes the overflow list back into the lists.
- Drawing: the virtual [`CRenderPass::DoDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-DoDraw) draws `min(m_ListSize, m_NumElements)` entries of `m_CurrentDrawList` through a local cursor and never writes the list, so a list can be drawn any number of times.
- `CRenderEngine::EraseAllDeletedRenderBlocks` works on a separate deletion list and does not touch the draw lists.

Running the whole `Draw` twice therefore rotates twice: the second rotation toggles the parity back, points every draw list at the buffer the first rotation just zeroed, and zeroes the populated one, so the second dispatch draws no list-driven geometry (its fullscreen and compute work still runs). Restoring `current_add_buffer` before the second `Draw` makes its rotation select and zero the same lists as the first, so it draws the same populated lists; running only the dispatch body twice under one prologue rotates once. Neither needs the list contents saved.

### 11.1 The render-pass sort and depth buckets

Each [`RenderInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderInstance) (`SRenderInstance`) holds a sort ID, the render block, its instance info, the render-block type, and a depth key. Once per rotation, [`RenderPass::SortList`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##func-SortList) (labelled `CRigidObject::Destruct` in the IDB):

1. Recomputes each entry's keys through [`RenderBlock::GetSortID`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderBlock##vfunc-GetSortID) and [`RenderBlock::GetSqDistanceToCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderBlock##vfunc-GetSqDistanceToCamera), the squared distance from the instance's buffered world translation to the sort camera; only `CRenderBlockWindow` and the particle blocks override the distance.
2. `std::sort`s by `(m_Depth, m_RenderBlockType, m_SortID)` as [`RenderPass::m_SortMethod`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_SortMethod) says: by sort ID with no depth key, back to front or front to back by raw distance, or front to back by bucket.
3. Latches [`RenderPassSortState::m_Sorted`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPassSortState##flag-m_Sorted) in [`RenderPass::m_SortStateFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_SortStateFlags) under [`RenderPass::m_SortLock`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_SortLock); `SaveRenderFrameData` clears it, so a list sorts at most once per rotation and every draw of it that frame sees one order.

Two callers reach it: the render thread's sort task, [`RenderEngine::SortRenderPasses`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-SortRenderPasses) (`m_SortTaskProxy`, created fresh per dispatch and awaited before `EndDraw`), which sorts by the pass's external camera when it has one and the render camera otherwise, and a lazy call at the top of `DoDraw`. The distance is the instance centre's, not the nearest point of its bounds ([`RenderBlock::GetBoundingBox`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderBlock##vfunc-GetBoundingBox)), so a large object with a far centre sorts as far.

`m_SortMethod` defaults to `Auto`: front to back by bucket, or back to front for alpha-blended passes. Bucketing quantizes the squared distance through [`RenderPass::m_DepthSqTable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_DepthSqTable), a fixed-capacity table of ascending squared boundaries with its count in [`RenderPass::m_NumDepthBuckets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass##field-m_NumDepthBuckets), storing the bucket index in `m_Depth`. With one bucket, the constructor's default, no depth key is computed and the sort is by type, then sort ID. Adding a boundary (the inlined `AddDepthBucket`: append, keep sorted, raise the count) makes the sorted list one contiguous, type-batched run per bucket. Back-to-front passes keep raw distances, so a distance splits them too, in reverse. The only stock pass with more than one bucket is [`RenderPassId::RP_Z_AND_VELOCITY_PASS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderPassId##variant-RP_Z_AND_VELOCITY_PASS), with boundaries [`RenderPass::Z_AND_VELOCITY_DEPTH_SQ_NEAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass::Z_AND_VELOCITY_DEPTH_SQ_NEAR) and [`RenderPass::Z_AND_VELOCITY_DEPTH_SQ_FAR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_pass::RenderPass::Z_AND_VELOCITY_DEPTH_SQ_FAR) registered inline in [`RenderEngine::InitializeSystem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-InitializeSystem). Some content is near or far by block type rather than distance: the distant terrain, distant trees, and occluders ([terrain.md](terrain.md#which-blocks-draw-the-visible-terrain)).

## 12. There is no final resolve

The `EndDraw` call in `HandleDrawThreadTask` (§1.4) is not the MSAA resolve into the presentable surface its IDB name suggests. Two IDB symbols swap a pair of `Graphics::` functions: the function labelled `Graphics::CopySurfaceToTexture` is [`Graphics::EndDraw(ctx)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-EndDraw), which reads only the context, takes the context critical section, and submits the frame's two prebuilt command objects from the command-slot ring; and the function labelled `NGraphicsEngine::CGPUProfiler::BeginScope` is [`Graphics::CopySurfaceToTexture(ctx, dst, src)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-CopySurfaceToTexture), a whole-resource `CopyResource`, whose one call in `HandleDrawThreadTask` is the EffectInfo depth copy (§5.1).

Nothing copies to the back buffer because `m_BackBufferLinear` is a format alias of swapchain buffer 0 (`CreateSurfaceAlias` copies the source handle, `AddRef`s the same `ID3D11Resource`, and builds fresh views), so the composite bound through `m_BackBufferRenderSetup` writes straight into the presentable surface, which `Graphics::Flip` presents. The composite's output is read from `m_BackBufferLinear` after the post chain.

## Open questions

1. The formats of GBuffer1 to 3 beyond GBuffer0's `ABGR32` (GBuffer1 as `A2R10G10B10` is unverified), of the reflection-proxy targets, and the `VfxDepthCopy_%d` count.
2. Where the exposure adaptation's smoothing lives and its decay; inferred to be in the tone-mapping effect, not traced.
3. The consumers of `render_frame_index` and `previous_render_context_frame_index`, presumed triple-buffered per-frame resources.
4. The TAA history target bindings (§5.7).
5. Which draw entry point WaveWorks uses (§4.2). [`CNvWaterHighEndRenderBlock::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::NvWaterHighEndRenderBlock##func-Draw) hands a raw `ID3D11DeviceContext` to `GFSDK_WaveWorks_Quadtree_DrawD3D11`, an import from `gfsdk_waveworks.win64.dll`, so the slot is not visible in the IDB; a per-slot counter on the water passes or a frame capture settles it.
6. What `GFSDK_WaveWorks_Savestate_RestoreD3D11`, which restores rasterizer state around the water draws on the engine's context, puts back, and whether it includes the viewport.
7. Whether `UVScale` is ever non-unity. The fullscreen reconstruction vertex shaders build their G-buffer UV as `o1 = (v0 · 0.5 + 0.5) · cb1[5].xy`, from [`RenderContext::m_UVScale`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_UVScale), which `SetupRenderContext` copies each pass from [`GraphicsEngine::m_UVScale`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_UVScale); its writer was not traced.
8. The literal per-object `world.row3 -= CameraPosition` in the model render block (§2.4) was not line-verified; the camera-relative scheme rests on `SetRenderContextCamera` storing a translation-free offset view-projection beside a separate `CameraPosition`.
