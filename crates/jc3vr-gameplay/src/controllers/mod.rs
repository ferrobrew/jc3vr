//! Motion controllers built on the body layer: the weapon handling, the physical hands, the
//! vehicles, and the bindings, following the plan of record in
//! `docs/mod/input/controllers-and-roomscale.md`.
//!
//! The OpenXR side (action sets, suggested bindings, locating the controller spaces, haptics
//! output, hand tracking) lives in [`jc3vr_view::vr::input`], which publishes one
//! [`ControllerFrame`](jc3vr_input::frame::ControllerFrame) per rendered frame; the snapshot, the
//! hands' recent motion, and the scheme rule are the input crate's ([`jc3vr_input`]). The
//! subsystems every other one builds on (the scheme, the mode detector, the clock, the grip model,
//! the body, the hands, fingers, and presentation, the aim and the aim-state pin, the haptics, the
//! overlay producers, locomotion and roomscale, the spawn table, the telemetry notes, and the
//! shared vocabulary, held-item facts, physics substrate, and published facts) are the body
//! crate's, under [`jc3vr_body::controllers`]. Physical weapon handling with the loadout table is
//! the weapons crate's, under [`jc3vr_weapons::controllers`], the physical hands and the glass are
//! the physical crate's, under [`jc3vr_physical::controllers`], and the vehicles, mounted guns, the
//! wingsuit, and the parachute are the vehicles crate's, under [`jc3vr_vehicles::controllers`]. The
//! grapple snatch is the grapple crate's, under [`jc3vr_grapple::controllers::snatch`]. This module
//! owns what builds on them:
//!
//! - [`config`]: [`ControllersConfig`], the controllers' settings assembled from each subsystem's
//!   group;
//! - [`bindings`]: the per-mode mapping from the controllers to the game's actions, and its
//!   injection on the input tick;
//! - [`summary`]: the controls summary's model, which the HUD draws on the off-hand wrist.
//!
//! [`install`] registers what these contribute to the body crate's registrations, and
//! [`hook_library`] composes the body-bound subsystems' hook libraries with these in their
//! established order. The controls map and the summary's presentation belong to the HUD layer, and
//! the pause menu's `VR:` pages to the UI layer.
//!
//! Every subsystem is gated on its toggle and, where it changes how the character behaves, on the
//! controller scheme ([`jc3vr_body::controllers::scheme::is_controllers`]), so that the mod
//! behaves as before whenever no controllers are in use.

use jc3vr_body::controllers::{aim, aim_state, body, hands, haptics, roomscale};
use jc3vr_core::lifecycle::RegistrationError;
use jc3vr_grapple::controllers::snatch;
use jc3vr_physical::controllers::{glass, physical_hands};
use jc3vr_vehicles::controllers::vehicles;
use jc3vr_weapons::controllers::weapons;
use re_utilities::hook_library::HookLibrary;

pub mod bindings;
pub mod config;
mod register_aim;
mod register_below;
mod register_hands;
mod register_head_pose;
mod register_lifecycle;
mod register_render;
mod register_siblings;
mod register_vr_frame;
pub mod summary;
pub mod tuning;

pub use config::ControllersConfig;

/// Register the controller subsystems' shutdown participants and their hook installation and
/// removal steps. Runs before the hooks are installed; an error refuses hook installation, and the
/// refusal is already logged.
pub fn install() -> Result<(), RegistrationError> {
    register_head_pose::register();
    register_vr_frame::register();
    register_render::register();
    register_hands::register();
    register_aim::register();
    register_below::register();
    register_lifecycle::register()
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
