//! The procedural body: its mode, its weight against the animation, and what its last gate did.

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
        "Mode {} · weight {:.2} · owns the body {}",
        match status.mode {
            ProceduralMode::Off => "off",
            ProceduralMode::PassThrough => "pass-through",
            ProceduralMode::Body => "standing body",
        },
        status.weight,
        if owns_body() { "yes" } else { "no" },
    ));
    if let Some(error) = status.rig_error {
        ui.label(format!("Not rigged: {error}"));
    }
    if let Some((metres, degrees)) = status.round_trip {
        ui.label(format!(
            "Pass-through read-back error: {:.5} m · {:.3}°",
            metres, degrees
        ));
    }
    if status.mode == ProceduralMode::Body {
        ui.label(format!(
            "Hips {:.3} m below the animation · neck {:.3} m from its target",
            status.hips_drop_m, status.neck_error_m
        ));
    }
}

const STALE: Duration = Duration::from_millis(500);
