# Runtime re-initialisation of the render setups

Every render target is sized from the device's display size through `GraphicsEngine::CreateRenderSetups`; this documents who re-runs that function at runtime, what state it assumes, what it tears down and rebuilds, how far the window swapchain is separable from the render-target re-init, and what a size change has to touch to leave the pipeline coherent.

The pyxis defs hold every address and offset named here. The engine calls the graphics-device backend through the `Graphics::` free functions; the engine class is the [`GraphicsEngine`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine) singleton (`CGraphicsEngine` in the game).

## 1. Who calls `CreateRenderSetups`, and when

[`GraphicsEngine::CreateRenderSetups(this, const SDeviceInfo *device_info)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-CreateRenderSetups) has exactly two callers:

1. **[`GraphicsEngine::InitializeSystem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-InitializeSystem)** — the one-time device + render-setup bring-up at startup. Not a runtime path.
2. **[`GraphicsEngine::ApplyResize(this, u32 width, u32 height)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-ApplyResize)** — **the runtime resize path**. This is what a windowed resize or a graphics-menu resolution change ultimately drives.

### 1.1 How a resize reaches `ApplyResize`

There is no `SetDisplayMode`; the resolution/resize machinery is a small state machine on the engine, serviced once per frame:

- [`GraphicsEngine::ResizeBuffers(this, width, height)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-ResizeBuffers) (the engine method, distinct from the backend `Graphics::ResizeBuffers`) is the request entry. If [`m_SynchronousResize`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_SynchronousResize) `&&` [`m_HasBeenInitialized`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_HasBeenInitialized) it calls `ApplyResize` inline; otherwise it stashes [`m_WindowWidth`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_WindowWidth)/[`m_WindowHeight`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_WindowHeight) and sets [`m_HasNewWindowSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_HasNewWindowSettings) `= 1` to defer.
- [`GraphicsEngine::HandleModeChange(this)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleModeChange) is the per-frame servicing point. When [`m_DisplayModeChangeState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DisplayModeChangeState) is idle and `m_HasNewWindowSettings` is set, it calls `ApplyResize(m_WindowWidth, m_WindowHeight)`; when a mode change is pending it calls [`ApplyMode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-ApplyMode) (the fullscreen/adapter path, which recreates the device via `Graphics::ResetDevice` rather than resizing buffers).
- **`HandleModeChange` is called from [`GraphicsEngine::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-Draw)**, in the Draw prologue — after the previous frame's render dispatch has been drained (`CpuFragmentWaitUntilSignalIsNonZero` on [`m_DrawThreadWorkSignal`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DrawThreadWorkSignal)) and after `Flip`, but **before** `Clock::Update`, `HandBackBuffers`, `CalculateConstantBufferIndices`, and `DispatchDraw`. So the resize applies on the main/sim thread, at the frame boundary, with no draw in flight and this frame's dispatch not yet issued.

`ApplyMode` also sets `m_SynchronousResize = 1` around its `ResetDevice` call so any resize it induces resolves inline; the resolution menu path funnels through here.

## 2. What `ApplyResize` does, and the state it assumes

The decompiled body of `ApplyResize`, in order:

1. Clamp `width`/`height` to a small minimum.
2. [`DestroyRenderSetups(this)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-DestroyRenderSetups) — tear down every scene render target and render setup (see §3).
3. If a UI manager exists, [`IUIManager::PrepareForReset`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-PrepareForReset) (a virtual call) — Scaleform drops its render-buffer references before the targets go away.
4. **[`Graphics::ResizeBuffers(m_Device, width, height)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-ResizeBuffers)** — **the DXGI swapchain resize** (see §4).
5. [`Graphics::GetDeviceInfo(m_Device, &device_info)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-GetDeviceInfo) — copy the device's now-updated `SDeviceInfo` (a plain `memcpy` of [`Device::m_DeviceInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Device##field-m_DeviceInfo), one [`DeviceInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::DeviceInfo)) onto the stack.
6. **`CreateRenderSetups(this, &device_info)`** — rebuild all render targets at the size in `device_info`.
7. Walk `m_RegisteredCallbacksVector` and invoke each registered resize callback with `{width, height}`. These are the per-subsystem RT re-allocations (post-effects, SSAO, motion blur, anti-aliasing, camera manager aspect, UI, etc.); several of them call `Graphics::GetDeviceInfo` themselves to size their own targets.
8. `GetMasterContext` + `SetRenderSetup(ctx, 0, 0)` then `SetRenderSetup(ctx, m_BackBufferRenderSetup, 0)` — unbind, then bind the rebuilt back-buffer setup.
9. If a UI manager exists, [`IUIManager::RestoreAfterReset`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-RestoreAfterReset) (a virtual call).
10. Scalar fixups, all trivially replicable by hand:
    - [`CameraManager::m_AspectRatio`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera_manager::CameraManager##field-m_AspectRatio) `= (float)width / (float)height`.
    - The graphics params' [`m_Width`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsParams##field-m_Width)/[`m_Height`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsParams##field-m_Height) (what `get_graphics_params()` returns) `= width`/`height`.
    - [`GraphicsEngine::m_DeviceDisplayWidth`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DeviceDisplayWidth)/[`m_DeviceDisplayHeight`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DeviceDisplayHeight) = the *device info's* width/height (also written by `CreateRenderSetups`).
    - [`GraphicsEngine::m_HasNewValidDisplayMode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_HasNewValidDisplayMode) `= 1`.
    - [`GraphicsEngine::m_AppliedWidth`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_AppliedWidth)/[`m_AppliedHeight`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_AppliedHeight) = `width`/`height`.
    - [`GraphicsEngine::m_TitleSafeWidth`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_TitleSafeWidth)/[`m_TitleSafeHeight`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_TitleSafeHeight) = the title-safe area, a fixed fraction of the width and height with a small floor.

**State assumptions.** The path assumes the immediate context is idle and no render dispatch is in flight: `DestroyRenderSetups` begins with `SetRenderSetup(masterCtx, 0, 0)` (which unbinds the OM targets), `Graphics::ResizeBuffers` calls `OMSetRenderTargets(0, …)` and releases the back buffer's RTV/SRV/texture, and both then free/recreate GPU resources on the device. Because the only caller is `HandleModeChange` inside the `Draw` prologue, that idle condition is guaranteed by construction: the previous dispatch was drained and this frame's `DispatchDraw` has not run. No explicit mutex is taken around `ApplyResize` itself; the resource operations that need the immediate-context critical section ([`Context::m_Mutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Context##field-m_Mutex)) take it internally through `GetMasterContext`/`SetRenderSetup`. Running `ApplyResize` off this boundary (e.g. mid-dispatch, or on the render worker) would race the draw thread against the teardown and is unsafe.

## 3. What `DestroyRenderSetups` tears down

It first unbinds the active setup (`SetRenderSetup(masterCtx, 0, 0)`), then destroys, in bulk:

- Every engine render setup: `m_GBufferRenderSetup`, `m_BackBufferRenderSetup`, `m_PostEffectRenderSetup`, `m_RenderSetupZ`, `m_RenderSetupZAndVelocity`, the debug setups, the AO volume setup, the three reflection-proxy setups, and the two downsampled-depth setups.
- Every engine-owned scene texture and surface: MainDepth, MainColor, all four GBuffers, the velocity buffer (+ its sRGB alias), DownsampledDepth (+ its alias/second copy), the reflection-proxy water-plane/depth/normal-gloss textures, the AO volume, and the five `VfxDepthCopy_%d` slots on [`GraphicsEngine::m_EffectInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_EffectInfo).
- The **`BackBufferLinear`** *alias* ([`GraphicsEngine::m_BackBufferLinear`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_BackBufferLinear)).

**What it does not touch:** the DXGI swapchain and its back-buffer surface ([`Device::m_BackBuffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Device##field-m_BackBuffer)) are device-owned; only the `BackBufferLinear` alias of that surface is destroyed here. The back buffer's *own* RTV and SRV are likewise untouched — only `Graphics::ResizeBuffers` releases and recreates those, so a `CreateRenderSetups` run on its own cannot corrupt them. Everything the teardown does release is nulled after, so a following `CreateRenderSetups` re-creates cleanly.

**Stale views and pools.** `DestroyRenderSetups` only frees what the engine holds directly. Render targets owned by the *passes* — the post-effect fullscreen temp textures and their render setups (`m_FullscreenSrgbTempTexture[3]` / `m_FullscreenLinearTempTexture[3]` / `m_RenderSetups[3]`), the SSAO pass targets, the SSR pass, the anti-aliasing history ping-pong, and other per-pass pools — are **not** freed here. They are re-allocated by the **registered resize callbacks** in step 7. A `CreateRenderSetups` run **without** those callbacks therefore leaves every pass-owned pool at its old dimensions, and the pipeline is mismatched (for example, a resized MainColor sampled into an old-size tonemap temp). Cross-frame cached SRVs are otherwise not a hazard at the `Draw`-prologue boundary, because the render context rebinds all scene SRVs each dispatch.

## 4. Swapchain vs. render-target re-init: are they separable?

**Yes. The render-target sizing is separable from the swapchain.**

`CreateRenderSetups` sizes every *scene* render target from the **`device_info` parameter** (`device_info->m_DisplayWidth`/`m_DisplayHeight`), not from the device object directly — confirmed in the decompile (e.g. `refl_tex_params.m_Width = device_info->m_DisplayWidth` for MainDepth/MainColor/GBuffers/reflection proxies/AO, `>>1` for the half-res depth). So `CreateRenderSetups(engine, &customInfo)` with a custom `SDeviceInfo` builds all scene RTs at the size that `customInfo` carries, **without touching the swapchain**.

`Graphics::ResizeBuffers` is the part that touches the swapchain, and it is a distinct call in `ApplyResize` (step 4). It:

- calls `OMSetRenderTargets(0, …)` and releases the back buffer's RTV/SRV/texture,
- calls `IDXGISwapChain::ResizeBuffers(0, width, height, m_BackBufferFormat, 2)`,
- re-acquires buffer 0 and recreates its RTV/SRV,
- writes the width, height, and ratio in `device->m_DeviceInfo` and sets [`device->m_WasResized`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Device##field-m_WasResized).

Skipping this call leaves the game window and its swapchain untouched. **However, the separation is not total**: at its tail, `CreateRenderSetups` calls [`Graphics::GetDeviceSurface(device, BACK_BUFFER)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-GetDeviceSurface) (which returns `device->m_BackBuffer`) and builds **three** render setups against the *live* swapchain surface, independent of `device_info`:

- `m_BackBufferLinear` = a `SURFACEFORMAT_ABGR32` surface **alias** of the real back buffer,
- `m_PostEffectRenderSetup` = colour → the real back-buffer surface (no depth),
- `m_BackBufferRenderSetup` = colour → `m_BackBufferLinear`, **depth → `m_MainDepthSurface`**, and this setup is stored as `m_RenderContext.m_RenderSetup` (the final composite / HUD target).

So when `CreateRenderSetups` runs with a `device_info` whose size differs from the swapchain's, the scene RTs (MainColor/MainDepth/GBuffers/…, and the pass pools via the callbacks) take the `device_info` size, while `m_BackBufferLinear` and the two back-buffer setups stay at swapchain size. That is **not** a benign mismatch: `m_BackBufferRenderSetup` binds `m_BackBufferLinear` (swapchain size) as colour together with `m_MainDepthSurface` (`device_info` size) as depth, and D3D11 requires the RTV and DSV to share dimensions — the final composite/UI pass would bind a mismatched RTV+DSV pair.

So the separation splits cleanly in two: the scene half is fully separable and viewport-clean (§5), while the final-composite half (`BackBufferLinear` and the two back-buffer setups) is entangled with the live back buffer through `GetDeviceSurface(BACK_BUFFER)`. Freeing that half means replacing those three objects after `CreateRenderSetups` has built them, and the engine has no function that does this. Nothing has to be suppressed alongside it: there is no final resolve into the back buffer, because the composite writes straight into the alias ([`rendering.md` §12, "Per-eye output routing"](rendering.md#12-per-eye-output-routing)).

## 5. Viewports and other size-derived state

**Viewports follow automatically.** `Graphics::SetRenderSetup` sets the viewport to the bound target's size; see [`rendering.md` §9, "Viewport / render resolution"](rendering.md#9-viewport--render-resolution). Re-running `CreateRenderSetups` at a new size re-creates the RTs at that size and the per-pass viewports track them with no per-pass viewport patching.

**What is keyed to display size and would NOT follow from `CreateRenderSetups` alone:**

- **Pass-owned RT pools** — re-sized by the registered resize callbacks, not by `CreateRenderSetups` (§3). Must run the callbacks (or drive the whole `ApplyResize`).
- **Camera aspect ratio** — `CameraManager::m_AspectRatio` is set from `width/height` in `ApplyResize` step 10, and it feeds the projection built by [`Camera::RecalcProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-RecalcProjection). It is display-size-derived and does not follow from `CreateRenderSetups`.
- **TAA jitter scale** — the jitter offset is divided by RT width/height at use, so it tracks the new size automatically once the RTs are re-sized.
- **UI / movie-space and world-to-screen mappings** — the HUD/Scaleform view size (`CUIManager::m_ViewWidth`/`m_ViewHeight`, used by `Convert3DCoords`/`Get2DInfo`) is refreshed through `IUIManager::PrepareForReset`/`RestoreAfterReset` and [`UIManager::UpdateCachedValues`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-UpdateCachedValues) (which itself calls `GetDeviceInfo`), i.e. via the resize-callback/UI-reset path, not `CreateRenderSetups`. The HUD is composited into `m_BackBufferLinear`, so its target size is whatever that surface ends up being (§4). Aspect-ratio-derived HUD constants come from the same UI-reset path.

## 6. Driving the re-init at a changed size

The frame position is the whole constraint: `ApplyResize` must run where `HandleModeChange` runs it, on the game thread in the `Draw` prologue, with the previous dispatch drained and this frame's not yet dispatched (§2). The engine already offers a way to ask for exactly that — write `m_WindowWidth`, `m_WindowHeight`, and `m_HasNewWindowSettings`, and the next prologue services the request — so nothing has to call `ApplyResize` directly except a caller that can guarantee the drain itself.

Whichever way it is driven, the resize is expensive: `DestroyRenderSetups` plus `CreateRenderSetups` free and re-allocate a large set of GPU resources, so it belongs on an actual size change, not on a frame boundary. And it must be the *whole* walk, not just `CreateRenderSetups`: the pass-owned pools follow only through the registered resize callbacks in step 7, and the Scaleform view size only through the UI reset in steps 3 and 9 (§3, §5).

## 7. Open unknowns

- **Registered-callback set is not enumerated.** The pass pools follow the resize only through `m_RegisteredCallbacksVector`, and the exact membership was never walked. Reusing `ApplyResize` sidesteps needing to know it.
- **`ApplyMode` / fullscreen path** recreates the device via `Graphics::ResetDevice` rather than resizing buffers; it was not fully traced.
- **`m_DisplayModeChangeState` value semantics** are only partially confirmed (idle vs. mode-change pending); it is recorded as a `u32` with the observed behaviour rather than a fully enumerated enum.
