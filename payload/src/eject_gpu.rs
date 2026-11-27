//! Releasing the GPU resources the render paths hold in statics, at eject.
//!
//! A payload static is never dropped: the DLL unmaps without running destructors, so a Direct3D
//! object kept in one stays referenced on the game's device for the rest of the process, and every
//! eject and re-injection adds another set. Holders that build on the render thread and keep their
//! resources in a static release them here, once the eject has committed and the engine has been
//! resized back. The eject renders no further frames, so nothing rebuilds them afterwards.
//!
//! The release logs the game process's video memory before and after (as the adapter reports it),
//! so a re-injection loop that still grows shows how much each eject returned.

use jc3gi::graphics_engine::graphics_engine::GraphicsEngine;
use jc3vr_core::lifecycle;
use jc3vr_hud::hud;
use jc3vr_render::{
    controllers::{overlay, scope},
    fsr,
};
use windows::{
    Win32::{
        Graphics::Dxgi::{DXGI_MEMORY_SEGMENT_GROUP_LOCAL, IDXGIAdapter3, IDXGIDevice},
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface as _,
};

/// Register the release with the eject's cleanups. Call before any other cleanup is registered:
/// the cleanups run in reverse, so this one runs last, after the VR teardown and the resize back to
/// the native resolution.
pub(crate) fn install() {
    lifecycle::on_cleanup(|_renderer| release());
}

fn release() {
    let before = video_memory_mib();
    let steps: [(&str, fn()); 8] = [
        ("hud", hud::release_on_eject),
        ("fsr", fsr::release_on_eject),
        ("overlay", overlay::pass::release_on_eject),
        ("scope", scope::pass::release_on_eject),
        ("world_markers", hud::world_markers::release_on_eject),
        ("atlas", hud::atlas::release_on_eject),
        ("mirror_overlay", hud::mirror_overlay::release_on_eject),
        ("egui_panel", hud::egui_panel::release_on_eject),
    ];
    for (holder, release) in steps {
        let held = video_memory_mib();
        release();
        flush();
        tracing::debug!(
            holder,
            before_mib = held,
            after_mib = video_memory_mib(),
            "eject: released"
        );
    }
    tracing::info!(
        before_mib = before,
        after_mib = video_memory_mib(),
        "eject: released the render paths' GPU resources"
    );
}

/// Log the game process's video memory at a point of the payload's lifecycle (`stage`), after
/// submitting the work recorded so far so that released resources are returned.
pub(crate) fn log_video_memory(stage: &'static str) {
    flush();
    tracing::info!(stage, mib = video_memory_mib(), "video memory");
}

/// Submit the immediate context's recorded work and give the device a moment to retire it, so the
/// memory of resources released since is returned before the next reading.
fn flush() {
    // SAFETY: the engine singleton and its device wrapper are live for the process; the context is
    // used under the engine's own mutex.
    unsafe {
        let Some(device) = GraphicsEngine::get().and_then(|ge| ge.m_Device.as_ref()) else {
            return;
        };
        let Some(context) = device.m_Context.as_ref() else {
            return;
        };
        EnterCriticalSection(context.m_Mutex);
        context.m_Context.Flush();
        LeaveCriticalSection(context.m_Mutex);
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
}

/// The game process's local video memory use in MiB, as the adapter reports it, or `None` when the
/// device or its adapter is unavailable.
fn video_memory_mib() -> Option<u64> {
    // SAFETY: the engine singleton and its device wrapper are live for the process; this runs on
    // the game thread during the eject, after the draw thread has stopped rendering.
    let device = unsafe { GraphicsEngine::get()?.m_Device.as_ref()? };
    let dxgi = device.m_Device.cast::<IDXGIDevice>().ok()?;
    // SAFETY: plain COM queries on a live device.
    let adapter = unsafe { dxgi.GetAdapter() }
        .ok()?
        .cast::<IDXGIAdapter3>()
        .ok()?;
    let mut info = Default::default();
    // SAFETY: `info` is a valid out-parameter for the local segment group of node 0.
    unsafe { adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info) }.ok()?;
    Some(info.CurrentUsage / (1024 * 1024))
}
