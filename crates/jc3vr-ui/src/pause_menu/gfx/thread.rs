//! The capture-thread token.

use std::marker::PhantomData;

use jc3gi::ui::ui_manager::UIManager;
use windows::Win32::System::Threading::GetCurrentThreadId;

/// Proof that the current thread is the UI manager's capture thread
/// ([`UIManager::m_CurrentCaptureThread`]), the only thread on which the engine touches AS3 values.
/// Every [`GfxValue`](crate::pause_menu::gfx::GfxValue) borrows one, so a value cannot outlive the
/// hook call that made it or cross to another thread: the token is neither `Send` nor `Sync`.
pub(crate) struct CaptureThread {
    _not_send: PhantomData<*const ()>,
}

impl CaptureThread {
    /// The token, when called on the capture thread; `None` on any other thread or before the UI
    /// manager exists.
    pub(crate) fn current() -> Option<Self> {
        // SAFETY: the UI manager is a process-lifetime singleton once created; the field is a
        // plain thread id the engine writes on the same thread that reads it here when it matches.
        let owner = unsafe { UIManager::get() }?.m_CurrentCaptureThread;
        // SAFETY: reads only the calling thread's own id.
        let current = unsafe { GetCurrentThreadId() };
        (owner == current).then_some(Self {
            _not_send: PhantomData,
        })
    }
}
