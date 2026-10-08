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
        "Mode {} · torso {:.2} · legs {:.2} · owns the body {}",
        match status.mode {
            ProceduralMode::Off => "off",
            ProceduralMode::PassThrough => "pass-through",
            ProceduralMode::Full => "full body",
        },
        status.weight,
        status.legs_weight,
        if owns_body() { "yes" } else { "no" },
    ));
    if status.mode == ProceduralMode::Full {
        ui.label(format!("Hand-over case: {}", status.case.label()));
    }
    if let Some(error) = status.rig_error {
        ui.label(format!("Not rigged: {error}"));
    }
    if let Some((metres, degrees)) = status.round_trip {
        ui.label(format!(
            "Pass-through read-back error: {:.5} m · {:.3}°",
            metres, degrees
        ));
    }
    if status.mode == ProceduralMode::Full {
        ui.label(format!(
            "Hips {:.3} m below the animation · neck {:.3} m from its target · {}",
            status.hips_drop_m,
            status.neck_error_m,
            if status.stepping {
                "stepping"
            } else {
                "feet planted"
            },
        ));
        ui.label(format!(
            "Speed {:.2} m/s · {} · reach drop {:.3} m · stance {}",
            status.speed_mps,
            status.gait_phase.map_or_else(
                || "standing".to_owned(),
                |p| format!("walking, phase {p:.2}")
            ),
            status.reach_drop_m,
            if status.stance_measured {
                "measured"
            } else {
                "not measured yet"
            },
        ));
        let ground = &status.ground;
        ui.label(format!(
            "Ground casts this tick {} · hits {} · too steep {} · on objects {} · errors {} · largest offset {:+.3} m",
            ground.casts,
            ground.hits,
            ground.steep,
            ground.objects,
            ground.errors,
            ground.largest_offset_m,
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
            "Legs in the air {:.2} · vertical speed {:+.2} m/s · landing dip {:.3} m",
            status.air_weight, status.vertical_mps, status.landing_dip_m,
        ));
    }
}

const STALE: Duration = Duration::from_millis(500);
