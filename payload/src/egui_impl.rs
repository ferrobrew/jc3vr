pub(crate) mod desktop_input;

use anyhow::Context as _;
use egui::CursorIcon as CI;
use jc3gi::{
    graphics_engine::graphics_engine::{ActiveCursor, GraphicsEngine, get_graphics_params},
    input::input_device_manager::InputDeviceManager,
};
use parking_lot::{Mutex, MutexGuard};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11RenderTargetView},
};

pub(crate) use crate::egui_impl::desktop_input::{
    consume_desktop_shortcut, desktop_input_observation, observe_desktop_message,
};
use crate::{
    config::Config,
    hud::{egui_panel, pointer},
    ui::capture::{
        CapturePolicy, CaptureUpdate, DeviceEffect, InputObservation, OwnershipSnapshot, PanelMode,
        PanelRequest,
    },
};

/// Input-thread observations before either editing or gameplay receives the sample.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DebugUiInput {
    pub focused: bool,
    pub tracking_ready: bool,
    /// Includes all source episodes, panel shortcuts, keys, buttons, composition, and scroll.
    pub controls_neutral: bool,
}

/// The latest policy observation; reading it never takes the renderer lock.
pub(crate) fn debug_ui_capture() -> Option<OwnershipSnapshot> {
    *CAPTURE_SNAPSHOT.lock()
}

/// Queue a UI request without re-entering the renderer lock from an egui callback.
pub(crate) fn request_debug_ui(request: PanelRequest) {
    REQUESTS.lock().push(request);
}

/// Step capture before mode detection, pointer routing, and gameplay output on the input thread.
pub(crate) fn update_debug_ui_capture(input: DebugUiInput) -> Option<CaptureUpdate> {
    let mut state = EguiState::get();
    state.as_mut().map(|state| {
        state.apply_pending_raw_loss();
        let input = state.capture.input_owner_sample(input);
        let update = state.update_capture(input);
        state.capture.notify_input_owner(update)
    })
}

pub struct EguiState {
    start_time: std::time::Instant,
    egui_context: egui::Context,
    pub egui_renderer: egui_directx11::Renderer,
    renderer_output: Option<egui_directx11::RendererOutput>,
    capture: EguiCaptureAdapter,
    saved_cursor: Option<ActiveCursor>,
    events: Vec<egui::Event>,
    /// When `Some`, the egui UI is being driven as the VR floating panel (issue #24): [`run`] sizes
    /// the layout to this panel-texture size instead of the game window, and [`render_to`] paints it
    /// into the panel render target. Set from the game thread each frame before [`run`]; `None` is
    /// the flat back-buffer overlay (the default, unchanged) behavior.
    panel: Option<(u32, u32)>,
}
static STATE: Mutex<Option<EguiState>> = Mutex::new(None);
static CAPTURE_SNAPSHOT: Mutex<Option<OwnershipSnapshot>> = Mutex::new(None);
static REQUESTS: Mutex<Vec<PanelRequest>> = Mutex::new(Vec::new());

/// The renderer-free part of the live adapter, also exercised with a real egui context.
#[derive(Default)]
struct EguiCaptureAdapter {
    policy: CapturePolicy,
    snapshot: Option<OwnershipSnapshot>,
    cancellation_pass_pending: bool,
    input_notification_pending: bool,
    focus_loss_observed: u64,
}

impl EguiCaptureAdapter {
    fn take_focus_loss(&mut self, generation: u64) -> bool {
        let lost = generation != self.focus_loss_observed;
        self.focus_loss_observed = generation;
        lost
    }

    fn update(
        &mut self,
        request: Option<PanelRequest>,
        observation: InputObservation,
        context: &egui::Context,
        events: &mut Vec<egui::Event>,
    ) -> CaptureUpdate {
        let update = self.policy.update(request, observation);
        if update.effects.clear_pending_events || update.effects.cancel_pointer_and_drag {
            self.cancel_input(context, events);
        }
        self.snapshot = Some(update.snapshot);
        update
    }

    fn apply_raw_loss(
        &mut self,
        generation: u64,
        observation: InputObservation,
        context: &egui::Context,
        events: &mut Vec<egui::Event>,
    ) -> Option<CaptureUpdate> {
        self.take_focus_loss(generation).then(|| {
            let mut update = self.update(
                None,
                InputObservation {
                    focused: false,
                    controls_neutral: false,
                    ..observation
                },
                context,
                events,
            );
            if !update.effects.clear_pending_events {
                self.cancel_input(context, events);
            }
            update.effects.clear_pending_events = true;
            update.effects.cancel_pointer_and_drag = true;
            update.effects.consume_until_neutral = true;
            update
        })
    }

    fn cancel_input(&mut self, context: &egui::Context, events: &mut Vec<egui::Event>) {
        events.clear();
        clear_remembered_input(context);
        self.cancellation_pass_pending = true;
        self.input_notification_pending = true;
    }

    fn validate_device_write(
        &mut self,
        update: &mut CaptureUpdate,
        observation: InputObservation,
        context: &egui::Context,
        events: &mut Vec<egui::Event>,
        mut focus_now: impl FnMut() -> (bool, u64),
    ) -> Option<bool> {
        let (focused, generation) = focus_now();
        if !focused || generation != self.focus_loss_observed {
            let previous = *update;
            let lost = InputObservation {
                focused: false,
                controls_neutral: false,
                ..observation
            };
            *update = self
                .apply_raw_loss(generation, lost, context, events)
                .unwrap_or_else(|| self.update(None, lost, context, events));
            update.effects.clear_pending_events |= previous.effects.clear_pending_events;
            update.effects.cancel_pointer_and_drag |= previous.effects.cancel_pointer_and_drag;
            update.effects.consume_until_neutral |= previous.effects.consume_until_neutral;
            if update.effects.game_device == DeviceEffect::Unchanged
                && previous.effects.game_device == DeviceEffect::SetEnabled(false)
            {
                update.effects.game_device = DeviceEffect::SetEnabled(false);
                update.snapshot.intended_game_device_enabled = false;
                self.snapshot = Some(update.snapshot);
            }
            return device_write(update.effects.game_device, false);
        }
        device_write(update.effects.game_device, observation.focused)
    }

    fn input_owner_sample(&self, input: DebugUiInput) -> DebugUiInput {
        DebugUiInput {
            controls_neutral: input.controls_neutral && !self.input_notification_pending,
            ..input
        }
    }

    fn surface_request(&self, visible: bool) -> Option<PanelRequest> {
        (!visible
            && self
                .snapshot
                .is_some_and(|snapshot| snapshot.mode != PanelMode::Hidden))
        .then_some(PanelRequest::Hide)
    }

    fn notify_input_owner(&mut self, mut update: CaptureUpdate) -> CaptureUpdate {
        if std::mem::take(&mut self.input_notification_pending) {
            update.effects.clear_pending_events = true;
            update.effects.cancel_pointer_and_drag = true;
            update.effects.consume_until_neutral = true;
        }
        update
    }

    fn take_cancellation_input(&mut self, input: &egui::RawInput) -> Option<egui::RawInput> {
        if !std::mem::take(&mut self.cancellation_pass_pending) {
            return None;
        }
        let mut cancellation = input.clone();
        cancellation.events.clear();
        cancellation.modifiers = egui::Modifiers::default();
        cancellation.focused = false;
        Some(cancellation)
    }

    fn prepare_input(&self, input: &mut egui::RawInput) {
        if let Some(snapshot) = self.snapshot {
            snapshot.filter_egui_input(input);
        } else {
            input.events.clear();
            input.modifiers = egui::Modifiers::default();
            input.focused = false;
        }
    }
}

/// Whether a requested device write is permitted by the application's focus ownership.
fn device_write(effect: DeviceEffect, focused: bool) -> Option<bool> {
    match effect {
        DeviceEffect::SetEnabled(false) => Some(false),
        DeviceEffect::SetEnabled(true) if focused => Some(true),
        DeviceEffect::Unchanged | DeviceEffect::SetEnabled(true) => None,
    }
}

fn cancel_external_pointer() {
    pointer::reset_egui_pointer();
    egui_panel::end_grab();
    crate::hooks::ui::cancel_pointer_on_game_thread();
}

fn native_game_focus() -> bool {
    // SAFETY: the graphics HWND is read on its game/window thread and is not dereferenced here.
    let hwnd = unsafe { get_graphics_params().m_Hwnd };
    desktop_input::native_focus(hwnd)
}

fn clear_remembered_input(context: &egui::Context) {
    context.input_mut(|input| *input = egui::InputState::default());
    context.stop_dragging();
    context.memory_mut(|memory| {
        if let Some(focused) = memory.focused() {
            memory.surrender_focus(focused);
        }
    });
}

impl EguiState {
    fn new() -> anyhow::Result<Self> {
        let device = unsafe {
            &GraphicsEngine::get()
                .context("Failed to get graphics engine")?
                .m_Device
                .as_mut()
                .context("Failed to get device")?
                .m_Device
        };
        Ok(Self {
            start_time: std::time::Instant::now(),
            egui_context: egui::Context::default(),
            egui_renderer: egui_directx11::Renderer::new(device)
                .context("Failed to create egui renderer")?,
            renderer_output: None,
            capture: EguiCaptureAdapter::default(),
            saved_cursor: None,
            events: Vec::new(),
            panel: None,
        })
    }

    pub fn install() -> anyhow::Result<()> {
        desktop_input::reset_desktop_input();
        STATE.lock().replace(Self::new()?);
        tracing::info!("Initialized egui");
        Ok(())
    }

    pub fn uninstall() {
        if let Some(mut state) = STATE.lock().take() {
            state.request_capture(PanelRequest::Hide);
        }
        *CAPTURE_SNAPSHOT.lock() = None;
        REQUESTS.lock().clear();
        desktop_input::reset_desktop_input();
        tracing::info!("Uninitialized egui");
    }

    pub fn get() -> MutexGuard<'static, Option<Self>> {
        STATE.lock()
    }

    /// Select the panel (VR) or flat (desktop) layout mode for the next [`run`]. `Some(size)` sizes
    /// the egui layout to a panel texture of that size (VR floating panel, issue #24); `None` sizes it
    /// to the game window (the flat back-buffer overlay). Call from the game thread before [`run`].
    pub fn set_panel_mode(&mut self, size: Option<(u32, u32)>) {
        let desktop_visible = crate::hud::mirror_overlay::is_active()
            || (crate::vr::render_params(0).is_none()
                && Config::lock_query(|config| config.hud.flat_overlay_visible)
                && !crate::capture::is_active()
                && !crate::director::cinematic::active());
        self.reconcile_surfaces(size.is_some() || desktop_visible);
        if size.is_some()
            && self.panel.is_none()
            && self
                .capture
                .snapshot
                .is_none_or(|snapshot| snapshot.mode == PanelMode::Hidden)
        {
            self.request_capture(PanelRequest::Open);
        }
        self.panel = size;
    }

    fn reconcile_surfaces(&mut self, visible: bool) {
        if let Some(request) = self.capture.surface_request(visible) {
            self.request_capture(request);
        }
    }

    /// F4 changes the desktop surface only; a live VR panel retains its own ownership.
    pub(crate) fn desktop_visibility_changed(&mut self, visible: bool) {
        if !egui_panel::is_active() {
            self.request_capture(PanelRequest::Hide);
            if visible {
                self.request_capture(PanelRequest::Open);
            }
        }
    }

    /// Append externally-sourced input events (the VR panel's virtual pointer, issue #24) to the next
    /// [`run`]'s event queue. Call from the game thread before [`run`].
    pub fn push_events(&mut self, events: impl IntoIterator<Item = egui::Event>) {
        if self.is_input_captured() {
            self.events.extend(events);
        } else {
            pointer::reset_egui_pointer();
        }
    }

    pub fn run(&mut self, mut callback: impl FnMut(&egui::Context, &mut egui_directx11::Renderer)) {
        self.apply_pending_raw_loss();
        // The layout space, decided by which surface will carry this frame's UI. The renderer takes
        // its frame size from the render target and maps points to pixels by `pixels_per_point`, so
        // `layout * pixels_per_point` must be that target's pixel size or the UI does not fill it.
        //
        // Every surface is already sized in the pixels the UI is measured in, so all three lay out one
        // point per pixel and window-pixel mouse coordinates need no remapping: the VR floating
        // panel's own texture (whose pointer is re-sourced anyway), the desktop mirror's overlay
        // texture, which is window-sized and composited across the whole back buffer so the present's
        // buffer→window stretch lands it 1:1, and the engine back buffer, which the engine keeps at
        // the window's size outside of the mod's own VR resize.
        let screen_size = match self.panel {
            Some((width, height)) => (width as f32, height as f32),
            // Window pixels, so the UI is independent of the render resolution. The live rect, for
            // the reason given in `crate::hud::mirror_overlay` -- the cursor mapping's cached copy
            // only refreshes on mouse movement, and this layout has to match the texture the overlay
            // is rendered into. Before the window rect is known at all, the overlay texture falls
            // back to the back buffer's size, so the layout follows it there.
            None if crate::hud::mirror_overlay::is_active() => crate::vr::window::client_size()
                .or_else(crate::hud::cursor::client_size)
                .map(|(w, h)| (w as f32, h as f32))
                .unwrap_or_else(flat_buffer_size),
            None => flat_buffer_size(),
        };
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_max(
                Default::default(),
                egui::Pos2::new(screen_size.0, screen_size.1),
            )),
            time: Some(self.start_time.elapsed().as_secs_f64()),
            focused: self.is_input_captured(),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        self.capture.prepare_input(&mut input);
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(1.0);
        let cancellation_output = self.capture.take_cancellation_input(&input).map(|input| {
            self.egui_context.run_ui(input, |ui| {
                callback(ui.ctx(), &mut self.egui_renderer);
            })
        });
        let egui_output = self
            .egui_context
            .run_ui(input, |ui| callback(ui.ctx(), &mut self.egui_renderer));
        let (mut renderer_output, platform_output, _) = egui_directx11::split_output(egui_output);
        if let Some(cancellation_output) = cancellation_output {
            let (cancelled, _, _) = egui_directx11::split_output(cancellation_output);
            let mut delta = cancelled.textures_delta;
            delta.append(renderer_output.textures_delta);
            renderer_output.textures_delta = delta;
        }

        // Carry a still-unrendered frame's texture delta forward instead of dropping it. `run()` (game
        // thread) can outpace `render_to()` (render thread) and overwrite an output that was never
        // painted; egui emits the font atlas `set` only on the frame it (re)builds it, so a dropped
        // frame silently loses the atlas upload and every later frame samples a non-existent
        // `Managed(0)`, blanking all text. Appending the older delta first preserves those uploads (the
        // standard egui skipped-frame handling); the newer geometry still replaces the older.
        if let Some(prev) = self.renderer_output.take() {
            let mut delta = prev.textures_delta;
            delta.append(renderer_output.textures_delta);
            renderer_output.textures_delta = delta;
        }

        if self.is_input_captured()
            && let Some(graphics_engine) = unsafe { GraphicsEngine::get() }
        {
            graphics_engine.m_ActiveCursor = match platform_output.cursor_icon {
                CI::None => ActiveCursor::None,
                CI::AllScroll => ActiveCursor::Cross,
                CI::ResizeHorizontal | CI::ResizeEast | CI::ResizeWest => ActiveCursor::Slider,
                CI::ZoomIn | CI::ZoomOut => ActiveCursor::Zoom,
                _ => ActiveCursor::Arrow,
            };
        }
        self.renderer_output = Some(renderer_output);
        if !REQUESTS.lock().is_empty() {
            let snapshot = self.capture.snapshot;
            self.update_capture(DebugUiInput {
                focused: snapshot.is_some_and(|snapshot| snapshot.focused),
                tracking_ready: snapshot.is_some_and(|snapshot| snapshot.tracking_ready),
                controls_neutral: false,
            });
        }
    }

    pub fn render(&mut self) {
        let Some(renderer_output) = self.renderer_output.take() else {
            return;
        };
        unsafe {
            let Some(device) = GraphicsEngine::get().and_then(|ge| ge.m_Device.as_mut()) else {
                return;
            };
            let Some(context) = device.m_Context.as_mut() else {
                return;
            };
            let Some(backbuffer) = device.m_BackBuffer.as_mut() else {
                return;
            };
            if let Err(e) = self.egui_renderer.render(
                &context.m_Context,
                &backbuffer.m_RTV,
                &self.egui_context,
                renderer_output,
            ) {
                tracing::error!("Failed to render egui: {e:?}");
            }
        }
    }

    /// Render the current frame's egui output into an arbitrary render target (the VR floating panel's
    /// offscreen texture, issue #24) using the supplied context, consuming the single-use
    /// [`renderer_output`](EguiState::renderer_output). The target is cleared to transparent first: the
    /// egui renderer does not clear and early-returns on an empty frame, so without the clear the panel
    /// would retain the previous frame. A no-op when the output has already been consumed, which
    /// leaves the target holding the last output rendered into it: a consumer on a dispatch the game
    /// thread does not wait for can run after the next frame's egui pass and take its output early, and
    /// the next frame's call then finds nothing to render. The caller must hold the engine context
    /// mutex.
    pub fn render_to(&mut self, context: &ID3D11DeviceContext, rtv: &ID3D11RenderTargetView) {
        let Some(renderer_output) = self.renderer_output.take() else {
            return;
        };
        // SAFETY: `rtv` is a live render-target view; `context` is the caller-locked engine context.
        unsafe {
            context.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 0.0]);
        }
        if let Err(e) = self
            .egui_renderer
            .render(context, rtv, &self.egui_context, renderer_output)
        {
            tracing::error!("Failed to render egui panel: {e:?}");
        }
    }

    /// The existing desktop capture key never turns a ReadOnly overlay into an editor.
    pub fn toggle_game_input_capture(&mut self) {
        if egui_panel::is_active() {
            return;
        }
        if !Config::lock_query(|config| config.hud.flat_overlay_visible) {
            self.request_capture(PanelRequest::Hide);
            return;
        }
        let request = match self.capture.snapshot.map(|snapshot| snapshot.mode) {
            Some(PanelMode::Editing) => PanelRequest::AllowGameplay,
            Some(PanelMode::ReadOnly) => return,
            Some(PanelMode::Hidden) | None => PanelRequest::Open,
        };
        self.request_capture(request);
    }

    pub fn is_input_captured(&self) -> bool {
        self.capture
            .snapshot
            .is_some_and(|snapshot| snapshot.edit_capture.any())
    }

    pub(crate) fn request_capture(&mut self, request: PanelRequest) {
        let snapshot = self.capture.snapshot;
        let input = DebugUiInput {
            focused: snapshot.is_some_and(|snapshot| snapshot.focused),
            tracking_ready: snapshot.is_some_and(|snapshot| snapshot.tracking_ready),
            controls_neutral: false,
        };
        self.apply_capture(Some(request), input);
    }

    fn update_capture(&mut self, input: DebugUiInput) -> CaptureUpdate {
        let requests = std::mem::take(&mut *REQUESTS.lock());
        let mut update = None;
        for request in requests {
            update = Some(self.apply_capture(
                Some(request),
                DebugUiInput {
                    controls_neutral: false,
                    ..input
                },
            ));
        }
        update.unwrap_or_else(|| self.apply_capture(None, input))
    }

    fn apply_pending_raw_loss(&mut self) {
        let generation = desktop_input::focus_loss_generation();
        // SAFETY: this boundary runs on the game/window thread while the singleton is live.
        let observation = InputObservation {
            focused: false,
            tracking_ready: self
                .capture
                .snapshot
                .is_some_and(|snapshot| snapshot.tracking_ready),
            controls_neutral: false,
            game_device_enabled: unsafe { InputDeviceManager::get() }
                .is_some_and(|device| device.m_Enabled),
        };
        if let Some(update) = self.capture.apply_raw_loss(
            generation,
            observation,
            &self.egui_context,
            &mut self.events,
        ) {
            self.finish_capture(update, observation);
        }
    }

    fn apply_capture(
        &mut self,
        request: Option<PanelRequest>,
        input: DebugUiInput,
    ) -> CaptureUpdate {
        self.apply_pending_raw_loss();
        let observation = InputObservation {
            focused: input.focused && native_game_focus(),
            tracking_ready: input.tracking_ready,
            controls_neutral: input.controls_neutral && !self.capture.input_notification_pending,
            // SAFETY: capture is stepped on the engine input/game thread while the singleton is live.
            game_device_enabled: unsafe { InputDeviceManager::get() }
                .is_some_and(|device| device.m_Enabled),
        };
        let update =
            self.capture
                .update(request, observation, &self.egui_context, &mut self.events);
        self.finish_capture(update, observation)
    }

    fn finish_capture(
        &mut self,
        mut update: CaptureUpdate,
        observation: InputObservation,
    ) -> CaptureUpdate {
        let cancelled = update.effects.cancel_pointer_and_drag;
        if cancelled {
            cancel_external_pointer();
        }
        // SAFETY: cancellation may call native code, so device and focus evidence are read again.
        let device = unsafe { InputDeviceManager::get() };
        let device_enabled = device.as_ref().is_some_and(|device| device.m_Enabled);
        let write = self.capture.validate_device_write(
            &mut update,
            InputObservation {
                game_device_enabled: device_enabled,
                ..observation
            },
            &self.egui_context,
            &mut self.events,
            || {
                let before = desktop_input::focus_loss_generation();
                let focused = native_game_focus();
                let after = desktop_input::focus_loss_generation();
                (focused && before == after, after)
            },
        );
        if let Some(enabled) = write
            && let Some(device) = device
        {
            device.m_Enabled = enabled;
        }
        if !cancelled && update.effects.cancel_pointer_and_drag {
            cancel_external_pointer();
        }
        // SAFETY: cursor ownership is updated only after the device write boundary on this thread.
        if update.snapshot.editing_requested
            && update.snapshot.focused
            && observation.tracking_ready
        {
            if self.saved_cursor.is_none()
                && let Some(graphics) = unsafe { GraphicsEngine::get() }
            {
                self.saved_cursor = Some(graphics.m_ActiveCursor);
            }
        } else if let Some(cursor) = self.saved_cursor.take()
            && let Some(graphics) = unsafe { GraphicsEngine::get() }
        {
            graphics.m_ActiveCursor = cursor;
        }
        *CAPTURE_SNAPSHOT.lock() = Some(update.snapshot);
        update
    }

    pub fn wndproc(&mut self, _hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) {
        self.apply_pending_raw_loss();
        if !self.is_input_captured() {
            return;
        }
        // In VR-panel mode the pointer is re-sourced from the panel's virtual cursor (issue #24), so
        // the window-pixel mouse messages -- whose coordinates are in the game window's space, not the
        // panel texture's -- must not reach egui. Keyboard, text, and paste still flow unchanged.
        if self.panel.is_some() && is_mouse_message(msg) {
            return;
        }
        // Window-client pixel coordinates are already the flat layout's point space (see `run`).
        wndproc(&mut self.events, msg, wparam, lparam);
    }
}

/// The engine's render size for flat-overlay layout purposes, read from the graphics params (which
/// `ApplyResize` writes from the size it was driven with, so this follows the render, not the
/// swapchain).
///
/// Under single-pass double-wide the render target is 2x per-eye wide while the image it carries is
/// one eye's half, so report the per-eye width; else the whole UI comes out half-relative-width.
fn flat_buffer_size() -> (f32, f32) {
    let params = unsafe { get_graphics_params() };
    let width = if crate::stereo::single_pass::double_wide_active() {
        params.m_Width as f32 / 2.0
    } else {
        params.m_Width as f32
    };
    (width, params.m_Height as f32)
}

/// Whether `msg` is a mouse move/button/wheel message whose coordinates are window-relative.
fn is_mouse_message(msg: u32) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
        WM_RBUTTONDOWN, WM_RBUTTONUP,
    };
    matches!(
        msg,
        WM_MOUSEMOVE
            | WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | WM_RBUTTONDOWN
            | WM_RBUTTONUP
            | WM_MBUTTONDOWN
            | WM_MBUTTONUP
            | WM_MOUSEWHEEL
    )
}

pub fn wndproc(events: &mut Vec<egui::Event>, msg: u32, wparam: WPARAM, lparam: LPARAM) {
    use windows::Win32::UI::WindowsAndMessaging::*;

    let wparam = wparam.0;
    let lparam = lparam.0;

    match msg {
        WM_MOUSEMOVE => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            events.push(egui::Event::PointerMoved(egui::Pos2::new(x, y)));
        }

        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let (x, y) = (
                (lparam & 0xFFFF) as i16 as f32,
                ((lparam >> 16) & 0xFFFF) as i16 as f32,
            );
            let button = match msg {
                WM_LBUTTONDOWN => egui::PointerButton::Primary,
                WM_RBUTTONDOWN => egui::PointerButton::Secondary,
                WM_MBUTTONDOWN => egui::PointerButton::Middle,
                _ => unreachable!(),
            };
            events.push(egui::Event::PointerButton {
                pos: egui::Pos2::new(x, y),
                button,
                pressed: true,
                modifiers: get_modifiers(),
            });
        }

        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            let (x, y) = (
                (lparam & 0xFFFF) as i16 as f32,
                ((lparam >> 16) & 0xFFFF) as i16 as f32,
            );
            let button = match msg {
                WM_LBUTTONUP => egui::PointerButton::Primary,
                WM_RBUTTONUP => egui::PointerButton::Secondary,
                WM_MBUTTONUP => egui::PointerButton::Middle,
                _ => unreachable!(),
            };
            events.push(egui::Event::PointerButton {
                pos: egui::Pos2::new(x, y),
                button,
                pressed: false,
                modifiers: get_modifiers(),
            });
        }

        WM_MOUSEWHEEL => {
            let delta = (wparam >> 16) as i16 as f32 / WHEEL_DELTA as f32;
            events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::Vec2::new(0.0, delta),
                // Discrete win32 wheel notches carry no gesture phase.
                phase: egui::TouchPhase::Move,
                modifiers: get_modifiers(),
            });
        }

        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if let Some(key) = virtual_key_to_egui_key(wparam as u32) {
                let modifiers = get_modifiers();

                // Special-case some key combinations, as is done by egui-winit
                // <https://github.com/emilk/egui/blob/9a1e358a144b5d2af9d03a80257c34883f57cf0b/crates/egui-winit/src/lib.rs#L754>
                if is_copy_command(modifiers, key) {
                    events.push(egui::Event::Copy);
                } else if is_cut_command(modifiers, key) {
                    events.push(egui::Event::Cut);
                } else if is_paste_command(modifiers, key) {
                    push_paste_command_if_available(events);
                } else {
                    events.push(egui::Event::Key {
                        key,
                        physical_key: None, // Windows doesn't provide this information easily
                        pressed: true,
                        repeat: (lparam & 0x40000000) != 0,
                        modifiers,
                    });
                }
            }
        }

        WM_KEYUP | WM_SYSKEYUP => {
            if let Some(key) = virtual_key_to_egui_key(wparam as u32) {
                events.push(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: get_modifiers(),
                });
            }
        }

        WM_CHAR => {
            let ch = wparam as u8 as char;
            if !ch.is_control() {
                events.push(egui::Event::Text(ch.to_string()));
            }
        }

        WM_PASTE => {
            push_paste_command_if_available(events);
        }

        _ => {}
    }
}

fn get_modifiers() -> egui::Modifiers {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
    unsafe {
        egui::Modifiers {
            alt: (GetKeyState(VK_MENU.0 as _) as u16 & 0x8000) != 0,
            ctrl: (GetKeyState(VK_CONTROL.0 as _) as u16 & 0x8000) != 0,
            shift: (GetKeyState(VK_SHIFT.0 as _) as u16 & 0x8000) != 0,
            mac_cmd: false,
            command: (GetKeyState(VK_CONTROL.0 as _) as u16 & 0x8000) != 0,
        }
    }
}

fn push_paste_command_if_available(events: &mut Vec<egui::Event>) {
    if let Ok(contents) = clipboard_win::get_clipboard_string() {
        let contents = contents.replace("\r\n", "\n");
        if !contents.is_empty() {
            events.push(egui::Event::Paste(contents));
        }
    }
}

// The below functions are copied from egui-winit:
// <https://github.com/emilk/egui/blob/9a1e358a144b5d2af9d03a80257c34883f57cf0b/crates/egui-winit/src/lib.rs#L1017-L1033>
fn is_cut_command(modifiers: egui::Modifiers, keycode: egui::Key) -> bool {
    keycode == egui::Key::Cut
        || (modifiers.command && keycode == egui::Key::X)
        || (cfg!(target_os = "windows") && modifiers.shift && keycode == egui::Key::Delete)
}

fn is_copy_command(modifiers: egui::Modifiers, keycode: egui::Key) -> bool {
    keycode == egui::Key::Copy
        || (modifiers.command && keycode == egui::Key::C)
        || (cfg!(target_os = "windows") && modifiers.ctrl && keycode == egui::Key::Insert)
}

fn is_paste_command(modifiers: egui::Modifiers, keycode: egui::Key) -> bool {
    keycode == egui::Key::Paste
        || (modifiers.command && keycode == egui::Key::V)
        || (cfg!(target_os = "windows") && modifiers.shift && keycode == egui::Key::Insert)
}

#[cfg(test)]
#[path = "egui_impl/capture_tests.rs"]
mod capture_tests;

fn virtual_key_to_egui_key(vk: u32) -> Option<egui::Key> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    match VIRTUAL_KEY(vk as _) {
        VK_DOWN => Some(egui::Key::ArrowDown),
        VK_LEFT => Some(egui::Key::ArrowLeft),
        VK_RIGHT => Some(egui::Key::ArrowRight),
        VK_UP => Some(egui::Key::ArrowUp),
        VK_ESCAPE => Some(egui::Key::Escape),
        VK_TAB => Some(egui::Key::Tab),
        VK_BACK => Some(egui::Key::Backspace),
        VK_RETURN => Some(egui::Key::Enter),
        VK_SPACE => Some(egui::Key::Space),
        VK_INSERT => Some(egui::Key::Insert),
        VK_DELETE => Some(egui::Key::Delete),
        VK_HOME => Some(egui::Key::Home),
        VK_END => Some(egui::Key::End),
        VK_PRIOR => Some(egui::Key::PageUp),
        VK_NEXT => Some(egui::Key::PageDown),
        VK_OEM_2 => Some(egui::Key::Slash),
        VK_OEM_5 => Some(egui::Key::Backslash),
        VK_OEM_PERIOD => Some(egui::Key::Period),
        VK_OEM_COMMA => Some(egui::Key::Comma),
        VK_OEM_7 => Some(egui::Key::Quote),
        VK_OEM_4 => Some(egui::Key::OpenBracket),
        VK_OEM_6 => Some(egui::Key::CloseBracket),
        VK_OEM_3 => Some(egui::Key::Backtick),
        VK_OEM_MINUS => Some(egui::Key::Minus),
        VK_OEM_PLUS => Some(egui::Key::Plus),
        VK_0 => Some(egui::Key::Num0),
        VK_1 => Some(egui::Key::Num1),
        VK_2 => Some(egui::Key::Num2),
        VK_3 => Some(egui::Key::Num3),
        VK_4 => Some(egui::Key::Num4),
        VK_5 => Some(egui::Key::Num5),
        VK_6 => Some(egui::Key::Num6),
        VK_7 => Some(egui::Key::Num7),
        VK_8 => Some(egui::Key::Num8),
        VK_9 => Some(egui::Key::Num9),
        VK_A => Some(egui::Key::A),
        VK_B => Some(egui::Key::B),
        VK_C => Some(egui::Key::C),
        VK_D => Some(egui::Key::D),
        VK_E => Some(egui::Key::E),
        VK_F => Some(egui::Key::F),
        VK_G => Some(egui::Key::G),
        VK_H => Some(egui::Key::H),
        VK_I => Some(egui::Key::I),
        VK_J => Some(egui::Key::J),
        VK_K => Some(egui::Key::K),
        VK_L => Some(egui::Key::L),
        VK_M => Some(egui::Key::M),
        VK_N => Some(egui::Key::N),
        VK_O => Some(egui::Key::O),
        VK_P => Some(egui::Key::P),
        VK_Q => Some(egui::Key::Q),
        VK_R => Some(egui::Key::R),
        VK_S => Some(egui::Key::S),
        VK_T => Some(egui::Key::T),
        VK_U => Some(egui::Key::U),
        VK_V => Some(egui::Key::V),
        VK_W => Some(egui::Key::W),
        VK_X => Some(egui::Key::X),
        VK_Y => Some(egui::Key::Y),
        VK_Z => Some(egui::Key::Z),
        VK_F1 => Some(egui::Key::F1),
        VK_F2 => Some(egui::Key::F2),
        VK_F3 => Some(egui::Key::F3),
        VK_F4 => Some(egui::Key::F4),
        VK_F5 => Some(egui::Key::F5),
        VK_F6 => Some(egui::Key::F6),
        VK_F7 => Some(egui::Key::F7),
        VK_F8 => Some(egui::Key::F8),
        VK_F9 => Some(egui::Key::F9),
        VK_F10 => Some(egui::Key::F10),
        VK_F11 => Some(egui::Key::F11),
        VK_F12 => Some(egui::Key::F12),
        VK_F13 => Some(egui::Key::F13),
        VK_F14 => Some(egui::Key::F14),
        VK_F15 => Some(egui::Key::F15),
        VK_F16 => Some(egui::Key::F16),
        VK_F17 => Some(egui::Key::F17),
        VK_F18 => Some(egui::Key::F18),
        VK_F19 => Some(egui::Key::F19),
        VK_F20 => Some(egui::Key::F20),
        VK_F21 => Some(egui::Key::F21),
        VK_F22 => Some(egui::Key::F22),
        VK_F23 => Some(egui::Key::F23),
        VK_F24 => Some(egui::Key::F24),
        _ => None,
    }
}
