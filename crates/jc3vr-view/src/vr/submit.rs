//! The submit side of the OpenXR session: the frame stream (`xrBeginFrame`, `xrEndFrame`), the
//! stereo swapchain, and the reference space the projection layer is composited in, under a lock of
//! their own.
//!
//! The rest of the session (the frame waiter, the event pump, the view and controller location)
//! stays in the runtime state ([`crate::vr::state`]), which only the game thread locks, briefly. The
//! submit side is held for a whole frame instead: from the game thread's `xrBeginFrame` through the
//! dispatches, and then by the deferred frame tail until its `xrEndFrame` ([`crate::vr::tail`]).
//! Keeping the two apart is what lets the next frame wait on the compositor and locate its views
//! while the previous frame's tail still holds the submit side: OpenXR allows a frame's
//! `xrWaitFrame` once the previous frame has begun, from any thread, and only the next
//! `xrBeginFrame` must follow the previous `xrEndFrame`. See `docs/mod/threading.md`.
//!
//! Lock order: the runtime state, then this. Nothing holding the submit side takes the runtime
//! state, and the tail never takes either: it is handed the frame's guard.

use std::panic::Location;

use anyhow::Context as _;
use jc3vr_core::{frame_timing, frame_timing::Slot, thread_role};
use openxr as xr;
use parking_lot::{Mutex, MutexGuard};

use crate::vr::{VrConfig, state::report_forbidden_lock, swapchain::Swapchain};

/// The submit side of a running session.
pub(super) struct Submitter {
    instance: xr::Instance,
    system: xr::SystemId,
    session: xr::Session<xr::D3D11>,
    pub(super) frame_stream: xr::FrameStream<xr::D3D11>,
    /// A LOCAL reference space of the submit side's own, for the projection layer. The same frame as
    /// the runtime state's, which locates the views in it.
    pub(super) layer_space: xr::Space,
    pub(super) blend_mode: xr::EnvironmentBlendMode,
    /// The stereo swapchain, created lazily on the first frame that acquires an image.
    pub(super) swapchain: Option<Swapchain>,
}

impl Submitter {
    pub(super) fn new(
        instance: xr::Instance,
        system: xr::SystemId,
        session: xr::Session<xr::D3D11>,
        frame_stream: xr::FrameStream<xr::D3D11>,
        blend_mode: xr::EnvironmentBlendMode,
    ) -> anyhow::Result<Self> {
        let layer_space = session
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .context("vr: creating the projection layer's LOCAL reference space")?;
        Ok(Self {
            instance,
            system,
            session,
            frame_stream,
            layer_space,
            blend_mode,
            swapchain: None,
        })
    }

    /// Acquire and wait on the swapchain image, creating the swapchain on first use.
    pub(super) fn acquire_swapchain_image(&mut self, cfg: &VrConfig) -> anyhow::Result<()> {
        if self.swapchain.is_none() {
            self.swapchain = Some(Swapchain::create(
                &self.instance,
                self.system,
                &self.session,
                cfg,
            )?);
        }
        self.swapchain
            .as_mut()
            .expect("the swapchain was just ensured")
            .acquire()
    }

    /// Release the acquired swapchain image.
    pub(super) fn release_swapchain_image(&mut self) -> anyhow::Result<()> {
        self.swapchain
            .as_mut()
            .context("vr: no swapchain to release")?
            .release()
    }
}

/// Make `submitter` the running session's submit side. Waits for a frame in flight to finish first,
/// as every write of the slot does.
pub(super) fn install(submitter: Submitter) {
    *lock_submit() = Some(submitter);
}

/// Take the submit side out, once no frame holds it, for teardown or persistence.
pub(super) fn take() -> Option<Submitter> {
    lock_submit().take()
}

/// Lock the submit side, blocking until a frame in flight (on the deferred tail) has submitted.
/// Checked like the runtime state's lock: only the game thread, outside the frame, may block on it
/// (see `docs/mod/threading.md`), and the wait is timed as the runtime lock.
#[track_caller]
pub(super) fn lock_submit() -> MutexGuard<'static, Option<Submitter>> {
    let role = thread_role::current();
    if !role.may_block_on_vr_state() {
        report_forbidden_lock(role, Location::caller());
    }
    let _timing = frame_timing::scope(Slot::VrLock);
    SUBMIT.lock()
}

/// The running session's submit side, or `None` while no session is up.
static SUBMIT: Mutex<Option<Submitter>> = Mutex::new(None);
