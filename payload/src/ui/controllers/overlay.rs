//! The world-space overlay layer: what each source contributes and what was drawn. The overlay's
//! settings live in the global visualization drawer.

use egui::Ui;

use crate::controllers::{
    body::mirror,
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
    let attachments = mirror::attachment_status();
    ui.label(format!(
        "Last mirror attachment pass: sockets current {}; {} markers, {} offers, {} occupied, {} missing poses; pouches current {}, {} missing or hidden models.",
        attachments.sockets_current,
        attachments.socket_markers,
        attachments.socket_offers,
        attachments.occupied_sockets,
        attachments.sockets_missing,
        attachments.pouches_current,
        attachments.pouches_missing,
    ));
    ui.small("Attachment counts describe the mirror's last pass, not a live draw or engine-ownership guarantee.");
    let pouches = mirror::pouch_render_status();
    ui.label(format!(
        "Pouch render placement: {:?}; input targets: {:?}; displayed {}, targets {}, missing {}.",
        pouches.render, pouches.input, pouches.displayed, pouches.targets, pouches.missing,
    ));
    ui.small("These are separate producer-local owner/epoch/sequence identities, not engine frame numbers or a timestamp-based join.");
}
