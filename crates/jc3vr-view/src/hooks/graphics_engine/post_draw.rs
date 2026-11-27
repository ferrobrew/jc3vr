//! The post-draw composition seam: what the layers above the render hooks contribute to
//! `RenderEngine::PostDraw`, to the draw task's UI redirect, and to the engine's flip.
//!
//! The post-draw (`jc3vr_render::hooks::graphics_engine::engine`) runs, per dispatch: the dispatch
//! ticks, outside the engine's context lock; then, under it, the render crate's own passes, the
//! eye layers, and on the frame's last dispatch the [`FramePass`]es; and after the eye copies, on
//! the frame's last dispatch, the [`EyeFrameRecorder`]. Each contribution is registered once at startup by the module that
//! owns it, so the render hooks name none of them. The dispatch ticks and the eye layers run in
//! the order their one registrant declares them in, and the frame passes in [`FramePass`] order,
//! whatever order the registrations ran in. A registrant reports what it registers to the install's
//! registration check through the `*_registered` probes here, so nothing this seam runs is missing
//! by the time the hooks are installed.

use std::sync::OnceLock;

use jc3gi::graphics_engine::{device::Device, texture::Texture};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D};

/// A draw over one eye of the dispatch's final back buffer: the context, the engine's device, the
/// back buffer, and the eye. Under the collapse the caller has selected the eye with
/// `set_collapse_ui_eye`.
pub type EyeDraw = fn(&ID3D11DeviceContext, &Device, &Texture, usize);

/// A draw over the frame's final back buffer, once per frame.
pub(crate) type FrameDraw = fn(&ID3D11DeviceContext, &Device, &Texture);

/// A tick run once per dispatch, with the dispatch's draw index, before the engine's context lock
/// is taken. The ticks read the published state of the systems they draw under those systems' own
/// locks.
pub type DispatchTick = fn(usize);

/// A pass run once per frame, on its last dispatch, in this order.
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
    /// Note a rendered frame, on its last dispatch; returns the frame's ordinal while a
    /// recording runs.
    pub frame_rendered: fn() -> Option<u32>,
    /// Record both eyes' captures, which hold the frame `frame_rendered` returned, under the
    /// engine's context lock.
    pub capture_frame:
        unsafe fn(&ID3D11Device, &ID3D11DeviceContext, [&ID3D11Texture2D; 2], Option<u32>),
    /// Release what recording the eyes holds, on a frame whose eyes are not recorded.
    pub release_capture: fn(),
}

/// Register the dispatch ticks, in the order they run.
pub fn register_dispatch_ticks(ticks: &'static [DispatchTick]) {
    set_once(&DISPATCH_TICKS, ticks, "dispatch tick table");
}

/// Register the layers drawn onto each eye over the render crate's own passes, in draw order.
pub fn register_eye_layers(layers: &'static [EyeDraw]) {
    set_once(&EYE_LAYERS, layers, "eye layer table");
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

/// Whether the dispatch ticks are registered, for the install's registration check.
pub fn dispatch_ticks_registered() -> bool {
    DISPATCH_TICKS.get().is_some()
}

/// Whether the eye layers are registered, for the install's registration check.
pub fn eye_layers_registered() -> bool {
    EYE_LAYERS.get().is_some()
}

/// Whether `pass` is registered, for the install's registration check.
pub fn frame_pass_registered(pass: FramePass) -> bool {
    FRAME_PASSES.registered(pass as usize)
}

/// Whether the UI redirect is registered, for the install's registration check.
pub fn ui_redirect_registered() -> bool {
    UI_REDIRECT.get().is_some()
}

/// Whether the eye-frame recorder is registered, for the install's registration check.
pub fn eye_frame_recorder_registered() -> bool {
    RECORDER.get().is_some()
}

/// Whether the desktop overlay is registered, for the install's registration check.
pub fn desktop_overlay_registered() -> bool {
    DESKTOP_OVERLAY.get().is_some()
}

#[inline]
pub fn run_dispatch_ticks(index: usize) {
    DISPATCH_TICKS
        .get()
        .into_iter()
        .flat_map(|ticks| ticks.iter())
        .for_each(|tick| tick(index));
}

#[inline]
pub fn draw_eye_layers(
    context: &ID3D11DeviceContext,
    device: &Device,
    target: &Texture,
    eye: usize,
) {
    EYE_LAYERS
        .get()
        .into_iter()
        .flat_map(|layers| layers.iter())
        .for_each(|draw| draw(context, device, target, eye));
}

#[inline]
pub fn run_frame_passes(context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
    FRAME_PASSES
        .each()
        .for_each(|draw| draw(context, device, target));
}

#[inline]
pub fn maintain_ui_redirect(device: &Device, width: u32, height: u32) {
    if let Some(redirect) = UI_REDIRECT.get() {
        (redirect.maintain)(device, width, height);
    }
}

#[inline]
pub fn ui_redirect_applied() -> bool {
    UI_REDIRECT
        .get()
        .is_some_and(|redirect| (redirect.applied)())
}

#[inline]
pub fn eye_frame_recorder() -> Option<&'static EyeFrameRecorder> {
    RECORDER.get()
}

#[inline]
pub fn recorder_wants_eye_captures() -> bool {
    RECORDER
        .get()
        .is_some_and(|recorder| (recorder.wants_eye_captures)())
}

#[inline]
pub fn draw_desktop_overlay() {
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

    fn registered(&self, index: usize) -> bool {
        self.0[index].get().is_some()
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

static DISPATCH_TICKS: OnceLock<&'static [DispatchTick]> = OnceLock::new();

static EYE_LAYERS: OnceLock<&'static [EyeDraw]> = OnceLock::new();

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
