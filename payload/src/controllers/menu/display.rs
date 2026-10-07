//! The `VR: DISPLAY` page: the headset's render resolution, the anti-aliasing and sharpening, the
//! foveated rendering, and where the floating panel sits and how the reticle lands on it. The
//! rendering internals (the FSR jitter and motion vectors, the foveation's radii, the mirror) and
//! the panel's follow and depth tuning stay in the debug UI.
//!
//! Every setting applies on the next frame, except the size of the headset's own image: it keeps
//! the size it was created at until the game restarts, so a resolution change resizes the scene at
//! once and the image the headset is sent after a restart.

use crate::{
    controllers::menu::{config_choice, config_slider, config_toggle},
    hud::ReticleAlign,
    pause_menu::model::{
        page::{Page, PageDef},
        slider::{SliderFormat, SliderRange},
    },
};

/// The `VR: DISPLAY` page.
pub(crate) const PAGE: PageDef = PageDef {
    tab: "DISPLAY",
    build,
    on_close: None,
    on_frame: None,
};

fn build() -> Page {
    Page::new("VR: DISPLAY")
        .header("RENDERING")
        .item(config_slider!(
            "RESOLUTION",
            RESOLUTION_RANGE,
            SliderFormat::new(2, "X"),
            |c| c.vr.resolution_scale
        ))
        .text("THE SCENE RESIZES AT ONCE, THE")
        .text("HEADSET IMAGE AFTER A RESTART.")
        .item(config_toggle!("FSR ANTI-ALIASING", |c| c.fsr.enabled))
        .item(config_toggle!("FSR SHARPENING", |c| c.fsr.sharpen))
        .item(config_slider!(
            "SHARPNESS",
            UNIT_RANGE,
            SliderFormat::new(2, ""),
            |c| c.fsr.sharpness
        ))
        .item(config_toggle!("FOVEATED RENDERING", |c| c
            .foveation
            .enabled))
        .header("FLOATING PANEL")
        .item(config_slider!(
            "PANEL DISTANCE",
            PANEL_DISTANCE_RANGE,
            SliderFormat::new(1, " M"),
            |c| c.hud.distance
        ))
        .item(config_slider!(
            "PANEL SIZE",
            PANEL_SCALE_RANGE,
            SliderFormat::new(2, "X"),
            |c| c.hud.panel_scale
        ))
        .item(config_choice!(
            "RETICLE PLACEMENT",
            &RETICLE_ALIGNS,
            &RETICLE_ALIGN_LABELS,
            |c| c.hud.reticle_align
        ))
}

const RETICLE_ALIGNS: [ReticleAlign; 2] =
    [ReticleAlign::PanelSubtense, ReticleAlign::GameProjection];
const RETICLE_ALIGN_LABELS: [&str; 2] = ["WHERE YOU AIM", "GAME'S PROJECTION"];

// Slider notch counts are multiples of ten: the movie's keyboard step is a tenth of the notches,
// truncated to an integer.

/// The per-eye resolution scale on the runtime's recommendation, in twentieths: from a quarter of
/// the pixels to four times as many.
const RESOLUTION_RANGE: SliderRange = SliderRange::new(0.5, 2.0, 30);
/// A strength, `0..=1` in hundredths.
const UNIT_RANGE: SliderRange = SliderRange::new(0.0, 1.0, 100);
/// The panel's distance from the eyes, metres, in tenths. The panel keeps its apparent size at any
/// distance, so this moves only its depth.
const PANEL_DISTANCE_RANGE: SliderRange = SliderRange::new(1.0, 10.0, 90);
/// The panel's apparent size, in fiftieths.
const PANEL_SCALE_RANGE: SliderRange = SliderRange::new(0.4, 2.0, 80);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::menu::tests::exercise_page;

    #[test]
    fn the_page_edits_valid_settings_and_resets_to_the_defaults() {
        exercise_page(&build());
    }

    #[test]
    fn the_choice_lists_line_up() {
        assert_eq!(RETICLE_ALIGNS.len(), RETICLE_ALIGN_LABELS.len());
    }
}
