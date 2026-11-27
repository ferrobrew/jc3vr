use std::{
    cell::Cell,
    sync::atomic::{AtomicBool, AtomicU32},
};

use detours_macro::detour;
use jc3gi::{
    graphics_engine::{
        device::{Context, Device, DeviceInfo},
        graphics_engine::{DrawThreadTaskParam, GraphicsEngine, HContext_t, HDevice_t},
        render_engine::RenderEngine,
        texture::Texture,
    },
    ui::ui_manager::{GetIUIManager, UIManager},
};
use jc3vr_core::{frame_timing, frame_timing::Slot};
use jc3vr_view::{
    hooks::graphics_engine::{
        capture::{EGUI_DEBUG_RENDER_STATE, eye_capture_fresh, mark_eye_capture, previews_visible},
        post_draw,
    },
    stereo::layout::collapse_active,
    vr::{BlitSource, VR_CONFIG},
};
use re_utilities::hook_library::HookLibrary;
use windows::Win32::{
    Graphics::Direct3D11::{D3D11_BOX, ID3D11DeviceContext, ID3D11Texture2D},
    System::Threading::{
        CRITICAL_SECTION, EnterCriticalSection, GetCurrentThreadId, LeaveCriticalSection,
    },
};

use crate::{
    controllers::{overlay::pass as overlay_pass, scope::pass as scope_pass},
    debug::trace::{TraceEvent, TraceState},
    probes,
    probes::scoped,
    screenshot::CaptureSource,
    stereo::single_pass::set_collapse_ui_eye,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GRAPHICS_FLIP_BINDER)
        .with_static_binder(&GRAPHICS_ENGINE_DRAW_BINDER)
        .with_static_binder(&HANDLE_DRAW_THREAD_TASK_BINDER)
        .with_static_binder(&UI_START_RENDER_IMPL_BINDER)
        .with_static_binder(&RENDER_ENGINE_POST_DRAW_BINDER)
        .with_static_binder(&CREATE_RENDER_SETUPS_BINDER)
        .with_static_binder(&DEVICE_RESIZE_BUFFERS_BINDER)
}

/// Logs the OS thread id of a hook's first call, and again if a later call lands on a different
/// thread than the last one logged. Not debug scaffolding: this is permanent instrumentation for
/// `render_engine_post_draw`, `CreateRenderSetups` (both here), and the payload's
/// `hooks::game::game_update_render` (which calls this too, so it is `pub` for exactly that), whose
/// thread identity settles two questions static reasoning could not.
///
/// The first is whether `render_engine_post_draw` runs apart from the payload's `update`, which is
/// what this hook's shared state has to assume. `update` takes the `EguiState` lock and then
/// `jc3vr_view::hooks::graphics_engine::capture::EGUI_DEBUG_RENDER_STATE` inside it, so `render_engine_post_draw` does not hold
/// `EGUI_DEBUG_RENDER_STATE` across the registered post-draw layers and passes, of which the debug
/// panel and the mirror overlay take the `EguiState` lock: it clones the captures under a brief lock, as everywhere
/// else. Two blocking mutexes in opposite orders deadlock if and only if the two sides run on
/// different threads.
///
/// The second is whether [`jc3vr_view::vr::back_buffer`]'s safety argument holds: it is built on
/// `CreateRenderSetups` running on the render thread under the drained idle context while the
/// game-thread hooks transition its `BACKING` mutex from elsewhere.
///
/// `target: "thread_identity"` and the `hook`/`thread_id` field names are shared across all three call
/// sites so the logs can be compared directly across a session.
///
/// The first call logs at info and later changes at debug (`RUST_LOG=thread_identity=debug`): the
/// draw task runs on whichever CPU fragment worker picks it up, so `render_engine_post_draw` changes
/// thread on most dispatches, and logging each change at info would write some eighty synchronous
/// log lines a second for the whole session.
pub fn log_hook_thread(hook: &'static str, last_thread_id: &AtomicU32) {
    // SAFETY: `GetCurrentThreadId` reads only the calling thread's own TEB entry; it has no
    // preconditions and cannot fail.
    let thread_id = unsafe { GetCurrentThreadId() };
    // Relaxed and diagnostic-only: if the same hook is genuinely re-entered concurrently from two
    // threads, both may see the prior value as unset and both log a "first call" line. That is itself
    // evidence the hook is not confined to one thread, which is the fact this exists to surface.
    let previous = last_thread_id.swap(thread_id, std::sync::atomic::Ordering::Relaxed);
    if previous == 0 {
        tracing::info!(target: "thread_identity", hook, thread_id, "hook entry thread (first call)");
    } else if previous != thread_id {
        tracing::debug!(
            target: "thread_identity",
            hook,
            thread_id,
            previous_thread_id = previous,
            "hook entry thread changed",
        );
    }
}

/// Keep the DXGI swapchain at the window size while the engine resizes everything else.
///
/// While the mod owns the back buffer, this is a **substitute** for the device-level
/// `ResizeBuffers` rather than a suppression of it: `ApplyResize` reads the new size back out of
/// `device->m_DeviceInfo` immediately afterwards and feeds it to `CreateRenderSetups` and to every
/// registered resize callback, so a plain no-op would leave the whole pipeline at the old size.
/// Writing the device info without touching DXGI lets `ApplyResize` run verbatim -- scene targets,
/// pass pools, UI reset, and camera aspect all follow the render size -- while the swapchain stays
/// where it is. See `docs/mod/stereo/swapchain-ownership.md` §5.1.
///
/// `device->m_BackBuffer`'s own dimensions are written only by the real function, so under the
/// substitute they keep reporting the true swapchain size: after this, `m_BackBuffer` means "the
/// window" and `m_BackBufferLinear` means "the render target".
#[detour(address = jc3gi::graphics_engine::device::ResizeBuffers_ADDRESS)]
fn device_resize_buffers(device: *mut HDevice_t, width: u32, height: u32) -> bool {
    // The mod drives a real resize of its own once the substitution is installed, to bring an
    // already-oversized swapchain down to the window; stand aside for that one.
    if !jc3vr_view::vr::back_buffer_owned() || jc3vr_view::vr::resize_substitute_bypassed() {
        return DEVICE_RESIZE_BUFFERS
            .get()
            .unwrap()
            .call(device, width, height);
    }
    // SAFETY: the engine passes its live device; `ApplyResize` runs on the drained idle context.
    let Some(dev) = (unsafe { device.cast::<Device>().as_mut() }) else {
        return DEVICE_RESIZE_BUFFERS
            .get()
            .unwrap()
            .call(device, width, height);
    };
    dev.m_DeviceInfo.m_DisplayWidth = width;
    dev.m_DeviceInfo.m_DisplayHeight = height;
    dev.m_DeviceInfo.m_DisplayRatio = if height > 0 {
        width as f32 / height as f32
    } else {
        0.0
    };
    // Deliberately claims a resize that did not happen to the swapchain. `m_WasResized` is the
    // engine's own "the buffers moved since you last looked" flag, and setting it is what keeps the
    // rest of `ApplyResize` -- the resize callbacks, the pass-owned pools -- behaving exactly as they
    // do on a real resize, which is the point of substituting rather than suppressing. The render
    // targets really did change size; only the DXGI buffers did not.
    dev.m_WasResized = true;
    true
}

/// The back-buffer substitution point: every path that rebuilds the engine's swapchain-derived render
/// setups funnels through `CreateRenderSetups` (its only callers are `InitializeSystem` and
/// `ApplyResize`), so an epilogue here cannot be missed. Inert unless the mod owns the back buffer;
/// see [`jc3vr_view::vr::back_buffer`] and `docs/mod/stereo/swapchain-ownership.md`.
#[detour(
    address = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::CreateRenderSetups_ADDRESS
)]
fn create_render_setups(this: *mut GraphicsEngine, device_info: *const DeviceInfo) -> bool {
    log_hook_thread("CreateRenderSetups", &CREATE_RENDER_SETUPS_THREAD);
    let returned = CREATE_RENDER_SETUPS.get().unwrap().call(this, device_info);
    // Deliberately *not* gated on `returned`: the game never sets a return value. Its C++
    // signature says `bool`, but the compiled code has no final `return 1` -- the function's last
    // instruction before the epilogue is the `CreateRenderSetup` call, so
    // `al` is the low byte of an aligned pointer. Read as a `bool` that is reliably even, and a
    // bit-0 test on it is therefore always false. Gating on it would silently disable the
    // substitution.
    //
    // SAFETY: the engine has just rebuilt its own render setups, on the drained idle context that
    // `CreateRenderSetups` runs under (the `Draw` prologue).
    if let Some(engine) = unsafe { this.as_mut() } {
        unsafe { jc3vr_view::vr::substitute_render_setups(engine) };
    }
    returned
}

/// Last-seen thread id for [`create_render_setups`]. See [`log_hook_thread`].
static CREATE_RENDER_SETUPS_THREAD: AtomicU32 = AtomicU32::new(0);

// `CGame::Draw` clears `m_DrawScene` while a static-background full-screen UI is up (pause / map), so
// the draw thread renders only the UI and clears the eye to transparent -- a black void behind the
// floating panel in VR. Force the 3D scene to keep rendering during gameplay menus so the
// frozen-but-head-trackable world stays visible behind the panel. Gated to E_GAME_RUN + a static
// background so loading screens, the frontend, and full-screen videos are untouched. See issue #7.
#[detour(address = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::Draw_ADDRESS)]
fn graphics_engine_draw(graphics_engine: *mut GraphicsEngine, dt: f32) {
    // SAFETY: runs inside `CGame::Draw` (which just set the flag) before the draw is dispatched, on
    // the render thread; `graphics_engine` is the live engine and `GetIUIManager` the live UI.
    unsafe {
        if jc3vr_core::hooks::in_gameplay()
            && let Some(ge) = graphics_engine.as_mut()
            && let Some(ui) = GetIUIManager().as_ref()
            && ui.IsUsingStaticBackGround()
        {
            ge.m_DrawScene = true;
        }
        // The prologue hands the draw task this dispatch; its per-frame values are the ones the
        // game thread prepared for this frame.
        jc3vr_core::render_frame::note_dispatch();
        GRAPHICS_ENGINE_DRAW
            .get()
            .unwrap()
            .call(graphics_engine, dt);
    }
}

#[detour(address = GraphicsEngine::HandleDrawThreadTask_ADDRESS)]
fn handle_draw_thread_task(engine: *mut GraphicsEngine, param: *mut DrawThreadTaskParam) -> u32 {
    scoped(
        probes::armed_for_benchmark(),
        |probes, body| probes.draw_task_scope(body),
        || {
            // The task records the frame its dispatch drew, whatever the game thread prepares
            // meanwhile.
            let _frame = jc3vr_core::render_frame::pin_dispatch();
            let _timing = frame_timing::scope(Slot::DrawTask);
            let _dispatch = UiDispatchGuard::enter(engine);
            HANDLE_DRAW_THREAD_TASK.get().unwrap().call(engine, param)
        },
    )
}

#[detour(address = UIManager::StartRenderImpl_ADDRESS)]
fn ui_start_render_impl(manager: *mut UIManager, context: *const HContext_t) {
    let main_context = UI_DISPATCH.get().and_then(|dispatch| unsafe {
        if manager.is_null() || manager != GetIUIManager() {
            return None;
        }
        let engine = dispatch.engine.as_ref()?;
        let main_context = engine.m_Device.as_ref()?.m_Context;
        if maintain_ui_redirect(dispatch.engine, manager, main_context) {
            UI_DISPATCH.set(Some(UiDispatch {
                maintenance_ran: true,
                ..dispatch
            }));
        }
        (!main_context.is_null()).then_some(main_context)
    });

    UI_START_RENDER_IMPL.get().unwrap().call(manager, context);

    if let Some(main_context) = main_context {
        UI_DISPATCH.set(UI_DISPATCH.get().map(|dispatch| UiDispatch {
            pending: Some(PendingUiJob {
                manager,
                context: main_context,
            }),
            ..dispatch
        }));
    }
}

#[derive(Clone, Copy)]
struct UiDispatch {
    engine: *mut GraphicsEngine,
    maintenance_ran: bool,
    pending: Option<PendingUiJob>,
}

#[derive(Clone, Copy)]
struct PendingUiJob {
    manager: *mut UIManager,
    context: *mut Context,
}

thread_local! {
    static UI_DISPATCH: Cell<Option<UiDispatch>> = const { Cell::new(None) };
}

struct UiDispatchGuard(Option<UiDispatch>);

impl UiDispatchGuard {
    fn enter(engine: *mut GraphicsEngine) -> Self {
        Self(UI_DISPATCH.replace(Some(UiDispatch {
            engine,
            maintenance_ran: false,
            pending: None,
        })))
    }
}

impl Drop for UiDispatchGuard {
    fn drop(&mut self) {
        UI_DISPATCH.set(self.0);
    }
}

unsafe fn maintain_ui_redirect(
    engine: *mut GraphicsEngine,
    manager: *mut UIManager,
    context: *mut Context,
) -> bool {
    unsafe {
        let Some(engine) = engine.as_ref() else {
            return false;
        };
        let Some(device) = engine.m_Device.as_ref() else {
            return false;
        };
        let Some(back_buffer) = device.m_BackBuffer.as_ref() else {
            return false;
        };
        let Some(context) = context.as_ref() else {
            return false;
        };
        if manager.is_null()
            || (*manager).m_DeferredRenderLock.is_null()
            || context.m_Mutex.is_null()
        {
            return false;
        }
        let _ui_lock = UiCriticalSectionGuard::enter((*manager).m_DeferredRenderLock);
        let _context_lock = UiCriticalSectionGuard::enter(context.m_Mutex);
        post_draw::maintain_ui_redirect(
            device,
            u32::from(back_buffer.m_Width),
            u32::from(back_buffer.m_Height),
        );
        true
    }
}

struct UiCriticalSectionGuard(*mut CRITICAL_SECTION);

impl UiCriticalSectionGuard {
    unsafe fn enter(lock: *mut CRITICAL_SECTION) -> Self {
        unsafe { EnterCriticalSection(lock) };
        Self(lock)
    }
}

impl Drop for UiCriticalSectionGuard {
    fn drop(&mut self) {
        unsafe { LeaveCriticalSection(self.0) };
    }
}

pub static BLOCK_FLIP: AtomicBool = AtomicBool::new(false);

#[detour(address = jc3gi::graphics_engine::graphics_engine::graphics_flip_ADDRESS)]
fn graphics_flip(device: *mut Device) -> u64 {
    let blocked = BLOCK_FLIP.load(std::sync::atomic::Ordering::Relaxed);
    TraceState::record_eye(TraceEvent::Flip { blocked });
    tracing::trace!(target: "frameloop", "graphics_flip: entry (blocked={blocked})");
    if blocked {
        tracing::trace!(target: "frameloop", "graphics_flip: blocked, returning");
        return 0;
    }

    post_draw::draw_desktop_overlay();
    tracing::trace!(target: "frameloop", "graphics_flip: calling original");
    let r = GRAPHICS_FLIP.get().unwrap().call(device);
    tracing::trace!(target: "frameloop", "graphics_flip: original returned");
    r
}

#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::PostDraw_ADDRESS)]
fn render_engine_post_draw(render_engine: *mut RenderEngine, context: *mut Context) -> u64 {
    log_hook_thread("render_engine_post_draw", &RENDER_ENGINE_POST_DRAW_THREAD);
    // The dispatch's config pin ends with this seam, on every return path.
    let _dispatch_end = crate::stereo::single_pass::end_dispatch();
    // The last render seam of the dispatch: bracket PostDraw on both timelines, then close the GPU
    // dispatch opened in `render_pass::pre_draw` (ending the disjoint query and reading back the
    // dispatches the GPU has since finished). `Context` and `HContext_t` are the same handle.
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("RenderEngine::PostDraw");
    let result = {
        #[cfg(feature = "profiler")]
        // SAFETY: `context` is the live immediate-context handle for this dispatch.
        let _gpu = unsafe {
            crate::profiler::gpu::seam(context.cast(), crate::profiler::gpu::GpuSeam::PostDraw)
        };
        RENDER_ENGINE_POST_DRAW
            .get()
            .unwrap()
            .call(render_engine, context)
    };
    {
        // The mod's own work on the eyes' images is bracketed as a seam of its own, so the GPU
        // dispatch closes after it and its cost is part of the dispatch's span.
        #[cfg(feature = "profiler")]
        // SAFETY: `context` is the live immediate-context handle for this dispatch.
        let _gpu = unsafe {
            crate::profiler::gpu::seam(context.cast(), crate::profiler::gpu::GpuSeam::ModPostDraw)
        };
        mod_post_draw(context);
    }
    #[cfg(feature = "profiler")]
    // SAFETY: `context` is the live immediate-context handle for this dispatch.
    unsafe {
        crate::profiler::gpu::end_dispatch(context.cast())
    };
    result
}

/// The mod's work after the engine's `PostDraw`: the HUD and the world-locked overlays drawn into
/// the eyes' images, the eye copies or direct swapchain blits, and the captures that read them.
fn mod_post_draw(context: *mut Context) {
    TraceState::record_eye(TraceEvent::PostDraw);
    let _timing = frame_timing::scope(Slot::PostDraw);

    unsafe {
        let dispatch = UI_DISPATCH.get();
        let engine = match dispatch {
            Some(dispatch) => dispatch.engine,
            None => GraphicsEngine::get().map_or(std::ptr::null_mut(), |engine| engine),
        };
        let Some(graphics_engine) = engine.as_ref() else {
            return;
        };
        if context.is_null() {
            return;
        }

        let main_context = graphics_engine
            .m_Device
            .as_ref()
            .map_or(std::ptr::null_mut(), |device| device.m_Context);
        if !dispatch.is_some_and(|dispatch| dispatch.maintenance_ran)
            && maintain_ui_redirect(engine, GetIUIManager(), main_context)
            && let Some(dispatch) = dispatch
        {
            UI_DISPATCH.set(Some(UiDispatch {
                maintenance_ran: true,
                ..dispatch
            }));
        }
        let pending = UI_DISPATCH.get().and_then(|mut dispatch| {
            let pending = dispatch
                .pending
                .filter(|job| job.context == context && job.context == main_context)?;
            dispatch.pending = None;
            UI_DISPATCH.set(Some(dispatch));
            Some(pending)
        });
        if let Some(pending) = pending
            && post_draw::ui_redirect_applied()
        {
            (*pending.manager).SyncRender();
            (*pending.manager).Submit();
        }

        let context = &mut *context;

        // Clone (AddRef) the per-eye capture textures under a brief EGUI lock, released before the
        // context work below, mirroring the lock discipline of the other consumers of these captures
        // (`vr::blit::submit` and `vr::mirror::present_mirror_inner`). Held across the body instead,
        // this deadlocks: the debug panel and the mirror overlay drawn below take the `EguiState`
        // lock, while the payload's `update` takes `EguiState` and then this one inside it. One
        // acquisition, not one per eye, so the collapse's two half-copies cannot straddle a resize
        // that rebuilds the pair.
        let eye_textures: [Option<ID3D11Texture2D>; 2] = {
            let lock = EGUI_DEBUG_RENDER_STATE.lock();
            [lock.texture(0).cloned(), lock.texture(1).cloned()]
        };
        let index = jc3vr_view::stereo::draw_index();
        // The frame's last dispatch: after it, every eye image of the frame is final.
        let last_dispatch = jc3vr_view::stereo::last_dispatch();
        // The dispatch ticks (the world-space overlay's producers and the scope lens's plan, each
        // acting once per frame), outside the context lock: they read the controller subsystems'
        // published state under those subsystems' own locks.
        post_draw::run_dispatch_ticks(index);

        EnterCriticalSection(context.m_Mutex);

        // Draw the floating HUD quad onto this eye's back buffer before it is captured/presented, so it
        // shows in both the preview and the final image. The HUD render target is also cleared so the
        // next frame starts clean rather than accumulating past frames.
        if let (Some(device), Some(back_buffer)) = (
            graphics_engine.m_Device.as_ref(),
            graphics_engine.m_BackBufferLinear.as_ref(),
        ) {
            // The scope lens reads the eyes' images as the scene left them, before the overlay.
            scope_pass::capture(&context.m_Context, device, back_buffer, index);
            // The eye layers (`draw_eye`): the world-space overlay, the scope lens over it, then
            // the layers registered above in their declared order (the HUD's panels). Under
            // collapse the render camera is centered and the target is double-wide, so draw the
            // world-locked layers once per eye into each half with that eye's own VP (see
            // `single_pass::collapse_ui_eye_override`); otherwise the per-dispatch single draw
            // carries the eye implicitly.
            #[cfg(feature = "profiler")]
            let overlays_gpu = crate::profiler::gpu::detail_interval("mod: HUD and overlays");
            if collapse_active() {
                for eye in 0..2 {
                    set_collapse_ui_eye(Some(eye));
                    draw_eye(&context.m_Context, device, back_buffer, eye);
                }
                set_collapse_ui_eye(None);
            } else {
                draw_eye(&context.m_Context, device, back_buffer, index);
            }
            #[cfg(feature = "profiler")]
            drop(overlays_gpu);
            // The frame passes, in `post_draw::FramePass` order: the flat mirror overlay rendered
            // offscreen (consuming this frame's egui output) so the desktop mirror can composite it
            // from the deferred frame tail's thread, then the learned body estimator's GPU pass.
            //
            // They run on the frame's last dispatch rather than eye 0's: with the tail
            // deferred, the game thread waits on eye 0's walk but not on the last one, which the
            // tail drains before it composites the mirror. Nothing per-eye reads the overlay, and
            // the floating panel, which consumes the same egui output, is off whenever it renders.
            if last_dispatch {
                post_draw::run_frame_passes(&context.m_Context, device, back_buffer);
            }
        }

        // Final back buffer for this eye. (The HDR scene / MainColor is captured earlier, at the
        // start of the post chain, before it gets read and recycled -- see capture_main_color.)
        //
        // On the direct route the eye is blitted straight into its swapchain slice here, and the
        // copy into its capture is made only for a reader of the capture (see `jc3vr_view::vr::blit`).
        // Either way this is the image the copy would have captured: everything the eye shows is
        // drawn by now, and nothing the engine draws after this hook reaches the headset.
        if let Some(src) = graphics_engine.m_BackBufferLinear.as_ref() {
            #[cfg(feature = "profiler")]
            let copies_gpu = crate::profiler::gpu::detail_interval("mod: eye copies and blits");
            if collapse_active() {
                // The collapsed single walk rendered both eyes into this one (viewport-split) back
                // buffer -- left half eye 0, right half eye 1. On the direct route each half is
                // blitted straight into its swapchain slice, and copied into its eye texture only
                // for a reader of the capture; otherwise each half is copied into its eye texture
                // for the submit to blit. The collapsed back buffer is double-wide, so each
                // `half_w`-wide half fills its full-resolution eye texture.
                let half_w = u32::from(src.m_Width) / 2;
                let height = u32::from(src.m_Height);
                for eye in 0..2u32 {
                    let blitted = graphics_engine.m_Device.as_ref().is_some_and(|device| {
                        jc3vr_view::vr::blit_direct(
                            device,
                            &context.m_Context,
                            &src.m_Texture,
                            eye as usize,
                            BlitSource::Half(eye),
                        )
                    });
                    let dst = eye_textures[eye as usize]
                        .as_ref()
                        .filter(|_| !blitted || eye_capture_wanted(eye as usize));
                    mark_eye_capture(eye as usize, dst.is_some());
                    if let Some(probes) = probes::armed() {
                        probes.note_eye(eye as usize, dst.is_some());
                    }
                    if let Some(dst) = dst {
                        let region = D3D11_BOX {
                            left: eye * half_w,
                            top: 0,
                            front: 0,
                            right: (eye + 1) * half_w,
                            bottom: height,
                            back: 1,
                        };
                        context.m_Context.CopySubresourceRegion(
                            dst,
                            0,
                            0,
                            0,
                            0,
                            &src.m_Texture,
                            0,
                            Some(&region),
                        );
                    }
                }
            } else {
                let blitted = graphics_engine.m_Device.as_ref().is_some_and(|device| {
                    jc3vr_view::vr::blit_direct(
                        device,
                        &context.m_Context,
                        &src.m_Texture,
                        index,
                        BlitSource::Whole,
                    )
                });
                let copied = match eye_textures.get(index).and_then(Option::as_ref) {
                    Some(dst) if !blitted || eye_capture_wanted(index) => {
                        context.m_Context.CopyResource(dst, &src.m_Texture);
                        true
                    }
                    _ => false,
                };
                mark_eye_capture(index, copied);
                if let Some(probes) = probes::armed() {
                    probes.note_eye(index, copied);
                }
            }
            #[cfg(feature = "profiler")]
            drop(copies_gpu);

            // The recorder's sidecar line for the frame, on its last dispatch, when every eye
            // image of the frame is final, and a streamed take's eye-view capture, which needs both
            // eyes' captures to hold the frame. Both are no-ops unless recording.
            if last_dispatch && let Some(recorder) = post_draw::eye_frame_recorder() {
                let frame = (recorder.frame_rendered)();
                match (graphics_engine.m_Device.as_ref(), &eye_textures) {
                    (Some(device), [Some(left), Some(right)])
                        if eye_capture_fresh(0) && eye_capture_fresh(1) =>
                    {
                        (recorder.capture_frame)(
                            &device.m_Device,
                            &context.m_Context,
                            [left, right],
                            frame,
                        );
                    }
                    _ => (recorder.release_capture)(),
                }
            }

            if last_dispatch
                && let Some(probes) = probes::armed()
                && let Some(device) = graphics_engine.m_Device.as_ref()
            {
                if collapse_active() || jc3vr_view::stereo::dispatch::selected_eye().is_some() {
                    probes.capture_frame(
                        &device.m_Device,
                        &context.m_Context,
                        CaptureSource::Full(&src.m_Texture),
                    );
                } else if let [Some(left), Some(right)] = &eye_textures {
                    probes.capture_frame(
                        &device.m_Device,
                        &context.m_Context,
                        CaptureSource::Eyes([left, right]),
                    );
                }
            }

            // Service an F12 screenshot request with a source that carries both eyes. Under
            // collapse the linear back buffer already holds the two eye-halves side by side; in
            // two-pass stereo it only holds the eye that just rendered, so the per-eye capture
            // textures (copied just above) are stitched instead, after the last dispatch,
            // when both hold the current frame. An isolated mode uses its one physical eye's buffer.
            if let Some(device) = graphics_engine.m_Device.as_ref() {
                let two_pass_stereo = jc3vr_view::stereo::active() && !collapse_active();
                let source =
                    if !two_pass_stereo || jc3vr_view::stereo::dispatch::selected_eye().is_some() {
                        Some(CaptureSource::Full(&src.m_Texture))
                    } else if !last_dispatch {
                        None
                    } else if let [Some(left), Some(right)] = &eye_textures {
                        // Both captures must hold this frame; one the direct route skipped leaves the
                        // request pending, and the next frame copies both for it.
                        (eye_capture_fresh(0) && eye_capture_fresh(1))
                            .then_some(CaptureSource::Eyes([left, right]))
                    } else {
                        // A missing eye capture (the pair is being rebuilt across a resize) still
                        // yields a screenshot -- of this eye's back buffer -- rather than none.
                        Some(CaptureSource::Full(&src.m_Texture))
                    };
                if let Some(source) = source {
                    crate::screenshot::capture_if_requested(
                        &device.m_Device,
                        &context.m_Context,
                        source,
                    );
                }
            }
        }

        LeaveCriticalSection(context.m_Mutex);
    }
}

/// Draw the eye layers onto eye `eye` of the dispatch's final back buffer: the render crate's own
/// world-space overlay and scope lens over it, then the layers registered in the post-draw (the
/// HUD's panels) on top.
fn draw_eye(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    overlay_pass::draw(context, device, target, eye);
    scope_pass::draw(context, device, target, eye);
    post_draw::draw_eye_layers(context, device, target, eye);
}

/// Whether anything besides the headset reads eye `eye`'s capture this frame, so its post-draw must
/// copy the eye into it even when the eye went straight to the swapchain: the desktop mirror's eyes,
/// a pending screenshot, the Previews tab, and a recording that streams the eye views.
fn eye_capture_wanted(eye: usize) -> bool {
    VR_CONFIG.query(|c| c.mirror && c.mirror_view.shows_eye(eye))
        || crate::screenshot::is_requested()
        || probes::armed().is_some_and(|probes| probes.wants_eyes())
        || previews_visible()
        || post_draw::recorder_wants_eye_captures()
}

/// Last-seen thread id for [`render_engine_post_draw`]. See [`log_hook_thread`].
static RENDER_ENGINE_POST_DRAW_THREAD: AtomicU32 = AtomicU32::new(0);
