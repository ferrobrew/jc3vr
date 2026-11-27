//! The OpenXR runtime: session lifecycle, event pump, and the per-frame API surface the Draw wiring
//! drives. This module owns the OpenXR instance, session, reference spaces, and stereo swapchain. The
//! per-frame loop ([`update`] → [`frame_wait`] → [`WaitedFrame::begin`] → [`FrameContext`] →
//! per-eye blit → [`present_and_submit`]) is driven from the payload's `game_update_render`; the
//! per-eye render parameters flow to the camera hook and the draw task through [`frame`]'s per-frame
//! slots ([`render_params`]), not the runtime's locks. See `docs/mod/vr-runtime.md` for the loop end
//! to end.
//!
//! ## Loader route
//!
//! The OpenXR loader is **dynamically loaded at runtime** (`xr::Entry::load_from`), not linked. The
//! `static` loader route (build the Khronos loader through cmake against the xwin/clang-cl cross
//! toolchain) does not build in this environment -- cmake selects the Ninja generator, which the
//! cross toolchain lacks -- so the portable choice is the runtime loader. The loader DLL defaults to
//! `openxr_loader.dll` next to the payload DLL ([`jc3vr_core::module::get_path`]) and is overridable via
//! [`crate::vr::VrConfig::loader_path`]. A loader that will not load is a deployment fault, so the
//! bring-up treats it as fatal rather than retrying (see the `loader` module).
//!
//! ## Threading
//!
//! The runtime state is a single `Mutex<VrState>` singleton, which the game thread locks briefly at
//! the top of the render update. The submit side (the frame stream and the swapchain, [`submit`]) is
//! a second lock, held from [`WaitedFrame::begin`] for the frame and taken over by the deferred frame
//! tail ([`tail`]) until it submits; both holders wait on engine threads meanwhile, so every blocking
//! lock of either goes through a thread-role check (see [`jc3vr_core::thread_role`] and
//! `docs/mod/threading.md`), and other threads read the published slots instead. The game's
//! `ID3D11Device` is fetched from the graphics engine singleton at session-create time,
//! null-guarded; the device is never stored (so the state carries no raw device pointer across
//! threads). All OpenXR handles are `Send` (`Arc`-backed handles), so the state is a safe singleton.
//!
//! ## Degradation and retry
//!
//! Bring-up failure at any stage logs on target `"vr"` and leaves the mod in flatscreen stereo;
//! [`update`] retries the whole bring-up on the cadence of the runtime tuning's `retry_interval_s`
//! while `vr.enabled`. Turning `vr.enabled` off, or [`jc3vr_core::lifecycle`] shutdown, tears the runtime down
//! in order (swapchain → session → instance) so the OpenXR instance never outlives the DLL.

pub mod projection;

pub use config::{FOVEATION_CONFIG, FoveationConfig, FreezeMode, MirrorView, VR_CONFIG, VrConfig};
pub use frame::{
    CaptureEyeView, CaptureViews, EyeRenderParams, begin_render_frame, capture_views,
    clear_render_params, cull_projection_standard, eyes_union_projection_standard, render_params,
    shadow_fit_projection_standard,
};
use openxr as xr;
pub use projection::{Fov, OffAxisProjection};

mod back_buffer;
mod blit;
mod blit_region;
mod blit_timer;
mod config;
pub(crate) mod convert;
mod eye_resolution;
pub mod foveation;
mod frame;
mod frame_loop;
pub mod harness;
pub mod input;
pub(crate) mod live_tracking;
mod loader;
mod mirror;
mod participants;
mod persist;
pub mod pose_control;
mod recenter;
mod resolution;
mod session;
pub mod stage;
mod state;
mod submit;
mod swapchain;
pub mod tail;
mod visibility_mask;
pub mod window;

pub use back_buffer::{
    owned as back_buffer_owned, resize_substitute_bypassed, substitute_render_setups,
    sync_swapchain_to_window,
};
pub use blit::{
    DirectArm, DirectStatus, arm_direct, blit_direct, direct_status, disarm_direct, fade_level,
    present_and_submit, set_fade_level, texture_size,
};
pub use blit_region::BlitSource;
pub(crate) use eye_resolution::engine_render_resolution;
pub use frame_loop::{
    EyeImage, EyeView, FrameContext, LocatedFrame, WaitedFrame, frame_wait, head_tracked,
    pose_age_totals,
};
pub use mirror::{
    EguiPanelConfig, MirrorDraw, MirrorFrame, MirrorLayers, MirrorObserver, PANEL_CONFIG,
    latest_present, mirror_layers_registered, mirror_observer_registered, present_mirror,
    register_mirror_layers, register_mirror_observer, toggle_stereo_pair,
};
pub use participants::{
    CockpitPositions, FrameInputs, FrameParticipants, ResizeObserver, TeardownParticipant,
    frame_participants_registered, register_frame_participants, register_resize_observer,
    register_teardown_participant, resize_observer_registered, teardown_participant_registered,
};
pub use recenter::{
    RecenterCause, RecenterKind, auto_recenter_tick, rebase_count, recenter, recenter_count,
    request_recenter, request_seat_recenter, request_seat_yaw_rebase,
};
pub use resolution::{
    ResolutionProgress, ResolutionStatus, apply_native_resolution, progress as resolution_progress,
    status as resolution_status,
};
pub use state::{
    PublishedStatus, VrStatus, install, is_running, is_running_published, published_status, status,
    uninstall, update, wait_for_frame_tail,
};
pub use visibility_mask::{EyeMask, MaskStatus, status as visibility_mask_status};

/// The OpenXR view configuration: standard stereo, two views (one per eye).
const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
/// The number of views (eyes), and the swapchain array size (one slice per eye).
const VIEW_COUNT: u32 = 2;
