//! The haptics: what the mixer sent each hand and the recent cues, and the settings: the switches,
//! the strength, the vocabulary's shapes, and the mixer's rules.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    haptics::{
        Category, Event, HapticsConfig, HapticsStatus, Sustain, cues::Cue, mixer::MixConfig,
        vocabulary::Primitive,
    },
};

pub(super) fn haptics_readout(ui: &mut Ui, h: &HapticsStatus) {
    let sent = Category::ALL
        .iter()
        .map(|c| format!("{} {}", c.label(), h.sent[c.index()]))
        .collect::<Vec<_>>()
        .join(" · ");
    ui.label(format!("Cues sent: {} ({sent})", h.total_sent()));
    ui.label(format!(
        "Mixer: {} played · {} stacked · {} masked · {} limited",
        h.mix.played, h.mix.stacked, h.mix.masked, h.mix.limited
    ));
    let output =
        |hand: Hand| h.last_output[hand.index()].map_or("-".to_owned(), |a| format!("{a:.2}"));
    ui.label(format!(
        "Last flush: left {} · right {}",
        output(Hand::Left),
        output(Hand::Right)
    ));
    for cue in h.recent.iter().rev() {
        ui.label(format!(
            "  {} ({}) on the {} hand: {:.2} for {:.0} ms",
            cue.event.label(),
            cue.primitive.label(),
            cue.hand.label(),
            cue.amplitude,
            cue.duration_s * 1000.0
        ));
    }
}

pub(super) fn haptics_settings(ui: &mut Ui, h: &mut HapticsConfig) {
    ui.checkbox(&mut h.enabled, "Haptics");
    ui.add(egui::Slider::new(&mut h.intensity, 0.0..=1.0).text("Strength"));
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut h.categories.contact, "Contact");
        ui.checkbox(&mut h.categories.weapons, "Weapons");
        ui.checkbox(&mut h.categories.vehicles, "Vehicles");
        ui.checkbox(&mut h.categories.traversal, "Traversal");
        ui.checkbox(&mut h.categories.interface, "Interface");
    });
    ui.checkbox(&mut h.game_rumble, "Game rumbles")
        .on_hover_text(
            "Weapon fire, damage, impacts, landings, vehicles, and the game's other rumbles.",
        );
    ui.checkbox(&mut h.gamepad_rumble, "Also rumble the gamepad")
        .on_hover_text("Let the physical gamepad rumble too, instead of staying silent.");
    ui.checkbox(&mut h.frequencies, "Per-cue frequencies")
        .on_hover_text("Send each primitive's frequency; off leaves the runtime's default.");
    ui.add(
        egui::Slider::new(&mut h.kick_amplitude_per_rad, 0.0..=20.0)
            .text("Kick amplitude (per rad)"),
    );
    ui.add(egui::Slider::new(&mut h.max_shot_s, 0.02..=2.0).text("Longest shot (s)"));
    ui.label("Vocabulary");
    for primitive in Primitive::ALL {
        let shape = h.vocabulary.shape_mut(primitive);
        ui.horizontal(|ui| {
            ui.label(primitive.label());
            ui.add(egui::Slider::new(&mut shape.soft, 0.0..=1.0).text("soft"));
            ui.add(egui::Slider::new(&mut shape.full, 0.0..=1.0).text("full"));
            ui.add(egui::Slider::new(&mut shape.duration_s, 0.002..=0.5).text("s"));
            ui.add(egui::Slider::new(&mut shape.frequency_hz, 0.0..=500.0).text("Hz"));
        });
    }
    ui.label("Mixer");
    mix_settings(ui, &mut h.mix);
    egui::CollapsingHeader::new("Cue table").show(ui, |ui| {
        for event in Event::ALL {
            cue_row(ui, event.label(), event.cue());
        }
        for sustain in Sustain::ALL {
            cue_row(ui, sustain.label(), sustain.cue());
        }
    });
}

fn cue_row(ui: &mut Ui, label: &str, cue: Cue) {
    ui.label(format!(
        "{label}: {} at {:.2} · {}",
        cue.primitive.label(),
        cue.strength,
        cue.category.label()
    ));
}

fn mix_settings(ui: &mut Ui, m: &mut MixConfig) {
    ui.add(egui::Slider::new(&mut m.sustain_knee, 0.0..=1.0).text("Sustained knee"));
    ui.add(egui::Slider::new(&mut m.sustain_ceiling, 0.0..=1.0).text("Sustained ceiling"));
    ui.add(egui::Slider::new(&mut m.knee, 0.0..=1.0).text("Cue knee"));
    ui.add(egui::Slider::new(&mut m.stack_share, 0.0..=1.0).text("Stacked share"));
    ui.add(egui::Slider::new(&mut m.duck_share, 0.0..=1.0).text("Sustained share under a cue"));
    ui.add(egui::Slider::new(&mut m.refresh_s, 0.02..=0.5).text("Sustained refresh (s)"));
}
