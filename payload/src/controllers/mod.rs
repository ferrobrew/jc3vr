//! Motion controllers: the controller scheme's input, hands, aiming, weapon handling, and roomscale
//! locomotion, following the plan of record in `docs/mod/input/controllers-and-roomscale.md`.
//!
//! The OpenXR side (action sets, suggested bindings, locating the controller spaces, haptics
//! output, hand tracking) lives in [`crate::vr::input`], which publishes one
//! [`ControllerFrame`](frame::ControllerFrame) per rendered frame. This module owns everything
//! built on that snapshot:
//!
//! - [`config`]: [`ControllersConfig`], one toggle per subsystem;
//! - [`frame`]: the published snapshot and the input edge latch;
//! - [`scheme`]: controller versus gamepad scheme selection;
//! - [`body`]: the player's body dimensions and the remap onto the character's proportions.
//!
//! Every subsystem is gated on its toggle and, where it changes how the character behaves, on the
//! controller scheme ([`scheme::is_controllers`]), so that the mod behaves as before whenever no
//! controllers are in use.

use re_utilities::hook_library::HookLibrary;

// The remap is milestone 2's; until the posture and hand work call it, only its tests and the
// debug UI's readout of the loaded dimensions do.
#[allow(
    dead_code,
    reason = "the body-scale remap is published for the hands and posture subsystems"
)]
pub mod body;
pub mod config;
pub mod frame;
pub mod scheme;

pub use config::ControllersConfig;

/// The controller subsystems' hooks, aggregated into the payload's hook library by
/// [`crate::hooks`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
}
