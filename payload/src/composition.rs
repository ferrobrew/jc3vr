//! The payload's composition of every layer crate's registrations and hook libraries.
//!
//! This module is the one place that names them all. [`register`] calls each crate's one
//! registration, builds the body's and the weapon handling's contributions records from what the
//! crates fill, and registers the native-callback participants; every registration reports what it
//! finds missing to one [`RegistrationCheck`]. [`install`] finishes the check, then hands
//! [`jc3vr_core::hooks::install`] the hook libraries composed in [`libraries`] order.
//!
//! Three orders are real constraints, and each is kept where it is made:
//!
//! - Every registration happens before [`core_hooks::install`]: a shutdown participant must be
//!   registered before its detour can accept work, the steps after enabling run once, when the
//!   hooks are enabled, and the check must see every contribution before it decides whether the
//!   hooks are installed at all.
//! - Stereo's step at hook removal registers before the texture census's, inside
//!   [`jc3vr_render::register`]: the removal steps run in registration order and stop at the first
//!   that refuses, so the single-pass COM detours are torn down even when the census refuses.
//! - The magazine executor's admission closes before the check that no native callback is still
//!   running, inside [`crate::native_callbacks`], for the same reason: it never refuses, and it must
//!   close even when a check after it keeps the image mapped.
//!
//! Every other registration fills a slot or a field of its own, so their order is free. The hook
//! libraries enable in [`libraries`] order (and disable in it, at removal), all under one thread
//! suspension, and no two of them detour the same function, so the game cannot observe the order;
//! the order test pins it so that a change to it is deliberate.

use std::fmt;

use jc3vr_body::{
    contributions::{self as body_contributions, BodyContributions},
    controllers::{aim, aim_state, body, hands, haptics, roomscale},
    hooks::{animation, character},
};
use jc3vr_core::{
    hooks as core_hooks,
    lifecycle::RegistrationError,
    registration::{MissingRegistrations, RegistrationCheck},
};
use jc3vr_gameplay::hooks::{input, online, sim};
use jc3vr_grapple::{controllers::snatch, grapple};
use jc3vr_hud::{controllers::map, hooks::ui};
use jc3vr_physical::controllers::{
    glass, physical_hands,
    physical_hands::{grab_view, held_props, human_hull},
};
#[cfg(feature = "profiler")]
use jc3vr_render::hooks::profiler;
use jc3vr_render::hooks::{clock, draw_count, graphics_engine};
use jc3vr_ui::pause_menu;
use jc3vr_vehicles::{Registered as VehiclesRegistered, vehicles, vehicles::mounted_recoil};
use jc3vr_view::hooks::{camera, window_focus};
use jc3vr_weapons::{
    contributions::{self as weapons_contributions, WeaponsContributions},
    controllers::weapons,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    hooks::{game, wndproc},
    native_callbacks,
};

/// Register every crate's contributions and lifecycle participants, then install the composed hook
/// library. A refused participant registration or a missing contribution leaves the hooks
/// uninstalled.
pub(crate) fn install() -> Result<(), InstallError> {
    let mut check = RegistrationCheck::new();
    let registered = register(&mut check).map_err(InstallError::Participant)?;
    check.finish().map_err(InstallError::Missing)?;
    core_hooks::install(|| compose(&registered));
    Ok(())
}

/// Why the composition did not install the hooks.
#[derive(Debug)]
pub(crate) enum InstallError {
    /// The lifecycle refused a shutdown participant.
    Participant(RegistrationError),
    /// Contributions a lower layer reads were left unregistered.
    Missing(MissingRegistrations),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("composition: ")?;
        match self {
            Self::Participant(error) => write!(f, "{error}"),
            Self::Missing(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Participant(error) => Some(error),
            Self::Missing(error) => Some(error),
        }
    }
}

/// Run every crate's registration, reporting what each finds missing to `check`, and return what
/// the vehicles crate hands back for its equipment detours. A refused participant registration
/// stops it; the refusal is already logged.
fn register(check: &mut RegistrationCheck) -> Result<VehiclesRegistered, RegistrationError> {
    jc3vr_render::register(check);
    jc3vr_capture::register(check);

    // The body's contributions: each crate fills its own fields, the gameplay crate the ordered
    // tables, and the body reports every field still empty when the record is registered.
    let mut body = BodyContributions::default();
    let registered = jc3vr_vehicles::register(&mut body, check);
    jc3vr_grapple::register(&mut body, check);
    jc3vr_physical::register(&mut body);
    jc3vr_weapons::register(&mut body);
    jc3vr_gameplay::register(&mut body, check);
    jc3vr_hud::register(check);
    jc3vr_tooling::register(&mut body, check);
    body_contributions::register(body, check);

    // What the weapon handling reads from the crates beside and above it: the handles' kick by each
    // shot of the mounted gun the hands aim, the grip view's flash for a refused take of a weapon,
    // and the props the hands carry, which the loadout table records.
    weapons_contributions::register(WeaponsContributions {
        mounted_shot: mounted_recoil::on_shot,
        take_refused: grab_view::take_refused,
        held_props,
    });
    check.require(
        "weapons",
        "the weapon handling's contributions",
        weapons_contributions::registered(),
    );

    native_callbacks::register()?;
    Ok(registered)
}

/// One hook library in the composition, named so the order can be checked.
struct NamedLibrary {
    #[cfg_attr(not(test), expect(dead_code, reason = "read only by the order test"))]
    name: &'static str,
    build: fn() -> HookLibrary,
}

/// The hook libraries, in enable order. The vehicles crate's equipment detours come from the value
/// its registration returns.
fn libraries(registered: &VehiclesRegistered) -> Vec<NamedLibrary> {
    #[cfg_attr(
        not(feature = "profiler"),
        expect(
            unused_mut,
            reason = "only the profiler's library is pushed after the list"
        )
    )]
    let mut libraries = vec![
        library("game", game::hook_library),
        library("sim", sim::hook_library),
        library("online", online::hook_library),
        library("clock", clock::hook_library),
        library("graphics_engine", graphics_engine::hook_library),
        library("draw_count", draw_count::hook_library),
        library("equipment", registered.equipment_hooks()),
        library("camera", camera::hook_library),
        library("wndproc", wndproc::hook_library),
        library("window_focus", window_focus::hook_library),
        library("character", character::hook_library),
        library("animation", animation::hook_library),
        library("input", input::hook_library),
        library("ui", ui::hook_library),
        library("pause_menu", pause_menu::hook_library),
        library("aim_state", aim_state::hook_library),
        library("hands", hands::hook_library),
        library("aim", aim::hook_library),
        library("map", map::hook_library),
        library("roomscale", roomscale::hook_library),
        library("rig_root", body::rig::root::hook_library),
        library("firm_ragdoll", body::firm_ragdoll::hook_library),
        library("weapons", weapons::hook_library),
        library("snatch", snatch::hook_library),
        library("haptics", haptics::hook_library),
        library("impacts", physical_hands::impacts::hook_library),
        library("lifetime", physical_hands::lifetime::hook_library),
        library("vehicles", vehicles::hook_library),
        library("glass", glass::hook_library),
        library("human_hull", human_hull::hook_library),
        library("grapple_physical", grapple::physical::hook_library),
        library("grapple_device", grapple::device::hook_library),
    ];
    #[cfg(feature = "profiler")]
    libraries.push(library("profiler", profiler::hook_library));
    libraries
}

fn library(name: &'static str, build: fn() -> HookLibrary) -> NamedLibrary {
    NamedLibrary { name, build }
}

/// Build every library in [`libraries`] order, as children of one root. A child library's binders
/// enable after its parent's and in the order it was added, so the root adds nothing to the order.
fn compose(registered: &VehiclesRegistered) -> HookLibrary {
    libraries(registered)
        .into_iter()
        .fold(HookLibrary::new(), |root, library| {
            root.with_hook_library((library.build)())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registrations_leave_nothing_missing() {
        let mut check = RegistrationCheck::new();
        register(&mut check).expect("the participants register once");
        assert_eq!(check.missing(), []);
    }

    #[test]
    fn the_libraries_compose_in_the_established_enable_order() {
        let mut expected = vec![
            "game",
            "sim",
            "online",
            "clock",
            "graphics_engine",
            "draw_count",
            "equipment",
            "camera",
            "wndproc",
            "window_focus",
            "character",
            "animation",
            "input",
            "ui",
            "pause_menu",
            "aim_state",
            "hands",
            "aim",
            "map",
            "roomscale",
            "rig_root",
            "firm_ragdoll",
            "weapons",
            "snatch",
            "haptics",
            "impacts",
            "lifetime",
            "vehicles",
            "glass",
            "human_hull",
            "grapple_physical",
            "grapple_device",
        ];
        if cfg!(feature = "profiler") {
            expected.push("profiler");
        }
        let registered = jc3vr_vehicles::register(
            &mut BodyContributions::default(),
            &mut RegistrationCheck::new(),
        );
        let names: Vec<_> = libraries(&registered)
            .iter()
            .map(|library| library.name)
            .collect();
        assert_eq!(names, expected);
    }
}
