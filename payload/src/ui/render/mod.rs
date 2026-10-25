//! Stereo, resolution, and graphics experiment controls, plus render-thread capture state.
//!
//! The feature bodies use one section module per concern. The capture state lives in [`capture`],
//! independent of whether a feature is drawn.

mod capture;
mod corrections;
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
    ui::catalog::{FeatureId, FeatureSection},
};

/// Debug-UI only: swap the two eyes in the side-by-side stereo preview, so the pair can be fused
/// cross-eyed (left image -> right eye) instead of parallel (left image -> left eye). Read by the
/// F10 capture composite so the recording window fuses the same way as the preview.
pub(crate) static STEREO_CROSS_EYED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

pub(crate) fn feature(ui: &mut egui::Ui, id: FeatureId, section: FeatureSection) {
    match section {
        FeatureSection::Controls => {
            let mut cfg = config::edit();
            let cfg = &mut *cfg;
            match id {
                FeatureId::Stereo => {
                    stereo::section(ui, cfg);
                    corrections::section(ui, cfg);
                }
                FeatureId::Resolution => resolution::section(ui, cfg),
                FeatureId::Graphics => {
                    far_field::section(ui, cfg);
                    single_pass::section(ui, cfg);
                    post_fx::section(ui, cfg);
                }
                _ => {}
            }
        }
        FeatureSection::Status if id == FeatureId::Graphics => far_field::readout(ui),
        FeatureSection::Status => {
            ui.label("No separate live snapshot is available. Existing readouts remain beside this feature's controls.");
        }
        FeatureSection::Visualizations => {
            ui.label("No separate visualization is available for this feature. Use Visualizations above for shared views.");
        }
    }
}
