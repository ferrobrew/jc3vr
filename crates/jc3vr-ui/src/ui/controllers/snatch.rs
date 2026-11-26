//! The grapple snatch's switches, the held line's tuning, and the yank that fires it.

use egui::{Slider, Ui};

use crate::controllers::snatch::{SnatchConfig, SwingConfig};

pub(super) fn snatch_settings(ui: &mut Ui, s: &mut SnatchConfig) {
    ui.checkbox(&mut s.enabled, "Grapple snatch").on_hover_text(
        "Grip with the empty grapple hand at a weapon out of reach under the grapple's aim, \
             then yank the hand back: the hook pulls the weapon into the hand. The grapple's aim \
             also sees weapons on the ground: a tap tosses one to your feet, a hold tethers it.",
    );
    ui.add_enabled_ui(s.enabled, |ui| {
        ui.indent("snatch", |ui| {
            ui.checkbox(&mut s.sling, "Swing on the line").on_hover_text(
                "Objects too heavy to carry, up to the ceiling, are swung on the line instead of \
                 pulled in: yank, then drag, whirl, or lift the hand, and open the grip to let go. \
                 Grip the grapple hand's wrist with the other hand to brace it. Holding B after \
                 gripping a weapon or light object swings it too; release B to bring it to the hand.",
            );
            ui.add_enabled_ui(s.sling, |ui| {
                ui.indent("swing", |ui| swing_settings(ui, &mut s.swing));
            });
            ui.add(
                Slider::new(&mut s.yank.min_speed_mps, 0.5..=5.0).text("Yank speed (m/s)"),
            );
            ui.add(
                Slider::new(&mut s.yank.min_distance_m, 0.05..=0.4).text("Yank length (m)"),
            );
        });
    });
}

fn swing_settings(ui: &mut Ui, s: &mut SwingConfig) {
    ui.add(
        Slider::new(&mut s.ceiling_kg, 40.0..=2000.0)
            .logarithmic(true)
            .text("Heaviest swung (kg)"),
    )
    .on_hover_text(
        "The heaviest loose object offered. Lighter than one hand carries is pulled in instead.",
    );
    ui.add(
        Slider::new(&mut s.strength_n, 200.0..=6000.0)
            .logarithmic(true)
            .text("Line strength at 10 kg (N)"),
    );
    ui.add(Slider::new(&mut s.mass_scaling, 0.0..=1.0).text("Strength's growth with mass"))
        .on_hover_text("0: the line pulls every mass equally hard. 1: every mass swings alike.");
    ui.add(Slider::new(&mut s.brace_factor, 1.0..=3.0).text("Braced strength (x)"));
    ui.add(Slider::new(&mut s.stiffness_per_s2, 50.0..=1200.0).text("Stiffness (1/s²)"));
    ui.add(Slider::new(&mut s.damping_per_s, 0.0..=60.0).text("Damping (1/s)"));
    ui.add(Slider::new(&mut s.swing_length_m, 1.0..=10.0).text("Swing length (m)"));
    ui.add(Slider::new(&mut s.reel_mps, 0.0..=12.0).text("Reel speed (m/s)"))
        .on_hover_text("How fast the line draws a far target in to the swing length. 0 keeps the length the hook bit at.");
}
