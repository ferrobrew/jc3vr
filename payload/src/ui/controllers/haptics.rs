//! The haptics: what the mixer sent each hand and the recent cues, and the gamepad rumble switch.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    haptics::{Category, HapticsConfig, HapticsStatus},
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
    ui.checkbox(&mut h.gamepad_rumble, "Also rumble the gamepad")
        .on_hover_text("Let the physical gamepad rumble too, instead of staying silent.");
}
