//! The Rendering page's stereo and foveation controls, the Experimental page's single-pass stereo,
//! far field, and post-FX controls, plus render-thread capture state.
//!
//! The feature bodies use one section module per concern. The capture state lives in [`capture`],
//! independent of whether a feature is drawn.

mod capture;
mod far_field;
mod passes;
mod post_fx;
mod resolution;
mod single_pass;
mod stereo;

pub use crate::ui::render::capture::{
    EGUI_DEBUG_RENDER_STATE, EguiDebugRenderState, POST_STAGE_DOF, POST_STAGE_MB,
    capture_main_color, capture_post_stage, eye_capture_fresh, install, mark_eye_capture,
    mark_previews_visible, previews_visible,
};
use crate::{
    config,
    ui::{catalog::FeatureId, dispatch::FeatureBody},
};

/// Debug-UI only: swap the two eyes in the side-by-side stereo preview, so the pair can be fused
/// cross-eyed (left image -> right eye) instead of parallel (left image -> left eye).
pub(crate) static STEREO_CROSS_EYED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

pub(crate) fn controls(ui: &mut egui::Ui, id: FeatureId) {
    let mut cfg = config::edit();
    let cfg = &mut *cfg;
    match id {
        FeatureId::Stereo => stereo::section(ui, cfg),
        FeatureId::Foveation => resolution::section(ui, cfg),
        FeatureId::SinglePass => single_pass::section(ui, cfg),
        FeatureId::FarField => far_field::section(ui, cfg),
        FeatureId::PostFx => post_fx::section(ui, cfg),
        _ => {}
    }
}

pub(crate) fn status(id: FeatureId) -> Option<FeatureBody> {
    let body: FeatureBody = |ui, _| far_field::readout(ui);
    (id == FeatureId::FarField).then_some(body)
}
