//! The tree impostors' pixel size, staged at one eye's width under the double-wide collapse.
//!
//! [`TreeImpostorRBType::Setup`](jc3gi::graphics_engine::render_block::TreeImpostorRBType::Setup)
//! stages the impostor vertex shader's `PixelSize` as one over the render context's display width,
//! which under the double-wide collapse is the width of both eyes. The shader measures each card's
//! clip-space extent through a single eye's projection and divides by that pixel size to get the
//! card's footprint in pixels, so the footprint came out twice as wide as in an eye-sized render, and
//! the pixel shader's atlas-coordinate offset, which scales the coordinate derivatives by it, moved
//! every card's alpha-tested silhouette. For the length of the call the display width is halved to
//! one eye's, then put back.

use jc3gi::graphics_engine::graphics_engine::RenderContext;
use jc3vr_view::stereo::layout::double_wide_active;

use crate::stereo::single_pass::in_gbuffer_range;

/// The render context's display width, held at one eye's for the life of the guard.
pub(crate) struct EyeDisplayWidth {
    rc: *mut RenderContext,
    saved: i32,
}

impl EyeDisplayWidth {
    /// Halve `rc`'s display width to one eye's when the double-wide collapse is drawing its routed
    /// range; `None` leaves it untouched.
    ///
    /// # Safety
    ///
    /// `rc` must be the live render context the engine passed to the impostor type's `Setup`, and the
    /// guard must be dropped before that call returns to the engine.
    pub unsafe fn stage(rc: *mut RenderContext) -> Option<Self> {
        if !double_wide_active() || !in_gbuffer_range() {
            return None;
        }
        // SAFETY: the caller guarantees `rc` is live for the guard's lifetime.
        let context = unsafe { rc.as_mut() }?;
        let saved = context.m_DisplayWidth;
        // The double-wide target is exactly two eyes side by side.
        context.m_DisplayWidth = saved / 2;
        Some(Self { rc, saved })
    }
}

impl Drop for EyeDisplayWidth {
    fn drop(&mut self) {
        // SAFETY: `stage`'s contract keeps the context live until the guard drops.
        if let Some(context) = unsafe { self.rc.as_mut() } {
            context.m_DisplayWidth = self.saved;
        }
    }
}
