//! The post-draw composition seam: what the layers above the render hooks contribute to
//! `RenderEngine::PostDraw`, to the draw task's UI redirect, and to the engine's flip.
//!
//! The post-draw ([`crate::hooks::graphics_engine::graphics_engine`]) runs, per dispatch: the
//! [`DispatchTick`]s, outside the engine's context lock; then, under it, the scene capture, the
//! [`EyeLayer`]s in their order, and on the frame's last near dispatch the [`FramePass`]es; and
//! after the eye copies, the [`EyeFrameRecorder`]. Each contribution is registered once at startup
//! by the module that owns it, so the render hooks name none of them. The order is the enums'
//! order, whatever order the registrations ran in, and a contribution nobody registered is skipped.

use std::sync::OnceLock;

use jc3gi::graphics_engine::{device::Device, texture::Texture};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D};

/// A draw over one eye of the dispatch's final back buffer: the context, the engine's device, the
/// back buffer, and the eye. Under the collapse the caller has selected the eye with
/// `set_collapse_ui_eye`.
pub(crate) type EyeDraw = fn(&ID3D11DeviceContext, &Device, &Texture, usize);

/// A draw over the frame's final back buffer, once per frame.
pub(crate) type FrameDraw = fn(&ID3D11DeviceContext, &Device, &Texture);

/// A tick run once per dispatch, with the dispatch's draw index, before the engine's context lock
/// is taken. The ticks read the published state of the systems they draw under those systems' own
/// locks.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DispatchTick {
    /// The world-space overlay's producers.
    WorldOverlay,
    /// The scope lens's plan.
    ScopeLens,
}

impl DispatchTick {
    /// The number of ticks; the last variant closes the count.
    const COUNT: usize = Self::ScopeLens as usize + 1;
}

/// A layer drawn onto each eye's final back buffer, in draw order: the panels sit on top of the
/// world-locked readouts.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EyeLayer {
    /// The world-space overlay.
    WorldOverlay,
    /// The scope lens, over the overlay.
    ScopeLens,
    /// The floating HUD panel.
    HudPanel,
    /// The egui debug panel.
    DebugPanel,
    /// The controls summary.
    ControlsSummary,
    /// The restart notice.
    RestartNotice,
}

impl EyeLayer {
    /// The number of layers; the last variant closes the count.
    const COUNT: usize = Self::RestartNotice as usize + 1;
}

/// A pass run once per frame, on its last near dispatch, in this order.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FramePass {
    /// The flat debug overlay, rendered offscreen for the desktop mirror.
    MirrorOverlay,
    /// The learned body estimator's GPU pass.
    BodyEstimator,
}

impl FramePass {
    /// The number of passes; the last variant closes the count.
    const COUNT: usize = Self::BodyEstimator as usize + 1;
}

/// The game UI's redirect into the floating panel's texture, kept up by the draw task under the
/// UI's deferred-render lock and the engine's context lock.
#[derive(Clone, Copy)]
pub struct UiRedirect {
    /// Maintain the redirect for the device and its back buffer's width and height.
    pub maintain: fn(&Device, u32, u32),
    /// Whether the redirect is in place, so the UI's deferred render is submitted from the
    /// post-draw.
    pub applied: fn() -> bool,
}

/// What records the frame's eye images.
#[derive(Clone, Copy)]
pub struct EyeFrameRecorder {
    /// Whether the eye captures must be copied every frame, even where the eyes go straight to
    /// the headset.
    pub wants_eye_captures: fn() -> bool,
    /// Note a rendered frame, on its last near dispatch; returns the frame's ordinal while a
    /// recording runs.
    pub frame_rendered: fn() -> Option<u32>,
    /// Record both eyes' captures, which hold the frame `frame_rendered` returned, under the
    /// engine's context lock.
    pub capture_frame:
        unsafe fn(&ID3D11Device, &ID3D11DeviceContext, [&ID3D11Texture2D; 2], Option<u32>),
    /// Release what recording the eyes holds, on a frame whose eyes are not recorded.
    pub release_capture: fn(),
}

pub fn register_dispatch_tick(tick: DispatchTick, run: fn(usize)) {
    DISPATCH_TICKS.set(tick as usize, run, "dispatch tick");
}

/// Register the copy of the dispatch's eye images, taken before any [`EyeLayer`] draws.
pub fn register_scene_capture(capture: EyeDraw) {
    set_once(&SCENE_CAPTURE, capture, "scene capture");
}

pub fn register_eye_layer(layer: EyeLayer, draw: EyeDraw) {
    EYE_LAYERS.set(layer as usize, draw, "eye layer");
}

pub fn register_frame_pass(pass: FramePass, draw: FrameDraw) {
    FRAME_PASSES.set(pass as usize, draw, "frame pass");
}

pub fn register_ui_redirect(redirect: UiRedirect) {
    set_once(&UI_REDIRECT, redirect, "UI redirect");
}

pub fn register_eye_frame_recorder(recorder: EyeFrameRecorder) {
    set_once(&RECORDER, recorder, "eye-frame recorder");
}

/// Register the desktop overlay drawn into the game's own back buffer just before the engine's
/// flip presents it.
pub fn register_desktop_overlay(draw: fn()) {
    set_once(&DESKTOP_OVERLAY, draw, "desktop overlay");
}

pub(super) fn run_dispatch_ticks(index: usize) {
    DISPATCH_TICKS.each().for_each(|tick| tick(index));
}

pub(super) fn capture_scene(
    context: &ID3D11DeviceContext,
    device: &Device,
    target: &Texture,
    eye: usize,
) {
    if let Some(capture) = SCENE_CAPTURE.get() {
        capture(context, device, target, eye);
    }
}

pub(super) fn draw_eye_layers(
    context: &ID3D11DeviceContext,
    device: &Device,
    target: &Texture,
    eye: usize,
) {
    EYE_LAYERS
        .each()
        .for_each(|draw| draw(context, device, target, eye));
}

pub(super) fn run_frame_passes(context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
    FRAME_PASSES
        .each()
        .for_each(|draw| draw(context, device, target));
}

pub(super) fn maintain_ui_redirect(device: &Device, width: u32, height: u32) {
    if let Some(redirect) = UI_REDIRECT.get() {
        (redirect.maintain)(device, width, height);
    }
}

pub(super) fn ui_redirect_applied() -> bool {
    UI_REDIRECT
        .get()
        .is_some_and(|redirect| (redirect.applied)())
}

pub(super) fn eye_frame_recorder() -> Option<&'static EyeFrameRecorder> {
    RECORDER.get()
}

pub(super) fn recorder_wants_eye_captures() -> bool {
    RECORDER
        .get()
        .is_some_and(|recorder| (recorder.wants_eye_captures)())
}

pub(super) fn draw_desktop_overlay() {
    if let Some(draw) = DESKTOP_OVERLAY.get() {
        draw();
    }
}

/// Registered contributions keyed by an enum's discriminant, run in discriminant order.
struct Slots<F: Copy, const N: usize>([OnceLock<F>; N]);

impl<F: Copy, const N: usize> Slots<F, N> {
    const fn new() -> Self {
        Self([const { OnceLock::new() }; N])
    }

    fn set(&self, index: usize, value: F, what: &str) {
        set_once(&self.0[index], value, what);
    }

    fn each(&self) -> impl Iterator<Item = F> + '_ {
        self.0.iter().filter_map(|slot| slot.get().copied())
    }
}

/// Set a registration; the first one stands.
fn set_once<T>(slot: &OnceLock<T>, value: T, what: &str) {
    if slot.set(value).is_err() {
        tracing::warn!("post-draw: a {what} is already registered; the new one is ignored");
    }
}

static DISPATCH_TICKS: Slots<fn(usize), { DispatchTick::COUNT }> = Slots::new();

static SCENE_CAPTURE: OnceLock<EyeDraw> = OnceLock::new();

static EYE_LAYERS: Slots<EyeDraw, { EyeLayer::COUNT }> = Slots::new();

static FRAME_PASSES: Slots<FrameDraw, { FramePass::COUNT }> = Slots::new();

static UI_REDIRECT: OnceLock<UiRedirect> = OnceLock::new();

static RECORDER: OnceLock<EyeFrameRecorder> = OnceLock::new();

static DESKTOP_OVERLAY: OnceLock<fn()> = OnceLock::new();

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    thread_local! {
        static RAN: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    }

    fn first(_: usize) {
        RAN.with(|ran| ran.borrow_mut().push(0));
    }

    fn second(_: usize) {
        RAN.with(|ran| ran.borrow_mut().push(1));
    }

    fn third(_: usize) {
        RAN.with(|ran| ran.borrow_mut().push(2));
    }

    #[test]
    fn contributions_run_in_slot_order_whatever_the_registration_order() {
        let slots: Slots<fn(usize), 3> = Slots::new();
        slots.set(2, third, "test");
        slots.set(0, first, "test");
        slots.each().for_each(|run| run(0));
        assert_eq!(RAN.with(|ran| ran.take()), [0, 2]);
    }

    #[test]
    fn the_first_registration_of_a_slot_stands() {
        let slots: Slots<fn(usize), 2> = Slots::new();
        slots.set(1, second, "test");
        slots.set(1, third, "test");
        slots.each().for_each(|run| run(0));
        assert_eq!(RAN.with(|ran| ran.take()), [1]);
    }
}
