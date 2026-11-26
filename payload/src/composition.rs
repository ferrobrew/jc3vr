//! The payload's composition of its hook libraries and lifecycle participants.
//!
//! Each module that installs detours exports a `hook_library()`, and each module whose detours need
//! work at shutdown or hook removal registers it from its own `install`. This module is the one
//! place that names them all: it runs the registrations, then hands [`jc3vr_core::hooks::install`] the
//! composed library.
//!
//! The order of [`libraries`] is the order the detours are enabled in (and disabled in, at
//! removal), and the registration order is the order the removal steps run in. Both are load-bearing
//! for startup and ejection timing, so neither is reshuffled for grouping's sake.

use jc3vr_body::hooks::{animation, character};
use jc3vr_capture::screenshot::harness::{register_pipeline, register_single_pass};
use jc3vr_core::hooks::{self as core_hooks, online};
use jc3vr_gameplay::{
    controllers, grapple,
    hooks::{equipment, input, sim},
};
use jc3vr_hud::{controllers::map, egui_impl::register_desktop_overlay, hooks::ui, hud};
use jc3vr_physical::controllers::physical_hands::human_hull;
#[cfg(feature = "profiler")]
use jc3vr_render::hooks::profiler;
use jc3vr_render::{
    controllers::register_passes,
    debug,
    hooks::{clock, draw_count, graphics_engine},
    stereo,
};
use jc3vr_tooling::{diagnostics, director, recorder};
use jc3vr_ui::pause_menu;
use jc3vr_view::hooks::{camera, window_focus};
use re_utilities::hook_library::HookLibrary;

use crate::hooks::{game, wndproc};

/// Register every lifecycle participant and hook step, then install the composed hook library. A
/// refused participant registration leaves the hooks uninstalled.
pub(crate) fn install() {
    // Registered first, so hook removal attempts them whether or not installation got further.
    stereo::register_unpatch::register();
    debug::register_unpatch::register();
    // The render layer's own passes in the post-draw.
    register_passes::register();
    // What the layers above the render hooks contribute to the post-draw, the mirror, the camera,
    // and the flip.
    hud::register_render::register();
    recorder::register_render::register();
    register_desktop_overlay::register();
    // What the debug tools and the graphics-engine hooks contribute to the VR runtime and the
    // camera hooks below them.
    debug::register_view::register();
    graphics_engine::register_vr_teardown::register();
    // The capture harness's probes into the single-pass draw path.
    register_single_pass::register();
    // What the capture tooling observes at the graphics-engine pipeline's seams.
    register_pipeline::register();
    // What the input tick reads from the HUD and the recorder, registered before any tick runs.
    hud::pointer::register_pointer::register();
    recorder::register_mark::register();
    hud::grab_hint::register_gate::register();
    if controllers::install().is_err() {
        return;
    }
    diagnostics::install();
    director::register_tracking::register();
    core_hooks::install(compose);
}

/// One hook library in the composition, named so the order can be checked.
struct NamedLibrary {
    #[cfg_attr(not(test), expect(dead_code, reason = "read only by the order test"))]
    name: &'static str,
    build: fn() -> HookLibrary,
}

/// The hook libraries, in enable order.
fn libraries() -> Vec<NamedLibrary> {
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
        library("equipment", equipment::hook_library),
        library("camera", camera::hook_library),
        library("wndproc", wndproc::hook_library),
        library("window_focus", window_focus::hook_library),
        library("character", character::hook_library),
        library("animation", animation::hook_library),
        library("input", input::hook_library),
        library("ui", ui::hook_library),
        library("pause_menu", pause_menu::hook_library),
        library("controllers", || {
            controllers::hook_library(map::hook_library())
        }),
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
