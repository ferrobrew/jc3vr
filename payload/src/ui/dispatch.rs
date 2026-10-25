//! Production feature routing and links to shared control owners.

#[cfg(test)]
#[path = "dispatch/director_tests.rs"]
mod director_tests;
#[cfg(test)]
#[path = "dispatch/environment_tests.rs"]
mod environment_tests;
#[cfg(test)]
#[path = "dispatch/hud_tests.rs"]
mod hud_tests;
#[cfg(test)]
#[path = "dispatch/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "dispatch/video_tests.rs"]
mod video_tests;
#[cfg(test)]
#[path = "dispatch/visualizations_tests.rs"]
mod visualizations_tests;

use egui::Ui;

use crate::ui::{
    camera,
    catalog::{FeatureId, FeatureSection},
    controllers, diagnostics, director, environment, game, hud,
    navigation::NavigationState,
    overview, performance, previews, render, testing, vr,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FeatureRoute {
    Overview,
    Controller,
    HeadView,
    Runtime,
    Render,
    Hud,
    Previews,
    Performance,
    RenderDiagnostics,
    Tracking,
    Replay,
    Director,
    Environment,
    Engine,
}

pub(crate) fn route(id: FeatureId) -> FeatureRoute {
    match id {
        FeatureId::Session | FeatureId::AllStatus | FeatureId::ActiveTools => {
            FeatureRoute::Overview
        }
        FeatureId::HeadView => FeatureRoute::HeadView,
        FeatureId::Runtime => FeatureRoute::Runtime,
        FeatureId::Stereo | FeatureId::Resolution | FeatureId::Graphics => FeatureRoute::Render,
        FeatureId::Hud => FeatureRoute::Hud,
        FeatureId::Previews => FeatureRoute::Previews,
        FeatureId::Performance => FeatureRoute::Performance,
        FeatureId::RenderDiagnostics | FeatureId::RenderCapture => FeatureRoute::RenderDiagnostics,
        FeatureId::TrackingCapture => FeatureRoute::Tracking,
        FeatureId::Replay => FeatureRoute::Replay,
        FeatureId::DirectorSafety
        | FeatureId::DirectorSpawn
        | FeatureId::Video
        | FeatureId::Keyboard => FeatureRoute::Director,
        FeatureId::Environment => FeatureRoute::Environment,
        FeatureId::Engine | FeatureId::Swimming => FeatureRoute::Engine,
        _ => FeatureRoute::Controller,
    }
}

pub(crate) fn feature(
    ui: &mut Ui,
    selection: &mut NavigationState,
    renderer: Option<&mut egui_directx11::Renderer>,
) {
    let id = selection.feature;
    let section = selection.section;
    match route(id) {
        FeatureRoute::Overview => overview::render(ui, id, selection),
        FeatureRoute::Controller => {
            controllers::feature(ui, id, section);
            if matches!(
                id,
                FeatureId::Locomotion
                    | FeatureId::Driving
                    | FeatureId::Parachute
                    | FeatureId::Wingsuit
            ) {
                game::feature(ui, id, section);
            }
            if section == FeatureSection::Controls {
                if id == FeatureId::AnimationIk {
                    camera::body_ik(ui);
                }
                if id == FeatureId::Grapple {
                    camera::grapple_comfort(ui);
                }
                if id == FeatureId::Locomotion {
                    ui.collapsing("Advanced tuning", camera::turn_advanced);
                }
            }
        }
        FeatureRoute::HeadView if section == FeatureSection::Controls => camera::head_view(ui),
        FeatureRoute::Runtime if section == FeatureSection::Controls => vr::egui_debug_vr(ui),
        FeatureRoute::Runtime if section == FeatureSection::Status => vr::status(ui),
        FeatureRoute::Render => render::feature(ui, id, section),
        FeatureRoute::Hud if section == FeatureSection::Controls => {
            with_renderer(ui, renderer, hud::egui_debug_hud)
        }
        FeatureRoute::Hud if section == FeatureSection::Status => hud::status(ui),
        FeatureRoute::Previews if section == FeatureSection::Visualizations => {
            with_renderer(ui, renderer, previews::egui_debug_previews)
        }
        FeatureRoute::Performance if section == FeatureSection::Status => {
            performance::egui_debug_performance(ui)
        }
        FeatureRoute::RenderDiagnostics => diagnostics::feature(ui, id, section),
        FeatureRoute::Tracking if section == FeatureSection::Controls => {
            testing::tracking_feature(ui)
        }
        FeatureRoute::Replay if section == FeatureSection::Controls => testing::replay_feature(ui),
        FeatureRoute::Director if section == FeatureSection::Controls => director::feature(ui, id),
        FeatureRoute::Director
            if id == FeatureId::DirectorSafety && section == FeatureSection::Status =>
        {
            director::safety_status(ui)
        }
        FeatureRoute::Director if id == FeatureId::Video && section == FeatureSection::Status => {
            director::video_status(ui)
        }
        FeatureRoute::Environment if section == FeatureSection::Controls => environment::render(ui),
        FeatureRoute::Environment if section == FeatureSection::Status => environment::status(ui),
        FeatureRoute::Engine => game::feature(ui, id, section),
        _ => {
            ui.label(match section {
                FeatureSection::Controls => "No separate controls are available in this section.",
                FeatureSection::Status => "No separate live snapshot is available. Existing readouts remain beside this feature's controls.",
                FeatureSection::Visualizations => "No separate visualization is available for this feature. Use Visualizations above for shared views.",
            });
        }
    }
}

fn with_renderer(
    ui: &mut Ui,
    renderer: Option<&mut egui_directx11::Renderer>,
    render: impl FnOnce(&mut Ui, &mut egui_directx11::Renderer),
) {
    if let Some(renderer) = renderer {
        render(ui, renderer);
    } else {
        ui.label("Unavailable: the DX11 renderer is not attached to this view.");
    }
}
