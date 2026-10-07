//! The flat debug-UI overlay for the desktop mirror, rendered into an offscreen texture.
//!
//! Normally the flat egui overlay is drawn straight onto the mirror's back buffer by
//! [`EguiState::render`](crate::egui_impl::EguiState::render). That call mutates the shared
//! `EguiState` and issues D3D work, so it is only safe on the game thread after the frame's egui
//! pass — which the deferred frame tail ([`crate::vr::tail`]) is not. So when the tail is deferred,
//! render the overlay once on the draw thread, into an offscreen texture, and let the mirror composite
//! that texture from whichever thread runs it — the model the floating panel (issue #24) uses.
//!
//! It renders on the frame's last near dispatch, not on eye 0's like the panel: the game thread waits
//! on eye 0's walk, and with the tail deferred it does not wait on the last one, which the tail drains
//! before it composites the mirror. That walk can run after the next frame's egui pass has started,
//! so it may take that frame's output a frame early;
//! [`EguiState::render_to`](crate::egui_impl::EguiState::render_to) then leaves the texture
//! as it is on the frame that finds the output gone, rather than blanking it.
//!
//! Only active while a VR session renders, the mirror is on, the floating panel is off (with the
//! panel on, the mirror composites the panel texture instead), and the flat overlay is visible
//! (`hud.flat_overlay_visible`, toggled with F4). Off, every path is the untouched flat overlay.

use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11ShaderResourceView};

use crate::hud::target::HudTarget;

/// The overlay texture, sized to the window client rect, holding this frame's egui output.
static OVERLAY: Mutex<Option<HudTarget>> = Mutex::new(None);

/// Whether the flat overlay should be redirected into the offscreen texture this frame: a VR frame is
/// rendering, the mirror is on, and the floating panel is off. Read on the draw thread (the render
/// gate) and the mirror thread (the composite gate), so it uses the draw-safe render-params signal,
/// not the VR runtime lock (see [`crate::hud::egui_panel::is_active`] for why).
pub(crate) fn is_active() -> bool {
    crate::vr::render_params(0).is_some()
        && !crate::director::cinematic::active()
        && crate::config::Config::lock_query(|c| {
            c.vr.mirror && !c.hud.egui_panel.enabled && c.hud.flat_overlay_visible
        })
}

/// Render this frame's egui output into the overlay texture. Called from the post-draw hook of the
/// frame's last near dispatch with the engine context mutex held (a no-op while the floating panel
/// is on, so only one of the two consumes the single-use egui output).
///
/// Sizes the texture to the **window client rect**, not to `target` (the eye back buffer). The mirror
/// composites the overlay across the whole back buffer, and the present then stretches that buffer
/// onto the window client rect, so a window-sized overlay arrives on screen at exactly 1:1 whatever
/// the render resolution is. Sizing it to the back buffer instead couples the UI to the render
/// resolution: in a session that buffer is the near-square per-eye resolution, and the stretch onto a
/// widescreen window smears the UI horizontally, the more so the wider the window. `target` is only
/// the fallback for the frames before the window rect is known. For the same reason the double-wide
/// back buffer needs no special case here -- the overlay is not measured in back-buffer pixels at all.
pub(crate) fn render(context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
    if !is_active() {
        // Drop the texture when inactive so a later activation rebuilds at the current size.
        *OVERLAY.lock() = None;
        return;
    }
    let (width, height) = live_client_size().unwrap_or((
        u32::from(target.m_Width).max(1),
        u32::from(target.m_Height).max(1),
    ));
    let (width, height) = (width.max(1), height.max(1));
    let mut overlay = OVERLAY.lock();
    if overlay.as_ref().map(HudTarget::size) != Some((width, height)) {
        match HudTarget::new(device, width, height) {
            Ok(target) => {
                // A new texture's contents are undefined, and `render_to` leaves the target as it
                // is when this frame's egui output was already taken.
                // SAFETY: the caller holds the engine context mutex; the view is live.
                unsafe { context.ClearRenderTargetView(target.color_rtv(), &[0.0; 4]) };
                *overlay = Some(target);
            }
            Err(e) => {
                tracing::error!("mirror overlay target: {e:#}");
                *overlay = None;
            }
        }
    }
    let Some(overlay) = overlay.as_ref() else {
        return;
    };
    if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut() {
        egui_state.render_to(context, overlay.color_rtv());
    }
}

/// The window's client size, preferring the live rect over the cursor mapping's cached copy.
///
/// [`crate::hud::cursor::client_size`] is refreshed only on `WM_MOUSEMOVE`, so a window resized
/// without the pointer moving inside the client area -- a title-bar drag, a keyboard or WM resize, a
/// window managed while the user is in the headset -- leaves it stale. The mirror composites this
/// texture using the *live* rect, so a stale size here stretches the UI by exactly the resize ratio,
/// which is the distortion this overlay is sized to avoid.
fn live_client_size() -> Option<(u32, u32)> {
    crate::vr::window::client_size().or_else(crate::hud::cursor::client_size)
}

/// A clone of the overlay texture's shader-resource view for the desktop mirror composite, or `None`
/// when the overlay is inactive or not yet built. Locks [`OVERLAY`] briefly.
pub(crate) fn overlay_srv() -> Option<ID3D11ShaderResourceView> {
    OVERLAY.lock().as_ref().map(|t| t.color_srv().clone())
}
