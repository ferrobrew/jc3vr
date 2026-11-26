//! The Rendering page's stereo and foveation controls, the Experimental page's single-pass stereo,
//! far field, and post-FX controls, plus render-thread capture state.
//!
//! The feature bodies use one section module per concern. The capture state lives in
//! [`jc3vr_view::hooks::graphics_engine::capture`], beside the render hooks that fill it.

mod far_field;
mod passes;
mod post_fx;
mod resolution;
mod single_pass;
mod stereo;

use jc3vr_core::config::engine::{Parts, edit_parts};
use jc3vr_render::hooks::graphics_engine::config::POST_FX_CONFIG;
use jc3vr_view::vr::FOVEATION_CONFIG;

use crate::ui::{catalog::FeatureId, dispatch::FeatureBody};

/// Debug-UI only: swap the two eyes in the side-by-side stereo preview, so the pair can be fused
/// cross-eyed (left image -> right eye) instead of parallel (left image -> left eye).
pub(crate) static STEREO_CROSS_EYED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

pub(crate) fn controls(ui: &mut egui::Ui, id: FeatureId) {
    let mut cfg = edit_parts();
    let cfg = &mut cfg;
    match id {
        FeatureId::Stereo => stereo::section(ui, cfg),
        FeatureId::Foveation => resolution::section(ui, cfg.part_mut(&FOVEATION_CONFIG)),
        FeatureId::SinglePass => single_pass::section(ui, cfg),
        FeatureId::FarField => far_field::section(ui, cfg),
        FeatureId::PostFx => post_fx::section(ui, cfg.part_mut(&POST_FX_CONFIG)),
        _ => {}
    }
}

pub(crate) fn status(id: FeatureId) -> Option<FeatureBody> {
    let body: FeatureBody = |ui, _| far_field::readout(ui);
    (id == FeatureId::FarField).then_some(body)
}
