# VR runtime

The mod brings up an OpenXR session against the game's own D3D11 device, drives the render camera from the located head pose, builds each eye's off-axis projection from the headset's field of view, renders each eye at the headset's recommended resolution, copies the eyes into a stereo swapchain, and submits one projection layer. The game's own present stays blocked while a session runs, and the mod presents the desktop mirror into the game window in its place.

The runtime is `crates/jc3vr-view/src/vr/`, with the settings in `config.rs` (`vr.*`); the payload's `hooks::game` drives the frame, and `crates/jc3vr-view/src/headpose/xr.rs` composes the head pose into the world. Threads and the runtime's two locks are in [threading.md](threading.md).

## The frame loop

The loop runs inside the [`Game::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateRender) detour (`payload/src/hooks/game.rs`), in this order:

1. `vr::update` pumps the event queue and brings the session up, retries it, or tears it down.
2. `vr::auto_recenter_tick` services pending recenters, before the views are located, since a recenter moves the baseline they are expressed against.
3. `vr::apply_native_resolution` requests the per-eye resize, before the first dispatch, whose `Draw` prologue services it.
4. `vr::frame_wait` calls `xrWaitFrame`, which paces the game in place of vsync, and locates the views.
5. `vr::begin_render_frame` publishes the head pose and the per-eye parameters (or clears them when the runtime skips the frame) before the original render update, which consumes the camera's interpolation pair.
6. `WaitedFrame::begin` waits for the previous frame's tail and calls `xrBeginFrame`. With `vr.overlap_render_update` off, the render update waits for the tail before step 1 instead.
7. `vr::arm_direct` acquires the swapchain image, so each eye's [`RenderEngine::PostDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-PostDraw) can blit straight into its slice.
8. The dispatch plan renders the eyes, per eye or once for both in single-pass ([single-pass-stereo.md](stereo/single-pass-stereo.md)).
9. `vr::present_and_submit` ends the frame under the engine's immediate-context mutex, since the runtime's D3D11 client records on the game's immediate context.
10. `vr::present_mirror` presents the game's swapchain, the only present of the frame.

The tail (the final drain, the mirror, and the submit) runs on the frame-tail worker (`vr::tail`) while the game thread starts the next update. The render hooks never read the runtime's state: the per-eye parameters reach them through per-frame slots (`vr::render_params`), so the draw task reads the frame it records while the game thread prepares the next.

## Session lifecycle

Bring-up loads the OpenXR loader (`vr.loader_path`, else the one beside the payload), creates the instance and a session against the engine's device, and uses the LOCAL space, locating STAGE only for the floor height. A loader that will not load is a deployment fault and aborts the process; any other failure leaves the mod in flatscreen stereo and is retried, so a game without a headset plays normally.

The runtime allows few instances and sessions per process, and Proton's startup probe uses part of that budget, so an eject stashes the instance and session handles in the process environment and a reinject re-wraps them (`vr::persist`). Turning `vr.enabled` off, an `EXITING` or `LOSS_PENDING` transition, and instance loss destroy everything and clear the stashes.

## The cockpit pose

The runtime keeps a recenter baseline, a position and a yaw-only orientation; pitch and roll are the player's real head tilt and are never re-based. `begin_render_frame` composes the views' centre pose into the world through `headpose::xr::compose` (by the body frame, on the animated head anchor, at `vr.world_scale`); each eye's offset is its delta from the centre. It publishes a tick-spaced pair (`headpose::set_pose_pair`), so the engine's interpolation smooths the tick-rate body frame and anchor while the headset's delta, the same on both sides, adds no latency ([head-and-body.md](body/head-and-body.md#design)).

A recenter is a request (F7, the pause menu, the stick-click chord, and the seat transitions) that sets an atomic bit for `auto_recenter_tick`, because the callers cannot take the runtime lock ([threading.md](threading.md#lock-rules)). What it re-bases is in [rig.md](body/rig.md#recentering).

The frame calls the crates above it through tables they register at install (`vr/participants.rs`, `headpose/participants.rs`). The render crate's `vr::TeardownParticipant` is required, since a teardown without it would leave the game's present blocked.

## Projection

`projection.rs` builds the off-axis projection from the `XrFovf` half-angles as the engine builds [`Camera::m_Projection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##field-m_Projection) ([rendering.md §2.9](../engine/rendering/rendering.md#29-projection-construction--exact-formulas-real-nearfar-and-consume-vs-rebuild-decompiled)), with the engine's own near and far planes. It is a standard-depth projection written before [`Camera::SetupRenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-SetupRenderCamera), so the engine applies its reverse-Z remap and jitter once, as for every camera; an already reversed matrix would be remapped twice ([rendering.md §2.7](../engine/rendering/rendering.md#27-reverse-z--depth-convention--the-per-eye-projection-wedge-bug)). The eye blit linearizes the display-referred capture, so the `_SRGB` swapchain's encode reproduces the original bytes.

## Per-eye native resolution

With `vr.native_resolution` on, each eye renders at the runtime's recommended size times `vr.resolution_scale`, the swapchain's size, so the blit is a 1:1 copy. The engine has no dynamic-resolution path, so `resolution.rs` drives its own deferred mode change: it writes the pending size and sets [`m_HasNewWindowSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_HasNewWindowSettings), and [`HandleModeChange`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleModeChange) resizes in the `Draw` prologue, where the context is idle as [`ApplyResize`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-ApplyResize) assumes ([render-setups-reinit.md](../engine/rendering/render-setups-reinit.md)). With `vr.own_back_buffer` the DXGI swapchain stays at the window size ([swapchain-ownership.md](stereo/swapchain-ownership.md)).

The original size is restored when the session ends and at eject, while the hooks are still live ([lifecycle.md](lifecycle.md#teardown-after-commit)), and a resize that fails its check turns native resolution off for the session.

## The desktop mirror

`vr/mirror/` presents the game's swapchain once per frame, unsynced, since a vsynced present would throttle the whole loop to the monitor. It draws `vr.mirror_view` into the back buffer: a single eye fills the window, cropped and magnified by the view tuning's `mirror.zoom`, and the stereo pair (F10) is letterboxed whole in cross-eyed order, since cropping would cut each eye's outer edge differently and spoil the fusion. The debug overlay or the floating panel is composited over it at the window's size, and a draw or present fault turns the mirror off for the session.

The debug UI's Runtime feature has the `vr.*` switches and draws the hidden-area mask `vr/visibility_mask.rs` fetches from `XR_KHR_visibility_mask`, though nothing is masked with it. `vr.overlap_render_update` and `vr.large_cache_cores` are performance choices ([performance.md](performance/performance.md#what-limits-the-frame)).
