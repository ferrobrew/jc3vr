//! The world-space overlay layer: what each source contributes, what was drawn, and the overlay's
//! settings.

use egui::Ui;

use crate::controllers::{
    config::{GunReadout, OverlayConfig},
    overlay::{OverlayStatus, primitive::Source},
};

pub(super) fn overlay_readout(ui: &mut Ui, status: &OverlayStatus) {
    ui.label(format!(
        "Primitives drawn per eye: {} · frames drawn {}",
        status.drawn, status.frames
    ));
    ui.label(
        Source::ALL
            .iter()
            .map(|s| format!("{} {}", s.label(), status.counts[s.index()]))
            .collect::<Vec<_>>()
            .join(" · "),
    );
    if let Some(error) = &status.error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
}

pub(super) fn overlay_settings(ui: &mut Ui, o: &mut OverlayConfig) {
    ui.checkbox(
        &mut o.hide_game_reticle,
        "Hide the game's reticles while the hands aim",
    )
    .on_hover_text(
        "Hide the HUD's weapon reticle while the guns aim by hand, and its grapple reticle \
             always: both mark where the gaze aims, and the overlay marks the grapple's target.",
    );
    ui.checkbox(&mut o.enabled, "Enabled")
        .on_hover_text("Draw the world-space overlay layer.");
    ui.add_enabled_ui(o.enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Gun readout");
            ui.selectable_value(&mut o.gun_readout, GunReadout::None, "None");
            ui.selectable_value(&mut o.gun_readout, GunReadout::Dot, "Dot");
            ui.selectable_value(&mut o.gun_readout, GunReadout::Laser, "Laser");
        });
        ui.checkbox(&mut o.grapple_highlight, "Grapple target highlight");
        ui.checkbox(&mut o.grapple_beam, "Grapple beam");
        ui.checkbox(&mut o.ghost_hands, "Ghost hands")
            .on_hover_text("Show the controller's hand while the animation owns it.");
        ui.checkbox(&mut o.grab_volumes, "Grab-volume highlights")
            .on_hover_text("Ring the holster, foregrip, or pouch each hand is in.");
        ui.checkbox(&mut o.debug_gizmos, "Controller gizmos");
        ui.add(egui::Slider::new(&mut o.depth_tolerance_m, 0.0..=0.5).text("Depth tolerance (m)"));
        ui.add(
            egui::Slider::new(&mut o.depth_tolerance_rel, 0.0..=0.05)
                .text("Depth tolerance (fraction of distance)"),
        );
        ui.add(egui::Slider::new(&mut o.min_line_px, 0.5..=4.0).text("Minimum line width (px)"));
        ui.add(egui::Slider::new(&mut o.xray_alpha, 0.0..=1.0).text("Ghost x-ray alpha"));
    });
}
