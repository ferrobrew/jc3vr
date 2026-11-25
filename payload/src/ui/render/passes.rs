//! Render-pass id presentation: the engine's debug name for a pass.

/// The engine's debug name for a pass id, via `GetRenderPassName`.
pub(super) fn render_pass_name(id: i16) -> &'static str {
    if !(0..jc3gi::graphics_engine::render_engine::RenderPassId::RP_RENDERPASS_COUNT as i32)
        .contains(&i32::from(id))
    {
        return "(out of range)";
    }
    // SAFETY: the id is in the enum's verified range, and the engine returns static strings.
    unsafe {
        let pass = std::mem::transmute::<i32, jc3gi::graphics_engine::render_engine::RenderPassId>(
            i32::from(id),
        );
        let ptr = jc3gi::graphics_engine::render_engine::GetRenderPassName(pass);
        if ptr.is_null() {
            return "(null)";
        }
        std::ffi::CStr::from_ptr(ptr.cast())
            .to_str()
            .unwrap_or("(non-utf8)")
    }
}
