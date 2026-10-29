//! Production feature routing: each feature renders its controls followed by a collapsible live
//! readout.

#[cfg(test)]
#[path = "dispatch/tests.rs"]
mod tests;

use egui::Ui;

use crate::ui::{
    camera, catalog::FeatureId, controllers, diagnostics, director, environment, game, hud,
    navigation::NavigationState, overview, performance, previews, render, render_capture, testing,
    vr,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FeatureRoute {
    Status,
    Controller,
    HeadView,
    Runtime,
    Render,
    Hud,
    Previews,
    RenderCapture,
    RenderRe,
    Tracking,
    Replay,
    Director,
    Environment,
}

/// A feature body that needs neither the renderer nor the navigation state. Controller, game,
/// render, and diagnostics modules hand these out per feature so dispatch can tell whether a
/// feature has controls or a readout without rendering it.
pub(crate) type FeatureBody = fn(&mut Ui, FeatureId);

pub(crate) fn route(id: FeatureId) -> FeatureRoute {
    match id {
        FeatureId::Status => FeatureRoute::Status,
        FeatureId::HeadView => FeatureRoute::HeadView,
        FeatureId::Runtime => FeatureRoute::Runtime,
        FeatureId::Stereo
        | FeatureId::Foveation
        | FeatureId::SinglePass
        | FeatureId::FarField
        | FeatureId::PostFx => FeatureRoute::Render,
        FeatureId::Hud => FeatureRoute::Hud,
        FeatureId::Previews => FeatureRoute::Previews,
        FeatureId::RenderCapture => FeatureRoute::RenderCapture,
        FeatureId::RenderRe => FeatureRoute::RenderRe,
        FeatureId::TrackingCapture => FeatureRoute::Tracking,
        FeatureId::Replay => FeatureRoute::Replay,
        FeatureId::Director | FeatureId::Video => FeatureRoute::Director,
        FeatureId::Environment => FeatureRoute::Environment,
        FeatureId::Posture
        | FeatureId::BodyFit
        | FeatureId::AnimationIk
        | FeatureId::Presentation
        | FeatureId::GripFingers
        | FeatureId::PhysicalHands
        | FeatureId::CharactersDoors
        | FeatureId::Snatch
        | FeatureId::Feedback
        | FeatureId::Hold
        | FeatureId::Support
        | FeatureId::Aim
        | FeatureId::Reload
        | FeatureId::Inventory
        | FeatureId::Explosives
        | FeatureId::WeaponSelection
        | FeatureId::OnFoot
        | FeatureId::Grapple
        | FeatureId::Climbing
        | FeatureId::Vehicles
        | FeatureId::Air
        | FeatureId::Devices
        | FeatureId::Wrist
        | FeatureId::Semantic
        | FeatureId::Presets => FeatureRoute::Controller,
    }
}

pub(crate) fn feature(
    ui: &mut Ui,
    selection: &mut NavigationState,
    renderer: Option<&mut egui_directx11::Renderer>,
) {
    let id = selection.feature;
    controls(ui, selection, renderer);
    // Readouts run after the controls return, so no configuration edit guard is held.
    let readouts: Vec<FeatureBody> = readouts(id).collect();
    if !readouts.is_empty() {
        egui::CollapsingHeader::new("Live status")
            .default_open(true)
            .show(ui, |ui| {
                for readout in readouts {
                    readout(ui, id);
                }
            });
    }
}

fn controls(
    ui: &mut Ui,
    selection: &mut NavigationState,
    renderer: Option<&mut egui_directx11::Renderer>,
) {
    let id = selection.feature;
    match route(id) {
        FeatureRoute::Status => overview::render(ui, selection),
        FeatureRoute::Controller => {
            if let Some(body) = controllers::controls(id) {
                body(ui, id);
            }
            game::controls(ui, id);
        }
        FeatureRoute::HeadView => camera::head_view(ui),
        FeatureRoute::Runtime => vr::egui_debug_vr(ui),
        FeatureRoute::Render => render::controls(ui, id),
        FeatureRoute::Hud => with_renderer(ui, renderer, hud::egui_debug_hud),
        FeatureRoute::Previews => with_renderer(ui, renderer, previews::egui_debug_previews),
        FeatureRoute::RenderCapture => render_capture::controls(ui),
        FeatureRoute::RenderRe => diagnostics::open_button(ui),
        FeatureRoute::Tracking => testing::tracking_feature(ui),
        FeatureRoute::Replay => testing::replay_feature(ui),
        FeatureRoute::Director => director::feature(ui, id),
        FeatureRoute::Environment => environment::render(ui),
    }
}

fn readouts(id: FeatureId) -> impl Iterator<Item = FeatureBody> {
    let routed: Option<FeatureBody> = match route(id) {
        FeatureRoute::Status | FeatureRoute::RenderRe => None,
        FeatureRoute::HeadView => Some(|ui, _| camera::head_view_status(ui)),
        FeatureRoute::Controller => controllers::status(id),
        FeatureRoute::Runtime => Some(|ui, _| vr::status(ui)),
        FeatureRoute::Render => render::status(id),
        FeatureRoute::Hud => Some(|ui, _| hud::status(ui)),
        FeatureRoute::Previews => Some(|ui, _| performance::egui_debug_performance(ui)),
        FeatureRoute::RenderCapture => Some(|ui, _| render_capture::status(ui)),
        FeatureRoute::Tracking => Some(|ui, _| testing::tracking_status(ui)),
        FeatureRoute::Replay => Some(|ui, _| testing::replay_status(ui)),
        FeatureRoute::Director => director_status(id),
        FeatureRoute::Environment => Some(|ui, _| environment::status(ui)),
    };
    routed.into_iter().chain(game::status(id))
}

fn director_status(id: FeatureId) -> Option<FeatureBody> {
    let body: FeatureBody = match id {
        FeatureId::Director => |ui, _| director::director_status(ui),
        FeatureId::Video => |ui, _| director::video_status(ui),
        _ => return None,
    };
    Some(body)
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
