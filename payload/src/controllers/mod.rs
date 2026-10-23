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
//! - [`motion`]: the hands' recent motion, for throws and strikes;
//! - [`scheme`]: controller versus gamepad scheme selection;
//! - [`mode`]: the per-tick mode detector, which selects the OpenXR action set;
//! - [`bindings`]: the per-mode mapping from the controllers to the game's actions, and its
//!   injection on the input tick;
//! - [`grab`]: the grip model: what each hand's grip would take among the targets every subsystem
//!   provides, when it takes it, and the markers that show it;
//! - [`body`]: the player's body dimensions, their calibration in the headset, and the remap onto
//!   the character's proportions;
//! - [`gamepad_setting`]: the game's active-input-device setting held at the gamepad under the
//!   controller scheme;
//! - [`aim_state`]: the aim-state pin, the mental-state cuts, and skipping the player's aim IK;
//! - [`hands`]: controller-owned hands: the wrist IK, the hand tiers, and the render-time late
//!   update;
//! - [`presentation`]: the render-rate presentation layer: the upper body re-posed every rendered
//!   frame from the latest tracking, on top of the pose the tick wrote;
//! - [`haptics`]: the controllers' vibration: one vocabulary of cues for every source, the mixer
//!   that folds them into what each hand plays, and the game's rumble forwarded;
//! - [`fingers`]: finger curls on the rendered hands, from hand tracking or estimated from the
//!   inputs;
//! - [`aim`]: per-hand aiming: the guns along the hands' sight lines and the grapple along the
//!   off-hand's ray;
//! - [`overlay`]: the world-space overlay layer: sprites and lines drawn into the scene per eye;
//! - [`weapons`]: physical weapon handling: the holsters and grab volumes, the two-handed
//!   foregrip, the weapon actions that run with the hands on the controllers, recoil, and
//!   the dual-wield split;
//! - [`physical_hands`]: each empty hand's palm colliding with the world: pushing the body off
//!   surfaces, holding the hand at them, and striking characters and objects;
//! - [`vehicles`]: vehicles, mounted guns, the wingsuit, and the parachute: the opt-in physical
//!   controls, hand-aimed mounted guns and driver weapons, arm steering, and the wingsuit's head
//!   look-steer removed;
//! - [`roomscale`]: posture, the head-height remap, the cockpit composition, the crouch capsule, the
//!   head-through-geometry fade, and seat transitions. These need the headset rather than the
//!   controllers, so they are gated on the VR source and their own toggles instead of the scheme;
//! - [`summary`]: the controls summary on the off-hand wrist, and hiding the game's own button
//!   hints;
//! - [`menu`]: the pause menu's `VR: CONTROLS`, `VR: COMFORT`, and `VR: DISPLAY` pages over the
//!   settings a player reaches for in the headset (the `VR: BODY` page is [`body::menu`]); the
//!   rest are in the debug UI.
//!
//! Every subsystem is gated on its toggle and, where it changes how the character behaves, on the
//! controller scheme ([`scheme::is_controllers`]), so that the mod behaves as before whenever no
//! controllers are in use.

use re_utilities::hook_library::HookLibrary;

pub mod aim;
pub mod aim_state;
pub mod bindings;
pub mod body;
pub mod config;
pub mod diagnostics;
pub mod fingers;
pub mod frame;
pub mod gamepad_setting;
pub mod grab;
pub mod hands;
pub mod haptics;
pub mod loadout;
pub mod locomotion;
pub mod map;
pub(crate) mod menu;
pub mod mode;
pub mod motion;
pub mod overlay;
pub mod physical_hands;
pub mod presentation;
pub mod presets;
pub mod roomscale;
pub mod scheme;
pub mod summary;
pub mod vehicles;
pub mod weapons;

pub use config::ControllersConfig;

/// The controller subsystems' hooks, aggregated into the payload's hook library by
/// [`crate::hooks`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(aim_state::hook_library())
        .with_hook_library(hands::hook_library())
        .with_hook_library(aim::hook_library())
        .with_hook_library(map::hook_library())
        .with_hook_library(roomscale::hook_library())
        .with_hook_library(body::rig::root::hook_library())
        .with_hook_library(weapons::hook_library())
        .with_hook_library(haptics::hook_library())
        .with_hook_library(vehicles::hook_library())
}
