# The mod-owned back buffer

While a VR session runs, the engine renders into a texture the mod allocates rather than into the DXGI swapchain, so the render resolution and the swapchain size are independent. The scene is per-eye sized, the swapchain stays at the window's client size, and the desktop mirror's present is 1:1.

`vr.own_back_buffer` (on) controls it. The implementation is `crates/jc3vr-view/src/vr/back_buffer.rs`, with two hooks in `crates/jc3vr-render/src/hooks/graphics_engine/engine.rs` and the transitions in `crates/jc3vr-view/src/vr/resolution.rs`. The engine behaviour it rests on is in [render-setups-reinit.md](../../engine/rendering/render-setups-reinit.md) and [rendering.md](../../engine/rendering/rendering.md).

## 1. Why

The engine builds its final composite target, `m_BackBufferLinear`, as a format alias of DXGI back buffer 0: `Graphics::CreateSurfaceAlias` copies the source handle, takes a Direct3D 11 reference on the same resource, and builds fresh views over it. The composite pass writes directly into the presentable surface, with no copy afterwards.

Driving the scene to the per-eye render resolution therefore drags the swapchain along, because the only way to resize `m_BackBufferLinear` is to resize what it aliases. That has three costs:

- The desktop present rescales every frame from a near-square per-eye buffer onto a 16:9 window, so the mirror needs an aspect pre-compensation and the egui overlay is round-tripped through a stretch.
- Single-pass double-wide forces a swapchain twice as wide, for a buffer only half of which is shown.
- The back-buffer size and the render size are the same number, so code that wants one can read the other and be right by accident.

Substituting a mod-owned texture for the three swapchain-derived objects removes the link. The scene chain, the composite, and the per-eye capture stay at render resolution, and the swapchain is only what the window presents.

## 2. The substitution set

[`GetDeviceSurface(device, surface)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-GetDeviceSurface) returns the device's [`Device::m_BackBuffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Device##field-m_BackBuffer) for the `BACK_BUFFER` selector and null otherwise. It has one code cross-reference in the binary, inside [`GraphicsEngine::CreateRenderSetups`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-CreateRenderSetups), so the set of engine objects derived from the swapchain surface is closed: three fields plus one mirror of a field. The tail of `CreateRenderSetups` builds them:

| Store | Built from |
|---|---|
| [`GraphicsEngine::m_BackBufferLinear`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_BackBufferLinear) | `CreateSurfaceAlias` over `GetDeviceSurface(BACK_BUFFER)` at [`SurfaceFormat::ABGR32`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::surface::SurfaceFormat##variant-ABGR32), named `"BackBufferLinear"` |
| [`GraphicsEngine::m_PostEffectRenderSetup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_PostEffectRenderSetup) | `CreateRenderSetup` with the raw device back-buffer surface as colour 0 and no depth |
| [`GraphicsEngine::m_BackBufferRenderSetup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_BackBufferRenderSetup) | `CreateRenderSetup` with `m_BackBufferLinear` as colour 0 and [`GraphicsEngine::m_MainDepthSurface`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_MainDepthSurface) as depth |
| [`GraphicsEngine::m_RenderContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_RenderContext)`.`[`m_RenderSetup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_RenderSetup) | the same pointer as `m_BackBufferRenderSetup` |

Their readers, each confirmed against the decompile:

- `m_BackBufferLinear`: `CreateRenderSetups`, as the colour target of `m_BackBufferRenderSetup`; `DestroyRenderSetups`, which frees it with `Graphics::DestroySurface` and nulls it; `HandleDrawThreadTask` and `HandleScreenShot`, which pass it to `CGraphicsEngine::SaveScreen` on the screenshot paths; and `CUIManager::InitPlatformRT`, which takes its RTV alongside `m_MainDepthSurface`'s DSV, the pair the HUD redirect displaces. `CGraphicsEngine::GetBackBufferLinear` is an accessor with no callers.
- `m_PostEffectRenderSetup`: read only in `HandleDrawThreadTask`, three times in succession: by `SetRenderSetup`, then as the first argument of `CPostEffectsManager::ApplyWorldFilters` and of `CRenderEngine::DrawPosteffects`.
- `m_BackBufferRenderSetup`: read in `HandleDrawThreadTask` (the final composite and UI target), in `ApplyResize`, and in `ApplyMode`.
- `m_RenderContext.m_RenderSetup`: the render context's copy, read wherever the context's setup is read back.

Replacing the three objects and writing the mirrored copy covers every consumer. Nothing else in the engine reaches the swapchain surface except `Graphics::ResizeBuffers` and `Graphics::Flip`, which own it.

## 3. The two hooks

The `CreateRenderSetups` epilogue performs the substitution. `CreateRenderSetups` has two callers, `InitializeSystem` and `ApplyResize`, so every path that rebuilds the engine's originals passes through it. On return, with the engine's three objects freshly built, the mod ensures its backing texture exists at the device-info size, builds a surface and two render setups over it, installs them, and frees the engine's originals. The size comes from `device->m_DeviceInfo`, which `CreateRenderSetups` has just built everything else from, so the substitute's dimensions agree with `m_MainDepthSurface`'s and the composite setup can pair the two. That agreement is the reason to substitute here rather than earlier.

The hook ignores `CreateRenderSetups`' return value. The declared type is `bool`, but the compiled function sets no return value: the last instruction before the epilogue is the trailing `CreateRenderSetup` call, so `al` holds the low byte of the pointer it returned, which is always even. A gate on it would never pass.

[`Graphics::ResizeBuffers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-ResizeBuffers) is replaced, not suppressed. `ApplyResize` calls it between `DestroyRenderSetups` and `CreateRenderSetups`, then reads the new size back out of `device->m_DeviceInfo` and passes it to `CreateRenderSetups` and every registered resize callback, so a plain no-op would leave the whole pipeline at the old size. The detour skips the DXGI half (the `OMSetRenderTargets(0, ...)`, the view and texture release, `IDXGISwapChain::ResizeBuffers`, and the buffer-0 re-acquire) but still writes `m_DeviceInfo.m_DisplayWidth`, `m_DisplayHeight`, and `m_DisplayRatio`, sets `m_WasResized`, and returns success. `ApplyResize` then runs unchanged: the scene targets, the pass pools, the UI reset, and the camera aspect follow the render size, while the DXGI buffers stay put.

Only the real function writes `device->m_BackBuffer`'s `m_Width` and `m_Height`, so under the substitute they keep reporting the true swapchain size. After the substitution, `m_BackBuffer` means the window and `m_BackBufferLinear` means the render target (§6).

Both hooks are gated on the same ownership flag, so one cannot act without the other. A half-installed pair would give `m_BackBufferRenderSetup` a window-sized colour target against a per-eye depth, which Direct3D 11 leaves undefined. The gate also keeps `Graphics::ResizeBuffers` doing its real job on the device-reset paths and on eject.

## 4. Ownership

[`Graphics::DestroySurface`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::surface##func-DestroySurface) has no already-freed guard, so a double destroy is a silent use-after-free that surfaces later somewhere unrelated. One ownership rule follows:

- The three substitute objects live in the engine's own fields, and the engine's `DestroyRenderSetups` frees them, because they are engine-allocated objects where engine-allocated objects belong. The mod never holds a second pointer to any of them and never frees them itself.
- The backing texture is the one object the engine knows nothing about. The mod owns that handle alone and frees it once, with `Graphics::Destroy2DTexture`, after the engine has stopped using it.

Two orderings follow:

- Within the substitution, the new pointers are installed before the old ones are destroyed, so every field stays non-null throughout. `CreateRenderSetups` runs only on the drained idle context, so no other thread is reading, but the ordering costs nothing.
- Across a transition, the ownership flag is cleared before the resize that rebuilds the engine's originals. Clearing it afterwards leaves the engine bound to a texture that is about to be freed.

`DestroyRenderSetup` unbinds a setup from the device if it is the active one, so destroying a bound setup is safe. Nothing registers surfaces by name (the resource tracking that read `params.m_Name` is compiled out), so the substitute's name, `"VrBackBuffer"`, only labels it in a memory dump.

## 5. Lifecycle

Entering: `resolution.rs` sets the ownership flag, then requests the per-eye size through the engine's own deferred resize path. In the same `ApplyResize`, the `ResizeBuffers` substitute holds the swapchain and the `CreateRenderSetups` epilogue performs the substitution. An ownership change forces a resize even at an unchanged size, because `ApplyResize` carries the substitution in either direction; without it, toggling `vr.own_back_buffer` mid-session would take effect only at the next resolution change.

Bringing the swapchain down: taking ownership stops the swapchain following the render size, but it does not shrink a swapchain already enlarged by an earlier resize, which is the usual state mid-session. `back_buffer::sync_swapchain_to_window`, called from the frame top, compares the swapchain with the window's client rect and drives a real `Graphics::ResizeBuffers` when they differ. It does nothing unless ownership is live, the substitution is installed, and the sizes disagree, so it also follows window resizes. It works only because of the substitution: `IDXGISwapChain::ResizeBuffers` fails while any reference to buffer 0 is outstanding, and the engine's alias is gone. The mirror's cached views are the mod's own such references, so they are dropped first and rebuilt on the next frame. The call is bracketed by a bypass flag, so the `ResizeBuffers` substitute stands aside, and by a save and restore of `m_DeviceInfo`'s display dimensions, which the real function overwrites. The device info must keep meaning the render size; left overwritten, the render-size driver sees the size collapse to the window's, requests the per-eye size again, and every sync starts another resize round trip.

Leaving, and eject: the flag is cleared first, then the engine resizes to the window's live client size. With the flag clear both hooks are inert, so this is the stock path: `DestroyRenderSetups` frees the substitutes, the real `Graphics::ResizeBuffers` runs, and `CreateRenderSetups` rebuilds the engine's own alias and setups over the live swapchain. Only then is the backing texture destroyed. On eject the game thread tears down without drawing another frame, so `resolution.rs`'s `on_shutdown` runs that sequence synchronously, after `WaitForCPUDrawToFinish`, rather than through the deferred path. The synchronous restore is a separate function so that its early exits cannot skip the texture release that follows it. The detours come out later, in the payload's `shutdown_startup`, which is safe because the flag is already clear and a stray `CreateRenderSetups` in between does nothing.

The restore targets the window's live client size, falling back to the size captured at take-over. The engine follows window resizes itself, so after a window resize during the session the captured size is stale.

## 6. Where the frame goes, and what reads its size

Nothing is suppressed, so the flow after the substitution is the engine's own, one target over:

1. `HandleDrawThreadTask` runs the post effects into `m_PostEffectRenderSetup` (the mod texture, no depth), then the composite and the Scaleform UI into `m_BackBufferRenderSetup` (the mod texture and the per-eye `m_MainDepthSurface`), then `QuickDrawController::Draw`, then `Graphics::EndDraw`, all at render resolution.
2. The mod's `RenderEngine::PostDraw` hook copies from `m_BackBufferLinear` into the per-eye capture textures.
3. `BLOCK_FLIP` suppresses `Graphics::Flip`, so the engine never presents.
4. `vr::blit` copies the captures into the OpenXR swapchain images and submits.
5. `vr::mirror` draws the chosen view into `device.m_BackBuffer`, which is window-sized under the substitution, composites the egui panel or the flat overlay, and presents.

The substitution adds no present path; it makes the existing one 1:1.

Because `m_BackBuffer` and `m_BackBufferLinear` answer different questions, every consumer picks one. `jc3vr_view::stereo::render_size` reads `m_BackBufferLinear` and is the render basis, and `per_eye_render_size` halves its width under single-pass double-wide. Anything sized to the render uses those: the per-eye capture textures, the HUD's reset proxy, and its engine-binding restore. Only code addressing the presented surface reads the swapchain's size: the mirror's viewport and the cursor mapping's window basis.

The mod-owned render-sized resources (the FSR contexts and outputs and the per-eye capture textures) are outside the engine's teardown ([render-setups-reinit.md §3](../../engine/rendering/render-setups-reinit.md#3-what-destroyrendersetups-tears-down)), so a resize does not re-create them. Each compares the size it was built for with the current render size when it next runs and re-creates itself on a change (`crates/jc3vr-render/src/fsr/mod.rs` and `crates/jc3vr-view/src/hooks/graphics_engine/capture.rs`), following the engine's resize rather than racing it.

The HUD texture follows neither size. It is a flat panel at a fixed apparent size, so it needs enough texels to look sharp at that size, which depends on the headset and the panel's angular size rather than the scene's pixel count. Its size is the HUD tuning's `panel.render_scale` ([tuning.md](../tuning.md)) times the geometric mean of one eye's render width and height. The geometric mean keeps the budget stable as the target's shape changes, where the longer axis would not.

## 7. Interactions

`vr::resolution` keeps the engine's trigger: it writes `m_WindowWidth`, `m_WindowHeight`, and `m_HasNewWindowSettings`, and lets `HandleModeChange` service the request from the `Draw` prologue, the one frame position where the context is idle by construction. Its completion detection works because the `ResizeBuffers` substitute writes the device-info fields it polls, and its window-rect cross-check is stricter, since nothing in the chain touches the window. It also runs the ownership state machine (§5).

`vr::mirror::layout`'s buffer-to-window pre-compensation becomes the identity: with the swapchain equal to the client rect, its scale factors are 1 and the buffer-space viewport is the window-space one. It still applies whenever `vr.own_back_buffer` is off. `hud::mirror_overlay` is window-sized and stretched across the whole buffer to invert the present's stretch, which with the buffer equal to the window is pixel-exact.

The HUD redirect (`jc3vr-hud`'s `hud::binding`) is unaffected: it replaces Scaleform's RTV and DSV with mod-owned views whatever `m_BackBufferLinear` points at, and `hud::tick` re-applies it every frame from the render thread.

The flat desktop overlay is drawn into `device.m_BackBuffer` from the `graphics_flip` detour, only when `BLOCK_FLIP` is clear, that is, only outside a session, where the substitution is inactive (§8). It is the one mod path that draws into the swapchain outside the mirror.

## 8. Flatscreen

The substitution is active only while the mod drives a render size of its own: `resolution.rs` takes ownership only while an XR session runs with `vr.native_resolution` on and `vr.own_back_buffer` set.

Outside that it would buy nothing. In flatscreen the engine's render size is the window size, so substituting a texture of the same dimensions would add an indirection, a second allocation, and an eject-path risk, and would break the flat overlay's assumption that the engine presents (§7). The rule that the game never renders directly to the swapchain holds during VR only, not from injection onwards; making it hold always would mean owning the swapchain through the launcher, the loading screens, and flatscreen play.

Flatscreen stereo renders two eyes but presents to the window at window resolution, so it follows the flatscreen rule. If it ever needs a render size different from the window, the same predicate covers it.

## 9. The engine surface API

The four constructors and the two destructors are `Graphics::` free functions taking the device, and `CreateRenderSetups` and `DestroyRenderSetups` use all six, so the mod calls them as the engine does. They are defined in `graphics_engine/surface.pyxis` with their parameter structs and enums.

A surface in this engine is an `HTexture_t` obtained either from `GetRenderTarget` over a texture or from `CreateSurfaceAlias` over another surface; there is no `Graphics::CreateSurface`. `CreateRenderSetups` builds every scene target as `Create2DTexture` then `GetRenderTarget`, and the substitute follows that pattern.

A render setup stores no width or height: its dimensions come from its bound targets, which is why `SetRenderSetup` can synthesize a full-target viewport. The substitute setups use the values `CreateRenderSetups` uses for both back-buffer setups: [`MultisampleFormat::None`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::surface::MultisampleFormat##variant-None) for [`CreateRenderSetupParams::m_MultisampleFormat`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::surface::CreateRenderSetupParams##field-m_MultisampleFormat), [`CreateRenderSetupParams::MASK_ALL_CHANNELS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::surface::CreateRenderSetupParams::MASK_ALL_CHANNELS) for [`CreateRenderSetupParams::m_Mask`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::surface::CreateRenderSetupParams##field-m_Mask), and [`CreateRenderSetupParams::DEFAULT_FLAGS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::surface::CreateRenderSetupParams::DEFAULT_FLAGS) for [`CreateRenderSetupParams::m_Flags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::surface::CreateRenderSetupParams##field-m_Flags). The doc comment on `DEFAULT_FLAGS` lists its subfields.

Both parameter structs are written field by field through a raw pointer into zeroed storage rather than built as Rust values. The engine leaves several fields at zero, and zero is not a valid discriminant for every enum among them (`TileMode` has none), so producing either struct by value would be undefined behaviour. Writing through the pointer makes the bytes exactly what the engine's own call sites pass.

### 9.1 One texture, no alias

The substitute is a single `ABGR32` texture filling both the `m_PostEffectRenderSetup` colour-target role and the `m_BackBufferLinear` role. No alias and no `m_Castable` is needed, because there is no format gap to bridge.

[`GraphicsEngine::InitializeSystem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-InitializeSystem) hard-codes `m_FrontBufferFormat` to `ABGR32` and leaves the adjacent `m_FrontBufferColorSpace` zero. `Graphics::InitializeDevice` applies `MakeSRGBFormat` only when that colour-space flag is set, so the format passes through unconverted to `device->m_BackBufferFormat` and to the swapchain description: the swapchain is `R8G8B8A8_UNORM`, not the sRGB variant. The same function writes that value into the back-buffer `HTexture_t`'s format, so `GetDeviceSurface(BACK_BUFFER)->m_Format` is `ABGR32` too. `SurfaceFormat` has no separate sRGB enumerator for `ABGR32`; sRGB is applied at creation. For these common formats the engine's enum values are the DXGI numbering, so `m_BackBufferLinear`'s requested format equals its source's native format. The mirror relies on the swapchain being non-sRGB when it applies no gamma conversion.

### 9.2 What `CreateSurfaceAlias` does

It copies the source `HTexture_t`, takes a Direct3D 11 reference on the resource, then:

```
if ( source->m_Texture != device_backbuffer->m_Texture || requested_format == device_backbuffer->m_Format )
    build a fresh SRV at requested_format
else
    no SRV
build a fresh RTV at requested_format   /* unconditionally */
```

The SRV is skipped only when aliasing the device back buffer's own resource at a format that differs from its native one, probably because that resource is not typeless. When the formats match, as they do here, a real SRV is built, so `m_BackBufferLinear` is an ordinary alias with a real reference and real views. That reference is why `ApplyResize` destroys the setups before calling `Graphics::ResizeBuffers`, and why an independent swapchain resize is possible once the alias is gone (§5).

## 10. Unverified

- Under the substitution, the engine's screenshot paths (`SaveScreen` from `HandleDrawThreadTask` and `HandleScreenShot`) capture at render resolution rather than window resolution. They read the surface, not the swapchain, so they should work; the mod has its own capture path.
- Whether Scaleform's `RestoreAfterReset` behaves with a mod-owned `m_BackBufferLinear` is unobserved; the HUD redirect re-applies immediately afterwards.
- Whether fixing the DXGI buffers' size changes DXVK's present path (filtering, latency, or frame pacing) beyond removing the downscale is unmeasured.

## 11. A note on symbols

The IDB's `Graphics::` names are unreliable. Two functions have their names swapped: the function labelled `Graphics::CopySurfaceToTexture` is [`Graphics::EndDraw(ctx)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-EndDraw), and the function labelled `NGraphicsEngine::CGPUProfiler::BeginScope` is [`Graphics::CopySurfaceToTexture(ctx, dst, src)`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-CopySurfaceToTexture). [`GraphicsEngine::SaveScreen`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-SaveScreen) is labelled `CGPUProfiler::EndFrame`, and [`Graphics::CreateRenderSetup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::surface##func-CreateRenderSetup) is labelled `LockVolumeTexture`.

Read by name, these functions suggest a final resolve into the back buffer that would have to be suppressed or retargeted, and no such resolve exists. Every function this page relies on is verified by call-site correspondence or body inspection rather than by name, and any further symbol from that namespace needs the same check.
