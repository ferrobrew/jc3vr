//! The payload's composition of its hook libraries and lifecycle participants.
//!
//! Each module that installs detours exports a `hook_library()`, and each module whose detours need
//! work at shutdown or hook removal registers it from its own `install`. This module is the one
//! place that names them all: it runs the registrations, then hands [`crate::hooks::install`] the
//! composed library.
//!
//! The order of [`libraries`] is the order the detours are enabled in (and disabled in, at
//! removal), and the registration order is the order the removal steps run in. Both are load-bearing
//! for startup and ejection timing, so neither is reshuffled for grouping's sake.

use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers, debug, diagnostics, director, grapple, hooks, hud, pause_menu, recorder, stereo,
};

/// Register every lifecycle participant and hook step, then install the composed hook library. A
/// refused participant registration leaves the hooks uninstalled.
pub(crate) fn install() {
    // Registered first, so hook removal attempts them whether or not installation got further.
    stereo::register_unpatch::register();
    debug::register_unpatch::register();
    // What the input tick reads from the HUD and the recorder, registered before any tick runs.
    hud::pointer::register_pointer::register();
    recorder::register_mark::register();
    if controllers::install().is_err() {
        return;
    }
    diagnostics::install();
    director::register_tracking::register();
    hooks::install(compose);
}

/// One hook library in the composition, named so the order can be checked.
struct NamedLibrary {
    #[cfg_attr(not(test), expect(dead_code, reason = "read only by the order test"))]
    name: &'static str,
    build: fn() -> HookLibrary,
}

/// The hook libraries, in enable order.
fn libraries() -> Vec<NamedLibrary> {
    let mut libraries = vec![
        library("game", hooks::game::hook_library),
        library("sim", hooks::sim::hook_library),
        library("online", hooks::online::hook_library),
        library("clock", hooks::clock::hook_library),
        library("graphics_engine", hooks::graphics_engine::hook_library),
        library("draw_count", hooks::draw_count::hook_library),
        library("equipment", hooks::equipment::hook_library),
        library("camera", hooks::camera::hook_library),
        library("wndproc", hooks::wndproc::hook_library),
        library("window_focus", hooks::window_focus::hook_library),
        library("character", hooks::character::hook_library),
        library("animation", hooks::animation::hook_library),
        library("input", hooks::input::hook_library),
        library("ui", hooks::ui::hook_library),
        library("pause_menu", pause_menu::hook_library),
        library("controllers", controllers::hook_library),
        library(
            "human_hull",
            controllers::physical_hands::human_hull::hook_library,
        ),
        library("grapple_physical", grapple::physical::hook_library),
        library("grapple_device", grapple::device::hook_library),
    ];
    #[cfg(feature = "profiler")]
    libraries.push(library("profiler", hooks::profiler::hook_library));
    libraries
}

fn library(name: &'static str, build: fn() -> HookLibrary) -> NamedLibrary {
    NamedLibrary { name, build }
}

/// Build every library in [`libraries`] order, as children of one root. A child library's binders
/// enable after its parent's and in the order it was added, so the root adds nothing to the order.
fn compose() -> HookLibrary {
    libraries()
        .into_iter()
        .fold(HookLibrary::new(), |root, library| {
            root.with_hook_library((library.build)())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "controllers",
            "human_hull",
            "grapple_physical",
            "grapple_device",
        ];
        if cfg!(feature = "profiler") {
            expected.push("profiler");
        }
        let names: Vec<_> = libraries().iter().map(|library| library.name).collect();
        assert_eq!(names, expected);
    }
}
