//! The procedural torso: its status rows, its weight against the animation, the neck's error, the
//! clavicles, and the wingsuit's arm targets; and the IK body's state, weights, and feet.

use std::time::Instant;

use egui::Ui;

use crate::{
    controllers::body::{
        ik_body,
        procedural::{ProceduralStatus, config::ProceduralMode, owns_body},
    },
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
        ui.label(format!(
            "Square stance {:.2} · foot targets moved {}",
            status.stance_weight, status.stance_targets,
        ));
    }
}

/// The IK body's readout: its state, why it stands down, its weights, and the feet.
pub(super) fn ik_body_readout(ui: &mut Ui) {
    let ik = ik_body::status();
    let w = ik.weights;
    ui.label(format!(
        "IK body: {} · {} · pelvis from the {}",
        ik.state.label(),
        ik.stand_down
            .map_or("owns its channels", ik_body::StandDown::label),
        ik.source.label(),
    ));
    ui.label(format!(
        "IK weight: pelvis {:.2} turn {:.2} spine {:.2} feet {:.2} air {:.2} · {} targets",
        w.pelvis_position, w.pelvis_orientation, w.spine, w.feet, w.air_legs, ik.targets_queued,
    ));
    ui.label(format!(
        "Feet: left {} · right {} · {} planted · {} steps ({:.1}/min) · {} hand-overs · {} missed probes",
        ik.feet[0].label(),
        ik.feet[1].label(),
        ik.planted,
        ik.steps,
        ik.steps_per_minute,
        ik.handovers,
        ik.missed,
    ));
    ui.label(format!(
        "Last step: {} · hips moved {:.3} m · lowered {:.3} m (at most {:.3} m this spell)",
        ik.last_step.map_or_else(
            || "none".to_owned(),
            |(side, reason)| format!("{} foot, {}", side.label(), reason.label())
        ),
        ik.hips_moved_m,
        ik.pelvis_drop_m,
        ik.max_drop_m,
    ));
    ui.label(format!(
        "Feet stand on {} · {} times on a moving body · {} moving supports tracked",
        if ik.on_moving_support {
            "a moving body"
        } else {
            "the world"
        },
        ik.moving_landings,
        ik.moving_supports,
    ));
    let brace = |braced: bool| if braced { "braced" } else { "hanging" };
    ui.label(format!(
        "Climbing feet: left {} · right {} · {} braces taken",
        brace(ik.braced[0]),
        brace(ik.braced[1]),
        ik.brace_moves,
    ));
}
