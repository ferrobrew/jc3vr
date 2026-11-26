//! Stunt positions held by the grip: the last entry, and what each hand holds.

use egui::Ui;
use jc3vr_input::frame::Hand;
use jc3vr_physical::controllers::physical_hands::stunt::StuntStatus;

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
