# Runtime re-initialisation of the render setups

Every render target is sized from the device's display size by `GraphicsEngine::CreateRenderSetups`. This page covers who re-runs it at runtime, what it assumes, tears down, and rebuilds, how far the swapchain is separable from it, and what a size change must touch. The engine class is the [`GraphicsEngine`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine) singleton (`CGraphicsEngine`), calling the device backend through the `Graphics::` free functions.

## 1. Who calls `CreateRenderSetups`, and when

[`GraphicsEngine::CreateRenderSetups(this, const SDeviceInfo *device_info)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-CreateRenderSetups) has exactly two callers:

1. **[`GraphicsEngine::InitializeSystem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-InitializeSystem)** — the one-time bring-up at startup.
2. **[`GraphicsEngine::ApplyResize(this, u32 width, u32 height)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-ApplyResize)** — the runtime resize path, which a windowed resize or a graphics-menu resolution change drives.

### 1.1 How a resize reaches `ApplyResize`

There is no `SetDisplayMode`; resizing is a small state machine on the engine, serviced once per frame:

- [`GraphicsEngine::ResizeBuffers(this, width, height)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-ResizeBuffers) (distinct from the backend `Graphics::ResizeBuffers`) is the request. If [`m_SynchronousResize`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_SynchronousResize) `&&` [`m_HasBeenInitialized`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_HasBeenInitialized) it calls `ApplyResize` inline; otherwise it stashes [`m_WindowWidth`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_WindowWidth)/[`m_WindowHeight`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_WindowHeight) and sets [`m_HasNewWindowSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_HasNewWindowSettings).
- [`GraphicsEngine::HandleModeChange(this)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleModeChange) services it. When [`m_DisplayModeChangeState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DisplayModeChangeState) is idle and `m_HasNewWindowSettings` is set, it calls `ApplyResize(m_WindowWidth, m_WindowHeight)`; when a mode change is pending it calls [`ApplyMode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-ApplyMode) (the fullscreen/adapter path, which recreates the device via `Graphics::ResetDevice` rather than resizing buffers).
- `HandleModeChange` runs in the [`GraphicsEngine::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-Draw) prologue, after the previous dispatch has drained (`CpuFragmentWaitUntilSignalIsNonZero` on [`m_DrawThreadWorkSignal`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DrawThreadWorkSignal)) and after `Flip`, but before `Clock::Update`, `HandBackBuffers`, `CalculateConstantBufferIndices`, and `DispatchDraw` ([rendering §1.3](rendering.md#13-prologue-of-graphicsenginedraw--per-real-frame-once-state)). The resize thus applies on the main thread at the frame boundary, with no draw in flight.

`ApplyMode`, which the resolution menu funnels through, sets `m_SynchronousResize = 1` around its `ResetDevice` call so any resize it induces resolves inline.

## 2. What `ApplyResize` does, and the state it assumes

`ApplyResize`, in order:

1. Clamp `width`/`height` to a small minimum.
2. [`DestroyRenderSetups(this)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-DestroyRenderSetups) — tears down the scene render targets and setups (§3).
3. If a UI manager exists, [`IUIManager::PrepareForReset`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-PrepareForReset) (virtual), so Scaleform drops its render-buffer references.
4. **[`Graphics::ResizeBuffers(m_Device, width, height)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-ResizeBuffers)** — the DXGI swapchain resize (§4).
5. [`Graphics::GetDeviceInfo(m_Device, &device_info)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-GetDeviceInfo) — copies the device's updated `SDeviceInfo` (a `memcpy` of [`Device::m_DeviceInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Device##field-m_DeviceInfo), one [`DeviceInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::DeviceInfo)) onto the stack.
6. `CreateRenderSetups(this, &device_info)` — rebuilds the render targets at that size.
7. Invokes each resize callback in `m_RegisteredCallbacksVector` with `{width, height}`: the per-subsystem target re-allocations (post effects, SSAO, motion blur, anti-aliasing, the camera manager's aspect, the UI, and others), several of which size their targets through `Graphics::GetDeviceInfo` themselves.
8. `SetRenderSetup(ctx, 0, 0)` then `SetRenderSetup(ctx, m_BackBufferRenderSetup, 0)` on the master context.
9. If a UI manager exists, [`IUIManager::RestoreAfterReset`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##vfunc-RestoreAfterReset) (virtual).
10. Scalar fixups:
    - [`CameraManager::m_AspectRatio`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera_manager::CameraManager##field-m_AspectRatio) `= (float)width / (float)height`.
    - The graphics params' [`m_Width`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsParams##field-m_Width)/[`m_Height`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsParams##field-m_Height) (what `get_graphics_params()` returns) `= width`/`height`.
    - [`GraphicsEngine::m_DeviceDisplayWidth`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DeviceDisplayWidth)/[`m_DeviceDisplayHeight`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_DeviceDisplayHeight) = the *device info's* width/height (also written by `CreateRenderSetups`).
    - [`GraphicsEngine::m_HasNewValidDisplayMode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_HasNewValidDisplayMode) `= 1`.
    - [`GraphicsEngine::m_AppliedWidth`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_AppliedWidth)/[`m_AppliedHeight`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_AppliedHeight) = `width`/`height`.
    - [`GraphicsEngine::m_TitleSafeWidth`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_TitleSafeWidth)/[`m_TitleSafeHeight`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_TitleSafeHeight) = the title-safe area, a fixed fraction of the width and height with a small floor.

The path assumes an idle immediate context with no dispatch in flight: `DestroyRenderSetups` starts by unbinding the output targets, `Graphics::ResizeBuffers` calls `OMSetRenderTargets(0, …)` and releases the back buffer's views and texture, and both free and recreate device resources. Its only runtime caller, `HandleModeChange` in the prologue, guarantees that. `ApplyResize` takes no mutex itself; the operations that need the immediate-context critical section ([`Context::m_Mutex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Context##field-m_Mutex)) take it inside `GetMasterContext` and `SetRenderSetup`. Run anywhere else (mid-dispatch, or on the draw worker), it races the draw thread.

## 3. What `DestroyRenderSetups` tears down

It unbinds the active setup (`SetRenderSetup(masterCtx, 0, 0)`), then destroys:

- Every engine render setup: `m_GBufferRenderSetup`, `m_BackBufferRenderSetup`, `m_PostEffectRenderSetup`, `m_RenderSetupZ`, `m_RenderSetupZAndVelocity`, the debug setups, the AO volume setup, the three reflection-proxy setups, and the two downsampled-depth setups.
- Every engine-owned scene texture and surface: MainDepth, MainColor, the four G-buffers, the velocity buffer and its sRGB alias, DownsampledDepth and its second copy, the reflection-proxy water-plane/depth/normal-gloss textures, the AO volume, and the five `VfxDepthCopy_%d` slots on [`GraphicsEngine::m_EffectInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_EffectInfo).
- The `BackBufferLinear` alias ([`GraphicsEngine::m_BackBufferLinear`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_BackBufferLinear)).

It leaves the device-owned swapchain, its back-buffer surface ([`Device::m_BackBuffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Device##field-m_BackBuffer)), and that surface's own RTV and SRV alone; only `Graphics::ResizeBuffers` recreates those. Everything it releases is nulled, so a following `CreateRenderSetups` re-creates cleanly.

Pass-owned targets (the post-effect temp textures and their setups, `m_FullscreenSrgbTempTexture[3]`, `m_FullscreenLinearTempTexture[3]`, and `m_RenderSetups[3]`; the SSAO and SSR targets; the anti-aliasing history pair; and other pools) are not freed here but re-allocated by the resize callbacks of step 7. `CreateRenderSetups` without them leaves every pass pool at its old size (a resized MainColor sampled into an old-size tonemap temp, for example). Cached SRVs are not a hazard across the prologue boundary, because the render context rebinds every scene SRV each dispatch.

## 4. Swapchain vs. render-target re-init: are they separable?

Mostly. `CreateRenderSetups` sizes every scene target from its `device_info` parameter's `m_DisplayWidth` and `m_DisplayHeight`, not from the device (`refl_tex_params.m_Width = device_info->m_DisplayWidth` for MainDepth, MainColor, the G-buffers, the reflection proxies, and the AO volume, shifted right for the half-resolution depth), so a custom `SDeviceInfo` builds the scene targets at its size without touching the swapchain. The swapchain is touched only by `Graphics::ResizeBuffers` (step 4), which:

- calls `OMSetRenderTargets(0, …)` and releases the back buffer's RTV/SRV/texture,
- calls `IDXGISwapChain::ResizeBuffers(0, width, height, m_BackBufferFormat, 2)`,
- re-acquires buffer 0 and recreates its RTV/SRV,
- writes the width, height, and ratio in `device->m_DeviceInfo` and sets [`device->m_WasResized`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Device##field-m_WasResized).

The separation is not total: at its tail, `CreateRenderSetups` calls [`Graphics::GetDeviceSurface(device, BACK_BUFFER)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-GetDeviceSurface) (which returns `device->m_BackBuffer`) and builds three objects against the live swapchain surface, whatever `device_info` says:

- `m_BackBufferLinear`, a `SURFACEFORMAT_ABGR32` alias of the back buffer;
- `m_PostEffectRenderSetup`, colour on the back-buffer surface, no depth;
- `m_BackBufferRenderSetup`, colour on `m_BackBufferLinear` and depth on `m_MainDepthSurface`, stored as `m_RenderContext.m_RenderSetup` (the final composite and HUD target).

With a `device_info` size other than the swapchain's, the scene targets (and the pass pools, through the callbacks) take that size while these three stay at the swapchain's. `m_BackBufferRenderSetup` then pairs a swapchain-size RTV with a `device_info`-size DSV, which D3D11 rejects, since the two must share dimensions. The scene half is separable and viewport-clean (§5); the final-composite half is tied to the live back buffer, and the engine has no function to replace those three objects after `CreateRenderSetups` builds them. There is no final resolve into the back buffer to go with them; the composite writes straight into the alias ([rendering §12](rendering.md#12-there-is-no-final-resolve)).

## 5. Viewports and other size-derived state

Viewports follow the bound target's size ([rendering §9](rendering.md#9-viewport--render-resolution)), so re-created targets need no viewport patching. The jitter offset is divided by the target size at use, so it follows too. What does not follow from `CreateRenderSetups` alone:

- The pass-owned pools, re-sized only by the resize callbacks (§3).
- The camera aspect, `CameraManager::m_AspectRatio`, set in step 10 and feeding [`Camera::RecalcProjection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-RecalcProjection).
- The UI's view size (`CUIManager::m_ViewWidth` and `m_ViewHeight`, used by `Convert3DCoords` and `Get2DInfo`) and its aspect-derived constants, refreshed through `PrepareForReset`, `RestoreAfterReset`, and [`UIManager::UpdateCachedValues`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-UpdateCachedValues) (which calls `GetDeviceInfo`). The HUD draws into `m_BackBufferLinear`, so its target is that surface's size (§4).

## 6. Driving the re-init at a changed size

`ApplyResize` must run where `HandleModeChange` runs it (§2). Writing `m_WindowWidth`, `m_WindowHeight`, and `m_HasNewWindowSettings` asks the next prologue to do exactly that. The resize frees and re-allocates a large set of GPU resources, and only the whole walk keeps the pipeline coherent: the pass pools follow only through step 7, the UI view size only through steps 3 and 9.

## 7. Open unknowns

- The membership of `m_RegisteredCallbacksVector` was never walked.
- The `ApplyMode` fullscreen path, which recreates the device through `Graphics::ResetDevice`, was not fully traced.
- `m_DisplayModeChangeState`'s values are only partly confirmed (idle and mode-change pending); it is bound as a `u32`.
