//! Stunt positions held by the grip: the last entry, what each hand holds, and the settings.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    physical_hands::stunt::{StuntConfig, StuntStatus},
};

pub(super) fn stunt_readout(ui: &mut Ui, status: &StuntStatus) {
    if let Some(entry) = status.last_entry {
        ui.label(format!("Last entry onto {}", entry.label()));
    }
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let i = hand.index();
            ui.strong(hand.label());
            ui.label(if status.holding[i] {
                "holds the hang"
            } else if status.on_helicopter[i] {
                "grips a helicopter"
            } else {
                "holds no stunt handhold"
            });
        }
    });
}

pub(super) fn stunt_settings(ui: &mut Ui, c: &mut StuntConfig) {
    ui.checkbox(
        &mut c.enabled,
        "Hold stunt positions with the grip (needs the hand climbing)",
    )
    .on_hover_text(
        "Physical grips stay at the handhold, a mantle onto a roof enters its stunt area, and \
         opening every grip lets go of a hang; the normal grapple can still enter a hang.",
    );
}
