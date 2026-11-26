//! Motion controllers: the controller scheme's input, hands, aiming, weapon handling, and roomscale
//! locomotion, following the plan of record in `docs/mod/input/controllers-and-roomscale.md`.
//!
//! The OpenXR side (action sets, suggested bindings, locating the controller spaces, haptics
//! output, hand tracking) lives in [`jc3vr_render::vr::input`], which publishes one
//! [`ControllerFrame`](jc3vr_input::frame::ControllerFrame) per rendered frame; the snapshot, the
//! hands' recent motion, and the scheme rule are the input crate's ([`jc3vr_input`]). This module
//! owns everything built on that snapshot:
//!
//! - [`config`]: [`ControllersConfig`], the controllers' settings assembled from each subsystem's
//!   group;
//! - [`scheme`]: controller versus gamepad scheme selection;
//! - [`mode`]: the per-tick mode detector, which selects the OpenXR action set;
//! - [`bindings`]: the per-mode mapping from the controllers to the game's actions, and its
//!   injection on the input tick;
//! - [`clock`]: the game's clock, which its slow motion slows, against the player's real one;
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
//! - [`haptics`]: the controllers' vibration: one vocabulary of cues for every source, which the
//!   input crate's mixer folds into what each hand plays, and the game's rumble forwarded;
//! - [`fingers`]: finger curls on the rendered hands, from hand tracking or estimated from the
//!   inputs;
//! - [`aim`]: per-hand aiming: the guns along the hands' sight lines and the grapple along the
//!   off-hand's ray;
//! - [`overlay`]: the world-space overlay layer: sprites and lines drawn into the scene per eye;
//! - [`weapons`]: physical weapon handling: the holsters and grab volumes, the two-handed
//!   foregrip, the weapon actions that run with the hands on the controllers, recoil, and
//!   the dual-wield split;
//! - [`snatch`]: the grapple snatch: the empty grapple hand pulls a weapon out of reach into itself
//!   on the game's hook;
//! - [`spawn`]: the spawn system's requests and the shipped spawn definition table, for the
//!   inventory's gives and the director's spawner;
//! - [`physical_hands`]: each empty hand's palm colliding with the world: pushing the body off
//!   surfaces, holding the hand at them, and striking characters and objects;
//! - [`glass`]: breaking window glass with a palm or a held gun, as bullets crack it, and bringing
//!   a pane down with a hard enough strike;
//! - [`vehicles`]: vehicles, mounted guns, the wingsuit, and the parachute: the opt-in physical
//!   controls, hand-aimed mounted guns and driver weapons, arm steering, and the wingsuit's head
//!   look-steer removed;
//! - [`roomscale`]: posture, the head-height remap, the cockpit composition, the crouch capsule, the
//!   head-through-geometry fade, and seat transitions. These need the headset rather than the
//!   controllers, so they are gated on the VR source and their own toggles instead of the scheme;
//! - [`summary`]: the controls summary's model, which the HUD draws on the off-hand wrist;
//! - [`telemetry`]: the notes the subsystems write to the tracking recording;
//! - [`intent`], [`held`], [`physics`], and [`facts`]: the vocabulary, the held-item facts, the
//!   physics substrate, and the published facts that the subsystems share across their layering.
//!
//! The controls map and the summary's presentation belong to the HUD layer, and the pause menu's
//! `VR:` pages to the UI layer.
//!
//! Every subsystem is gated on its toggle and, where it changes how the character behaves, on the
//! controller scheme ([`scheme::is_controllers`]), so that the mod behaves as before whenever no
//! controllers are in use.

use jc3vr_core::lifecycle::RegistrationError;
use re_utilities::hook_library::HookLibrary;

pub mod aim;
pub mod aim_state;
pub mod bindings;
pub mod body;
pub mod clock;
pub mod config;
pub mod facts;
pub mod fingers;
pub mod gamepad_setting;
pub mod glass;
pub mod grab;
pub mod hands;
pub mod haptics;
pub mod held;
pub mod intent;
pub mod loadout;
pub mod locomotion;
pub mod mode;
pub mod overlay;
pub mod physical_hands;
pub mod physics;
pub mod presentation;
mod register_head_pose;
mod register_lifecycle;
mod register_render;
mod register_vr_frame;
pub mod roomscale;
pub mod scheme;
pub mod snatch;
pub mod spawn;
pub mod summary;
pub(crate) mod telemetry;
pub mod vehicles;
pub mod weapons;

pub use config::ControllersConfig;

/// Register the controller subsystems' shutdown participants and their hook installation and
/// removal steps. Runs before the hooks are installed; an error refuses hook installation, and the
/// refusal is already logged.
pub fn install() -> Result<(), RegistrationError> {
    register_head_pose::register();
    register_vr_frame::register();
    register_render::register();
    register_lifecycle::register()
}

/// Register the head pose's participants once, for a test that drives the head composition
/// without the install that registers them.
#[cfg(test)]
pub(crate) fn register_head_participants_for_tests() {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(register_head_pose::register);
}

/// The controller subsystems' hooks, aggregated into the payload's hook library by its composition.
///
/// The HUD layer's controls map hooks enable inside this library, after the aim's and before the
/// roomscale's, so the composition passes them in as `map`.
pub fn hook_library(map: HookLibrary) -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(aim_state::hook_library())
        .with_hook_library(hands::hook_library())
        .with_hook_library(aim::hook_library())
        .with_hook_library(map)
        .with_hook_library(roomscale::hook_library())
        .with_hook_library(body::rig::root::hook_library())
        .with_hook_library(body::firm_ragdoll::hook_library())
        .with_hook_library(weapons::hook_library())
        .with_hook_library(snatch::hook_library())
        .with_hook_library(haptics::hook_library())
        .with_hook_library(physical_hands::impacts::hook_library())
        .with_hook_library(physical_hands::lifetime::hook_library())
        .with_hook_library(vehicles::hook_library())
        .with_hook_library(glass::hook_library())
}
