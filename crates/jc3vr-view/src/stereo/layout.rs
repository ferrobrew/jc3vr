//! The stereo layout a frame renders in: whether the eyes collapse into one single-pass G-buffer
//! walk, whether that walk renders into a double-wide target, and the device capability both rest
//! on.
//!
//! The single-pass config snapshot owns the flags behind the layout and publishes the two layout
//! bits each time it samples them: the game thread's per-frame snapshot, and the pin a draw thread
//! takes for the dispatch it opens. A read here follows the snapshot's own rule, so code that sizes
//! or poses by the layout answers as the single-pass gates do: inside a dispatch, the dispatch's pin;
//! anywhere else, the frame snapshot; and before the first sample, no collapse.

use std::{
    cell::Cell,
    sync::atomic::{AtomicU8, Ordering},
};

use jc3gi::graphics_engine::graphics_engine::GraphicsEngine;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_FEATURE_D3D11_OPTIONS3, D3D11_FEATURE_DATA_D3D11_OPTIONS3, ID3D11Device,
};

/// The result of the DXVK viewport-routing capability probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Not yet probed (no device seen, or the probe has not run this session).
    Unprobed,
    /// `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` is supported: a vertex shader may write
    /// `SV_ViewportArrayIndex` directly, so single-pass routing is possible.
    Supported,
    /// The capability is absent; single-pass must fall back to double-draw.
    Unsupported,
}

/// Probe the D3D11 device for `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` (the D3D11.3 feature
/// that lets a vertex shader write `SV_ViewportArrayIndex`), caching the result. Idempotent and
/// cheap; safe to call every frame. `CheckFeatureSupport` on the device is free-threaded, so no
/// context lock is needed.
pub fn probe(device: &ID3D11Device) -> Capability {
    let mut options = D3D11_FEATURE_DATA_D3D11_OPTIONS3::default();
    let ok = unsafe {
        device.CheckFeatureSupport(
            D3D11_FEATURE_D3D11_OPTIONS3,
            std::ptr::from_mut(&mut options).cast(),
            std::mem::size_of::<D3D11_FEATURE_DATA_D3D11_OPTIONS3>() as u32,
        )
    };
    let capability = if ok.is_ok()
        && options
            .VPAndRTArrayIndexFromAnyShaderFeedingRasterizer
            .as_bool()
    {
        Capability::Supported
    } else {
        Capability::Unsupported
    };
    CAPABILITY.store(capability as u8, Ordering::Relaxed);
    capability
}

/// Probe the capability using the live engine device, if one is available and the probe has not run
/// yet. Returns the (now cached) result. Called from the debug UI and the frame driver so the probe
/// happens as soon as a device exists.
pub fn probe_if_needed() -> Capability {
    let cached = capability();
    if cached != Capability::Unprobed {
        return cached;
    }
    // SAFETY: `GraphicsEngine::get` returns the live singleton or `None`; the device pointer is stable
    // once the engine has initialised.
    let Some(device) = (unsafe { GraphicsEngine::get() }) else {
        return Capability::Unprobed;
    };
    let Some(device) = (unsafe { device.m_Device.as_ref() }) else {
        return Capability::Unprobed;
    };
    probe(&device.m_Device)
}

/// The cached capability-probe result.
#[inline]
pub fn capability() -> Capability {
    match CAPABILITY.load(Ordering::Relaxed) {
        x if x == Capability::Supported as u8 => Capability::Supported,
        x if x == Capability::Unsupported as u8 => Capability::Unsupported,
        _ => Capability::Unprobed,
    }
}

/// Whether the per-eye double-draw has been collapsed to a single G-buffer walk: one `game.Draw`
/// produces both eyes (via the single-pass dual-eye `cb13` + viewport routing + instance doubling),
/// the render camera stays centered (no per-eye offset -- both eyes come from `cb13`), and the
/// capture splits the one back buffer into the two eye textures. Requires single-pass to be active
/// with its dual-eye path on; independent of `single_pass_double_wide`, which only upgrades each
/// eye-half from squished to full resolution.
#[inline]
pub fn collapse_active() -> bool {
    routing_live() && sampled().collapse()
}

/// Whether the scene render targets are re-created at 2x per-eye width so each eye-half is full
/// resolution (instead of a squished half of a per-eye-sized target). Requires [`collapse_active`] --
/// it only makes sense for the single walk whose capture split reads one full-width half per eye.
/// Drives the engine render resolution ([`crate::vr::engine_render_resolution`]) and the per-eye
/// capture-texture width (`ui::render`); the XR swapchain stays per-eye width.
#[inline]
pub fn double_wide_active() -> bool {
    routing_live() && sampled().double_wide()
}

/// The layout bits of one sample of the single-pass configuration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutFlags(u8);

impl LayoutFlags {
    /// The layout a configuration sample asks for: `collapse` when single-pass is switched on for a
    /// live session, out of its dry run, with the dual-eye path and the collapse on; double-wide
    /// when the collapse is and `double_wide` is on too.
    pub fn new(collapse: bool, double_wide: bool) -> Self {
        let mut bits = 0;
        if collapse {
            bits |= COLLAPSE;
            if double_wide {
                bits |= DOUBLE_WIDE;
            }
        }
        Self(bits)
    }

    fn collapse(self) -> bool {
        self.0 & COLLAPSE != 0
    }

    fn double_wide(self) -> bool {
        self.0 & DOUBLE_WIDE != 0
    }
}

/// Publish the game thread's frame sample, read by every thread outside a dispatch.
pub fn publish_frame_layout(flags: LayoutFlags) {
    FRAME_LAYOUT.store(flags.0, Ordering::Relaxed);
}

/// Pin the sample of the dispatch this thread is opening, for the rest of the dispatch.
pub fn pin_dispatch_layout(flags: LayoutFlags) {
    DISPATCH_LAYOUT.set(Some(flags));
}

/// Drop this thread's dispatch pin, so whatever it runs between dispatches reads the frame sample.
pub fn unpin_dispatch_layout() {
    DISPATCH_LAYOUT.set(None);
}

const COLLAPSE: u8 = 1 << 0;
const DOUBLE_WIDE: u8 = 1 << 1;

/// Whether the layout can take effect at all: the device routes viewports from the vertex shader,
/// and eject has not begun (single-pass goes inert the instant it does, `jc3vr_core::is_shutting_down`).
#[inline]
fn routing_live() -> bool {
    !jc3vr_core::is_shutting_down() && capability() == Capability::Supported
}

/// The sample this thread answers from: its dispatch's pin, else the frame's.
#[inline]
fn sampled() -> LayoutFlags {
    DISPATCH_LAYOUT
        .get()
        .unwrap_or_else(|| LayoutFlags(FRAME_LAYOUT.load(Ordering::Relaxed)))
}

static CAPABILITY: AtomicU8 = AtomicU8::new(Capability::Unprobed as u8);

/// The layout bits of the game thread's latest frame sample; none until the first.
static FRAME_LAYOUT: AtomicU8 = AtomicU8::new(0);

thread_local! {
    /// The layout bits of the dispatch in flight on this thread, or `None` outside one.
    static DISPATCH_LAYOUT: Cell<Option<LayoutFlags>> = const { Cell::new(None) };
}
