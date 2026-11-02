//! The procedural torso: its mode, its weight against the animation, and what its last gate did.

use std::time::{Duration, Instant};

use egui::Ui;

use crate::controllers::body::procedural::{ProceduralStatus, config::ProceduralMode, owns_body};

pub(super) fn procedural_readout(ui: &mut Ui, status: &ProceduralStatus) {
    let fresh = status
        .at
        .is_some_and(|at| Instant::now().saturating_duration_since(at) < STALE);
    if !fresh {
        ui.label("Inactive: the local player's solve gate has not run recently.");
    }
    ui.label(format!(
        "Mode {} · torso {:.2} · owns the torso {}",
        match status.mode {
            ProceduralMode::Off => "off",
            ProceduralMode::Full => "on",
        },
        status.weight,
        if owns_body() { "yes" } else { "no" },
    ));
    if let Some(error) = status.rig_error {
        ui.label(format!("Not rigged: {error}"));
    }
    if status.mode == ProceduralMode::Full {
        ui.label(format!(
            "Hand-over case: {} · neck {:.3} m from its target",
            status.case.label(),
            status.neck_error_m,
        ));
        let [left, right] = status.clavicles;
        ui.label(format!(
            "Clavicles: left up {:+.1}° forward {:.1}° · right up {:+.1}° forward {:.1}°",
            left.elevation.to_degrees(),
            left.protraction.to_degrees(),
            right.elevation.to_degrees(),
            right.protraction.to_degrees(),
        ));
        ui.label(format!(
            "Wingsuit arms {:.2} · targets placed L {} R {} · queued L {} R {}",
            status.wing_weight,
            status.wing_hands[0],
            status.wing_hands[1],
            status.wing_queued[0],
            status.wing_queued[1],
        ));
    }
}

const STALE: Duration = Duration::from_millis(500);
