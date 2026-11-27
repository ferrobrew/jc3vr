# The mod-owned back buffer

While a VR session runs, the engine renders into a texture the mod allocates rather than into the DXGI swapchain, so the render resolution and the swapchain size are independent: the scene is per-eye (or double-wide) sized, the swapchain stays at the window's client size, and the desktop mirror's present is 1:1. `vr.own_back_buffer` (on) controls it. The code is `crates/jc3vr-view/src/vr/back_buffer.rs`, two hooks in `crates/jc3vr-render/src/hooks/graphics_engine/engine.rs`, and the transitions in `crates/jc3vr-view/src/vr/resolution.rs`. The engine behaviour it rests on is in [render-setups-reinit.md](../../engine/rendering/render-setups-reinit.md).

## Why

The engine's final composite target, `m_BackBufferLinear`, is a format alias of DXGI back buffer 0 (`Graphics::CreateSurfaceAlias` takes a reference on the same resource and builds fresh views over it), and the composite writes straight into the presentable surface. Driving the scene to the per-eye resolution therefore drags the swapchain along: the desktop present rescales a near-square buffer onto a 16:9 window every frame, single-pass's double-wide target forces a swapchain twice as wide, and the back-buffer size and render size become the same number, so code that wants one can read the other and be right by accident. Substituting a mod-owned texture for the swapchain-derived objects removes the link.

## The substitution set

[`GetDeviceSurface`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-GetDeviceSurface) returns [`Device::m_BackBuffer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::Device##field-m_BackBuffer) and has one code reference, in the tail of [`GraphicsEngine::CreateRenderSetups`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-CreateRenderSetups), so the set of objects derived from the swapchain is closed:

- [`GraphicsEngine::m_BackBufferLinear`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_BackBufferLinear), the alias at [`SurfaceFormat::ABGR32`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::surface::SurfaceFormat##variant-ABGR32);
- [`GraphicsEngine::m_PostEffectRenderSetup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_PostEffectRenderSetup), with the raw back-buffer surface as colour and no depth;
- [`GraphicsEngine::m_BackBufferRenderSetup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_BackBufferRenderSetup), with `m_BackBufferLinear` and [`m_MainDepthSurface`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_MainDepthSurface);
- and the render context's [`m_RenderSetup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_RenderSetup), the same pointer as `m_BackBufferRenderSetup`.

Replacing those covers every reader (confirmed against the decompile, including `CUIManager::InitPlatformRT`, whose RTV and DSV pair the HUD redirect displaces); nothing else reaches the swapchain surface except `Graphics::ResizeBuffers` and `Graphics::Flip`, which own it.

The substitute is one `ABGR32` texture in both the post-effect and `m_BackBufferLinear` roles, with no alias. [`GraphicsEngine::InitializeSystem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-InitializeSystem) leaves the front-buffer colour-space flag clear, so `Graphics::InitializeDevice` applies no sRGB conversion and the swapchain is `R8G8B8A8_UNORM`; the alias's requested format equals its source's, so `CreateSurfaceAlias` builds a real SRV and RTV over a real reference, which is why `ApplyResize` destroys the setups before resizing the swapchain. The mirror relies on the swapchain being non-sRGB.

## The two hooks

The `CreateRenderSetups` epilogue performs the substitution. Its two callers, `InitializeSystem` and `ApplyResize`, cover every path that rebuilds the originals. On return the mod ensures its texture exists at the device-info size (the size `CreateRenderSetups` just built everything else from, so it agrees with `m_MainDepthSurface`), builds a surface and two setups over it, installs them, and frees the engine's originals. The hook ignores the return value: the declared `bool` is never set, and `al` holds the low byte of the last `CreateRenderSetup` call's pointer, which is always even.

[`Graphics::ResizeBuffers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::device##func-ResizeBuffers) is replaced, not suppressed, because `ApplyResize` reads the new size back out of `device->m_DeviceInfo` for `CreateRenderSetups` and every resize callback. The replacement skips the DXGI half but writes the device info's display width, height, and ratio, sets `m_WasResized`, and returns success, so the scene targets, pass pools, UI reset, and camera aspect follow the render size while the DXGI buffers stay put. `m_BackBuffer`'s own dimensions keep reporting the swapchain, so after the substitution `m_BackBuffer` means the window and `m_BackBufferLinear` means the render target.

Both hooks are gated on one ownership flag: a half-installed pair would pair a window-sized colour target with a per-eye depth, which D3D11 leaves undefined. The gate also keeps the real `ResizeBuffers` on the device-reset paths and on eject.

## Ownership

[`Graphics::DestroySurface`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::surface##func-DestroySurface) has no already-freed guard, so a double destroy is a silent use-after-free. The three substitute objects live in the engine's fields and the engine's `DestroyRenderSetups` frees them; the mod never holds a second pointer to them. The backing texture is the one object the engine knows nothing about, and the mod frees it once, with `Graphics::Destroy2DTexture`, after the engine has stopped using it. Within the substitution the new pointers are installed before the old are destroyed, and across a transition the ownership flag is cleared before the resize that rebuilds the originals; clearing it after leaves the engine bound to a texture about to be freed.

The surface API's parameter structs (`graphics_engine/surface.pyxis`) are written field by field into zeroed storage, because the engine leaves fields at zero that are not a valid discriminant of their enum (`TileMode` has none).

## Lifecycle

Entering: `resolution.rs` sets the flag and requests the per-eye size through the engine's deferred resize, and in that `ApplyResize` the substitute holds the swapchain and the epilogue substitutes. An ownership change forces a resize even at an unchanged size, since `ApplyResize` carries the substitution either way. Ownership is taken only while an XR session runs with `vr.native_resolution` on and `vr.own_back_buffer` set; on flatscreen the render size is the window's, so the substitution would add an indirection and an eject risk for nothing.

Bringing the swapchain down: taking ownership does not shrink a swapchain an earlier resize enlarged. `back_buffer::sync_swapchain_to_window`, from the frame top, drives a real `Graphics::ResizeBuffers` when the swapchain and the client rect differ, which also follows window resizes. It works only because the engine's alias is gone (`IDXGISwapChain::ResizeBuffers` fails while any reference to buffer 0 is outstanding); the mirror's cached views are dropped first. The call is bracketed by a bypass flag and a save and restore of the device info's display size, which the real function overwrites; left overwritten, the render-size driver would see the size collapse and start another resize round trip every sync.

Leaving, and eject: the flag is cleared, then the engine resizes to the window's live client size (falling back to the size captured at take-over) on the stock path, and only then is the texture destroyed. On eject the game thread draws no more frames, so `resolution.rs`'s `on_shutdown` runs that sequence synchronously after `WaitForCPUDrawToFinish`, in a separate function so its early exits cannot skip the texture release. The detours come out later, which is safe with the flag already clear.

## Where the frame goes, and what reads its size

The flow after the substitution is the engine's own, one target over: the post effects, composite, and Scaleform UI render into the mod texture; the `RenderEngine::PostDraw` hook copies it into the per-eye capture textures; `BLOCK_FLIP` suppresses `Graphics::Flip`; `vr::blit` submits to OpenXR; and `vr::mirror` draws into the window-sized `device.m_BackBuffer` and presents. The flat desktop overlay draws into the swapchain from the `graphics_flip` detour only outside a session.

`jc3vr_view::stereo::render_size` reads `m_BackBufferLinear` and is the render basis, and `per_eye_render_size` halves it under double-wide; the capture textures and the HUD's reset proxy and binding restore use those. Only code addressing the presented surface (the mirror's viewport and the cursor mapping) reads the swapchain size. With the swapchain equal to the client rect, `vr::mirror::layout`'s pre-compensation is the identity. The FSR contexts and capture textures are outside the engine's teardown ([render-setups-reinit.md](../../engine/rendering/render-setups-reinit.md#3-what-destroyrendersetups-tears-down)), so each re-creates itself when it next sees a different render size. The HUD texture follows neither size: it is the HUD tuning's `panel.render_scale` times the geometric mean of one eye's render width and height ([tuning.md](../tuning.md)), which keeps the budget stable as the target's shape changes.

## Unverified

- The engine's screenshot paths capture at render resolution under the substitution; they read the surface, so they should work.
- Whether Scaleform's `RestoreAfterReset` behaves with a mod-owned `m_BackBufferLinear` is unobserved; the HUD redirect re-applies right after.
- Whether fixing the DXGI buffers' size changes DXVK's present pacing is unmeasured.

## A note on symbols

The IDB's `Graphics::` names are unreliable: the function labelled `Graphics::CopySurfaceToTexture` is [`Graphics::EndDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-EndDraw), the one labelled `CGPUProfiler::BeginScope` is [`Graphics::CopySurfaceToTexture`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::draw##func-CopySurfaceToTexture), [`GraphicsEngine::SaveScreen`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-SaveScreen) is labelled `CGPUProfiler::EndFrame`, and [`Graphics::CreateRenderSetup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/graphics_engine::surface##func-CreateRenderSetup) is labelled `LockVolumeTexture`. Read by name they suggest a final resolve into the back buffer, and none exists; any symbol from that namespace needs checking by call site or body.
