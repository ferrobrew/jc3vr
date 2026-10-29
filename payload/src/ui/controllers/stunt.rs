//! Stunt positions held by the grip: the position the game holds the player in, the entries and
//! let-gos, and the settings.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    physical_hands::stunt::{StuntConfig, StuntStatus},
};

pub(super) fn stunt_readout(ui: &mut Ui, status: &StuntStatus) {
    ui.label(format!(
        "{} · {}",
        status
            .position
            .map_or("not in a stunt position", |kind| kind.label()),
        if status.armed {
            "a grip held in this hang, so opening every grip lets go"
        } else {
            "no grip has held in this hang yet"
        }
    ));
    ui.label(format!(
        "{} hangs and {} roofs entered · {} let go · {} refused{}",
        status.hang_entries,
        status.roof_entries,
        status.releases,
        status.refusals,
        status.last_entry.map_or_else(String::new, |entry| format!(
            " · last entry onto {}",
            entry.label()
        ))
    ));
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
    if let Some(error) = &status.last_error {
        ui.label(format!("Last error: {error}"));
    }
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
