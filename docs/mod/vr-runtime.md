# VR runtime

The mod brings up an OpenXR session against the game's own D3D11 device, drives the render camera from the located head pose, builds each eye's off-axis projection from the headset's field of view, renders each eye at the headset's recommended resolution, copies the eyes into a stereo swapchain, and submits one projection layer. The game's own present stays blocked while a session runs, and the mod presents the desktop mirror into the game window in its place.

The runtime is `crates/jc3vr-view/src/vr/`: the session and its state (`state.rs`, `session.rs`, `persist.rs`), the frame API (`frame_loop.rs`, and `frame.rs`, which hands a frame to the render hooks), the submit side and the frame tail (`submit.rs`, `tail.rs`), the projection (`projection.rs`), the eye blit (`blit.rs`), the per-eye resize (`resolution.rs`), the mirror (`mirror/`), the recenter (`recenter.rs`), and the settings (`config.rs`). The payload's `hooks::game` drives the frame, and `crates/jc3vr-view/src/headpose/xr.rs` composes the head pose into the world. Which thread runs what, and the rules for the runtime's two locks, are in [threading.md](threading.md).

## The frame loop

The loop runs inside the [`Game::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateRender) detour (`payload/src/hooks/game.rs`), in this order:

1. `vr::update` pumps the OpenXR event queue once, brings the session up, retries it, or tears it down, and returns whether a session runs. The head pose source is `Vr` while one runs and `Sim` otherwise, so the flatscreen mouse look yields to the headset.
2. `vr::auto_recenter_tick` services the recenters asked for since the last frame and the recenter on resuming gameplay. It runs before the views are located, since a recenter moves the baseline they are expressed against.
3. `vr::apply_native_resolution` requests the per-eye resize ([below](#per-eye-native-resolution)). It must come before the first dispatch, whose `Draw` prologue services the request.
4. `vr::frame_wait` calls `xrWaitFrame`, locates the views, and samples the controllers, returning a `WaitedFrame`. `xrWaitFrame` paces the game against the compositor in place of vsync.
5. `vr::begin_render_frame` publishes the head pose and the per-eye render parameters, but only when the runtime asks for the frame to be rendered; otherwise the parameters are cleared and the camera hook falls back to flatscreen stereo for the draws that keep the engine alive. The publish comes before the original render update because the camera's `T0 → T1` interpolation is consumed inside it. The original `UpdateRender` runs next.
6. `WaitedFrame::begin` calls `xrBeginFrame` and returns the `FrameContext` that holds the submit side (the frame stream and the swapchain) until the frame is submitted. It first waits for the previous frame's tail to submit. With `vr.overlap_render_update` off, the render update waits for the tail before step 1 instead, and the frame begins right after step 4.
7. `vr::arm_direct` acquires the swapchain image, so each eye's [`RenderEngine::PostDraw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_engine::RenderEngine##func-PostDraw) can blit it from [`m_BackBufferLinear`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_BackBufferLinear) straight into its slice. Under the single-pass collapse the one dispatch blits each eye from its half of the double-wide back buffer, the blit clamping to that half's outermost texel centres.
8. The dispatch plan renders the eyes, once per eye in multipass or once for both in single-pass ([single-pass-stereo.md](stereo/single-pass-stereo.md)). The [`Camera::SetupRenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##func-SetupRenderCamera) detour applies each eye's projection and world offset.
9. `vr::present_and_submit` blits any eye the post-draw did not and ends the frame: a projection layer when the frame was rendered, an empty frame otherwise. The swapchain acquire, release, and `xrEndFrame` run under the engine's immediate-context mutex, since the runtime's D3D11 client records on the game's immediate context.
10. `vr::present_mirror` draws the mirror into the game window and presents the game's swapchain, the only present of the frame.

With a VR frame to submit, the frame tail (the final drain, steps 9 and 10, in the order mirror then submit) runs on the frame-tail worker (`vr::tail`), and the game thread returns to the next frame's update. The tail stays inline while a render trace runs, under a render mode other than the native one, and while the visual harness needs it inline.

The render hooks never read the runtime's state: the per-eye parameters reach the camera hook and the draw task through per-frame slots (`vr::render_params`), so the draw task reads the frame it records while the game thread prepares the next ([threading.md](threading.md#who-holds-what-across-a-frame)).

## Session lifecycle

Bring-up loads the OpenXR loader dynamically (`vr.loader_path`, else `openxr_loader.dll` beside the payload, else the default search path), creates the instance with `XR_KHR_D3D11_enable` and, when the runtime offers them, hand tracking and `XR_KHR_visibility_mask`, acquires the headset system, creates the session against the graphics engine's `ID3D11Device`, and creates the LOCAL reference space. The STAGE space is located against LOCAL only for the floor height (`vr::stage`). The stereo swapchain, one two-slice texture array, is created on the first rendered frame.

A loader that will not load is a deployment fault: the bring-up logs it, shows a message box, and aborts the process, rather than leaving a game that plays flat while its log looks like an idle headset. Any other failure logs on the `vr` target, leaves the mod in flatscreen stereo, and is retried every `runtime.retry_interval_s` of the view tuning while `vr.enabled` is on, so a game without a headset or a running runtime plays normally.

The runtime allows few instances and sessions per process, and Proton's startup probe uses part of that budget, so an eject persists the instance and the session instead of destroying them: it stashes their handles in the process environment, without ending the session, and a reinject re-wraps them (`vr::persist`). A persisted instance keeps the extensions it was created with, so an extension the runtime added later is missing until the game restarts. Turning `vr.enabled` off, a session transition to `EXITING` or `LOSS_PENDING`, and instance loss destroy everything in order (swapchain, session, instance) and clear the stashes.

## The cockpit pose

The runtime keeps a recenter baseline: a position and a yaw-only orientation taken from a located head pose. Pitch and roll are the player's real head tilt and are never re-based. `frame_wait` locates the eye views in LOCAL space and expresses each against the baseline. `begin_render_frame` reduces them to a centre pose, composes it into the world through `headpose::xr::compose` (rotated by the body frame, placed on the animated head-bone anchor, and scaled by `vr.world_scale`), and publishes it. Each eye's world offset is its located delta from that centre, rotated into the world by the body frame.

The VR source publishes through `headpose::set_pose_no_interp`, which writes the same pose to both sides of the engine's `T0 → T1` interpolation pair. The engine interpolates its camera to smooth its fixed-rate simulation tick, but the headset pose is sampled fresh at the predicted display time each frame, so interpolating it would only lag the view behind the head ([head-and-body.md](body/head-and-body.md#design)).

A recenter is a request: `vr::recenter` (F7), `vr::request_recenter` (the pause menu and the stick-click chord), and `vr::request_seat_recenter` (the seat transitions) set atomic bits that `auto_recenter_tick` services at the top of the next frame, because the callers are not at a point where the runtime lock may be taken ([threading.md](threading.md#lock-rules)). What a recenter re-bases, yaw and height on foot or the full pose in a seat, and how the body rig follows, are in [rig.md](body/rig.md#recentering).

The frame calls code above the view crate without naming it, through tables those crates register at install:

- `vr::FrameParticipants`, filled by the gameplay crate: the roomscale cockpit composition, a recenter's observers, and the choice of a manual recenter's kind.
- `headpose::HeadParticipants`, filled by the gameplay crate: the character state the body turn and the anchors read.
- `headpose::BodyFrameFilter`: a filter on the body frame and the head anchor, which the grapple registers for its comfort hold ([grapple-comfort.md](body/grapple-comfort.md)). Without one the body frame passes through.
- `vr::TeardownParticipant`, filled by the render crate, releases the flip block at teardown. The install fails when it is missing, since a teardown without it would leave the game's present blocked.
- `vr::ResizeObserver`, filled by the render crate, sees the resizes the native-resolution driver requests, for the texture census.

The visual harness pushes the headset-like rig it substitutes for the located views into `vr::harness`, where the frame loop and the camera hooks read it.

## Projection

`projection.rs` turns the four `XrFovf` half-angles into an off-axis projection in the engine's row-major, row-vector layout, built element for element as the engine's `CMatrix4f::PerspectiveOffCenter` builds [`Camera::m_Projection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##field-m_Projection) ([rendering.md §2.9](../engine/rendering/rendering.md#29-projection-construction--exact-formulas-real-nearfar-and-consume-vs-rebuild-decompiled)). It uses the engine's own near and far planes ([`Camera::DEFAULT_NEAR_PLANE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera::DEFAULT_NEAR_PLANE) and [`Camera::DEFAULT_FAR_PLANE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera::DEFAULT_FAR_PLANE)); a nearer far plane would clip the horizon.

The camera hook writes a standard-depth (not reversed) projection before `SetupRenderCamera`, so the engine applies its reverse-Z remap and TAA jitter to it exactly once, as for every other camera. Writing an already reversed matrix there applies the remap twice ([rendering.md §2.7](../engine/rendering/rendering.md#27-reverse-z--depth-convention--the-per-eye-projection-wedge-bug)). The eye blit linearizes the display-referred capture, so the `_SRGB` swapchain's hardware encode reproduces the original bytes.

## Per-eye native resolution

While a session runs and `vr.native_resolution` is on, each eye renders at the runtime's recommended size times `vr.resolution_scale`, the same size the swapchain uses, so the blit is a 1:1 copy. The engine has no dynamic-resolution path: every render target is sized from the device's [`m_DisplayWidth`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::device::DeviceInfo##field-m_DisplayWidth) and `m_DisplayHeight` when the render setups are created.

`resolution.rs` drives the engine's own deferred display-mode change instead of calling [`ApplyResize`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-ApplyResize) itself: it writes the pending size into `m_WindowWidth` and `m_WindowHeight` and sets [`m_HasNewWindowSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_HasNewWindowSettings), as a settings change does. [`HandleModeChange`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##func-HandleModeChange), in the `Draw` prologue, then resizes at a boundary where the previous dispatch has drained and this frame has not dispatched, which is the idle context `ApplyResize` assumes ([render-setups-reinit.md](../engine/rendering/render-setups-reinit.md)). The resize never touches the Win32 window. It would also resize the DXGI swapchain, but with `vr.own_back_buffer` on the mod substitutes that call, so the swapchain stays at the window size ([swapchain-ownership.md](stereo/swapchain-ownership.md)).

The size before the first resize is restored when the session ends and at eject. The eject's cleanup requests the restore while the hooks are live, and the hooks are removed a few frames later ([lifecycle.md](lifecycle.md#teardown-after-commit)), so the `Draw` prologue services it. Every serviced resize is checked: a request not serviced in time, a wrong size, or a changed window rect turns native resolution off for the session, restores the original size, and the mod continues at the desktop resolution.

## The desktop mirror

While a session runs the engine's present is blocked, so the game window would hold a stale frame. `vr/mirror/` presents the game's swapchain itself once per frame, unsynced: a vsynced present on a 60 Hz monitor would throttle the whole loop, headset submit included, to the monitor's rate. It draws `vr.mirror_view` (`Left`, `Right`, or `StereoPair`) into the game's back buffer. With `vr.own_back_buffer` on the back buffer is the window's client size and the present is 1:1; with it off the viewport pre-compensates DXGI's stretch of the near-square buffer onto the window.

A single eye is filled: scaled to cover the window, with the overflow cropped by the render target's own clip, and magnified about the centre by the view tuning's `mirror.zoom`. The stereo pair is fitted: both eyes laid out as one image and letterboxed whole, in cross-eyed order (the right eye's image on the left), since cropping would cut each eye's outer edge differently and spoil the fusion. F10 toggles between the pair and the last single eye.

The debug UI's flat overlay is rendered into a window-sized texture (`hud::mirror_overlay`) and composited over the whole back buffer, so it lands on the window at 1:1 whatever the render resolution. With the floating panel on, the mirror composites the panel texture instead. A draw or present fault turns `vr.mirror` off for the session, and the window keeps its last frame. A recording's video normally streams the eye views from inside the game; with the in-process capture off it records the mirror window from the screen ([capture.md](capture.md)).

## Controls and settings

F7 asks for a recenter and F10 toggles the mirror's stereo pair (`payload/src/hooks/wndproc.rs`, which also binds F1 and F2 for session recordings and take markers, F4 for the clean view, F5 for eject, F6 for egui input capture, F8 for the floating panel, F9 for a profiler capture, and F12 for a screenshot). The debug UI's Runtime feature, under Rendering, has the switches below and a readout of the runtime name, the head pose source, and the hidden-area mask. The session state is the Status page's `VR session` row.

The settings are `vr.*` (`VrConfig`), all on by default except where noted:

- `enabled`: bring up the session. Off tears a live runtime down.
- `resolution_scale` and `world_scale`: the per-eye size on the runtime's recommendation, and metres of head motion per engine unit, both `1.0`.
- `loader_path`: an explicit OpenXR loader, which then has no fallback.
- `native_resolution`: render each eye at the recommended size.
- `own_back_buffer`: render into a mod-owned back buffer so the swapchain stays at the window size ([swapchain-ownership.md](stereo/swapchain-ownership.md)).
- `overlap_render_update`: run the next frame's wait, view location, and render update alongside the previous frame's draw task and tail ([threading.md](threading.md#who-holds-what-across-a-frame), [performance.md](performance/performance.md#what-limits-the-frame)).
- `large_cache_cores`: keep the game's threads on the cores of the largest L3 on a processor whose L3 caches differ (`jc3vr_core::cpu_placement`, [performance.md](performance/performance.md#what-limits-the-frame)).
- `mirror` and `mirror_view`: the desktop mirror and what it shows, `Left` by default.
- `freeze_mode`: a diagnostic that holds the cockpit pose or the whole render camera still, off and never saved (the debug UI's Camera feature, `vr::pose_control`).

## The hidden-area mask

`vr/visibility_mask.rs` fetches each eye's hidden triangle mesh from `XR_KHR_visibility_mask` on the first located frame of a session and again after the runtime's visibility-changed event, and logs each eye's vertex and triangle counts and the hidden share of its field of view. The runtime readout lists the same and draws each eye's field-of-view rectangle with the hidden triangles in it. Nothing is masked with it. The spec places the vertices on the plane `z = -1` of the view's space, so they are tangents of the view angles, in the space the tangents of the located `XrFovf` angles bound, and the hidden share is the triangles' area over that rectangle's.
