//! Finger curls: the grip pose rows, the per-hand readout, and the finger settings.

use egui::Ui;

use crate::{
    controllers::{
        fingers::{
            HandFingersStatus,
            config::{FingerSource, FingersConfig},
            curl::Finger,
        },
        frame::Hand,
    },
    status::fingers::{LEFT_GRIP, RIGHT_GRIP},
    ui::{controllers::yes_no, status_row::headlines},
};

pub(super) fn fingers_readout(ui: &mut Ui, status: &[HandFingersStatus; 2], conforming: bool) {
    headlines(ui, &[&LEFT_GRIP, &RIGHT_GRIP]);
    ui.label(format!(
        "Joints located with the controller-conforming motion range: {}",
        yes_no(conforming)
    ));
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let s = &status[hand.index()];
            ui.strong(hand.label());
            let Some(source) = s.source else {
                continue;
            };
            ui.label(format!(
                "curls from {} · weight {:.2}",
                source.label(),
                s.weight
            ));
            if !s.unresolved && !s.applied {
                ui.label("not written");
            }
            for finger in Finger::ALL {
                let i = finger.index();
                ui.label(format!(
                    "{:<6} curl {:.2} · splay {:+.0}° · hook {:+.2}{}",
                    finger.label(),
                    s.shape.curl[i],
                    s.shape.splay[i].to_degrees(),
                    s.shape.hook[i],
                    if s.driven[i] { "" } else { " · animated" }
                ));
            }
        }
    });
}

pub(super) fn fingers_settings(ui: &mut Ui, f: &mut FingersConfig) {
    ui.checkbox(&mut f.enabled, "Finger curls")
        .on_hover_text("Curl the fingers, and wrap them round what the hand holds.");
    ui.add_enabled_ui(f.enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Source");
            for source in FingerSource::ALL {
                ui.selectable_value(&mut f.source, source, source.label());
            }
        })
        .response
        .on_hover_text(
            "Auto uses hand tracking for a hand the runtime tracks and the estimate from the \
             inputs otherwise.",
        );
    });
}
