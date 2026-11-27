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
//! [`register`] registers what these contribute to the layers below and fills the body's
//! contributions this crate declares the order of. The controls map and the summary's presentation
//! belong to the HUD layer, and the pause menu's `VR:` pages to the UI layer.
//!
//! Every subsystem is gated on its toggle and, where it changes how the character behaves, on the
//! controller scheme ([`jc3vr_body::controllers::scheme::is_controllers`]), so that the mod
//! behaves as before whenever no controllers are in use.

use jc3vr_body::contributions::BodyContributions;
use jc3vr_core::registration::RegistrationCheck;

pub mod bindings;
pub mod config;
mod register_aim;
mod register_character;
mod register_hands;
mod register_head_pose;
mod register_render;
mod register_vr_frame;
pub mod summary;
pub mod tuning;

pub use config::ControllersConfig;

/// Register what the controller subsystems contribute to the layers below them (the head pose's,
/// the VR frame's, and the render layer's participants), and fill the body's contributions whose
/// order this crate declares (the aim's hooks, the hands' stage and observer tables, the grip
/// sources' order, and the character hooks' refusals). Runs before the hooks are installed; it
/// reports what is still missing to `check`, and the body's registration reports its fields.
pub fn register(body: &mut BodyContributions, check: &mut RegistrationCheck) {
    register_head_pose::register(check);
    register_vr_frame::register(check);
    register_render::register(check);
    register_hands::register(body);
    register_aim::register(body);
    register_character::register(body);
}
