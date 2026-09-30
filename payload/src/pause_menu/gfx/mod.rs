//! Safe access to AS3 values through the Scaleform value API
//! ([`ValueObjectInterface`](jc3gi::ui::scaleform::ValueObjectInterface)).
//!
//! Two rules make this safe, and the types enforce both: every access happens on the UI capture
//! thread ([`CaptureThread`] is the proof, and nothing can be built without it), and every managed
//! value the mod obtains is released before the hook call returns ([`GfxValue`] releases on drop
//! and cannot outlive the token it borrows). A new access always starts again from the screen's
//! clip ([`UIBase::m_Clip`](jc3gi::ui::ui_base::UIBase::m_Clip)), which is only valid while the
//! screen is active.

mod thread;
mod value;

pub(crate) use crate::pause_menu::gfx::{
    thread::CaptureThread,
    value::{GfxValue, Param, decode_arg},
};
