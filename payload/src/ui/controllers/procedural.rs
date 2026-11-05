//! The procedural torso: its status rows, its weight against the animation, the neck's error, the
//! clavicles, and the wingsuit's arm targets.

use std::time::Instant;

use egui::Ui;

use crate::{
    controllers::body::procedural::{ProceduralStatus, config::ProceduralMode, owns_body},
    status::{
        body::{PARACHUTE_BODY, ROOF_TORSO, SEATED_TORSO, SWIM_TORSO, WINGSUIT_ARMS},
        controllers::{HAND_OVER, PROCEDURAL_MODE, TWIST_BONES},
    },
    ui::status_row::headlines,
};

pub(super) fn procedural_readout(ui: &mut Ui, status: &ProceduralStatus) {
    headlines(
        ui,
        &[
            &PROCEDURAL_MODE,
            &HAND_OVER,
            &TWIST_BONES,
            &SEATED_TORSO,
            &WINGSUIT_ARMS,
            &PARACHUTE_BODY,
            &SWIM_TORSO,
            &ROOF_TORSO,
        ],
    );
    if !status.fresh(Instant::now()) {
        ui.label("Inactive: the local player's solve gate has not run recently.");
    }
    ui.label(format!(
        "Torso {:.2} · owns the torso {}",
        status.weight,
        if owns_body() { "yes" } else { "no" },
    ));
    if status.mode == ProceduralMode::Full {
        ui.label(format!("Neck {:.3} m from its target", status.neck_error_m));
        let [left, right] = status.clavicles;
        ui.label(format!(
            "Clavicles: left up {:+.1}° forward {:.1}° · right up {:+.1}° forward {:.1}°",
            left.elevation.to_degrees(),
            left.protraction.to_degrees(),
            right.elevation.to_degrees(),
            right.protraction.to_degrees(),
        ));
        ui.label(format!(
            "Wingsuit arm targets placed: L {} R {}",
            status.wing_hands[0], status.wing_hands[1],
        ));
    }
}
