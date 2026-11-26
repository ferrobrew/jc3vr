//! What the layers above the runtime contribute to the desktop mirror: the overlay textures it
//! composites over the eye images, and the observer of its layout and presents.
//!
//! The mirror reads both through registrations its owners install at startup, so the runtime names
//! neither the HUD nor the recorder. Until one is registered, the mirror composites no overlay and
//! nothing observes it.

use std::sync::OnceLock;

use serde::Serialize;
use windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView;

use crate::vr::MirrorView;

/// The desktop mirror's layout, as the recorder's sidecar records it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MirrorFrame {
    pub view: MirrorView,
    /// The window's client size.
    pub window: (u32, u32),
    /// Where each eye's image lands in the window, in on-screen order.
    pub draws: [Option<MirrorDraw>; 2],
}

/// One eye's image in the mirror window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MirrorDraw {
    pub eye: usize,
    /// The whole eye image's rectangle (`x, y, width, height`) in window pixels. It extends past the
    /// window where the mirror crops the image.
    pub rect: [f32; 4],
}

/// The overlay textures the mirror composites over the eye images.
#[derive(Clone, Copy)]
pub struct MirrorLayers {
    /// The floating panel's texture, while the panel is up and has drawn.
    pub panel_srv: fn() -> Option<ID3D11ShaderResourceView>,
    /// The flat debug overlay's offscreen texture, while it holds a frame.
    pub overlay_srv: fn() -> Option<ID3D11ShaderResourceView>,
}

/// The observer of the mirror's presents.
#[derive(Clone, Copy)]
pub struct MirrorObserver {
    /// Whether anything records the mirror window, so the frame stamp means something.
    pub records_mirror: fn() -> bool,
    /// The layout of a present, after it.
    pub note_mirror: fn(MirrorFrame),
    /// A present's counter, and whether the frame stamp drew it into the window.
    pub note_present: fn(u32, bool),
}

/// Register the mirror's overlay textures. The first registration stands.
pub fn register_mirror_layers(layers: MirrorLayers) {
    if LAYERS.set(layers).is_err() {
        tracing::warn!("vr: mirror layers are already registered; the new ones are ignored");
    }
}

/// Register the mirror's observer. The first registration stands.
pub fn register_mirror_observer(observer: MirrorObserver) {
    if OBSERVER.set(observer).is_err() {
        tracing::warn!("vr: a mirror observer is already registered; the new one is ignored");
    }
}

pub(super) fn panel_srv() -> Option<ID3D11ShaderResourceView> {
    LAYERS.get().and_then(|layers| (layers.panel_srv)())
}

pub(super) fn overlay_srv() -> Option<ID3D11ShaderResourceView> {
    LAYERS.get().and_then(|layers| (layers.overlay_srv)())
}

pub(super) fn records_mirror() -> bool {
    OBSERVER
        .get()
        .is_some_and(|observer| (observer.records_mirror)())
}

pub(super) fn note_mirror(frame: MirrorFrame) {
    if let Some(observer) = OBSERVER.get() {
        (observer.note_mirror)(frame);
    }
}

pub(super) fn note_present(present: u32, stamped: bool) {
    if let Some(observer) = OBSERVER.get() {
        (observer.note_present)(present, stamped);
    }
}

static LAYERS: OnceLock<MirrorLayers> = OnceLock::new();

static OBSERVER: OnceLock<MirrorObserver> = OnceLock::new();
