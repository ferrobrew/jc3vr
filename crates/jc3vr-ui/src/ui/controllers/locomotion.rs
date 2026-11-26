//! The stick locomotion: its status rows, the stick and the movement task's velocities, the on-foot
//! view anchor's height smoothing and neutral head, and the arm strokes in water.

use std::time::Instant;

use egui::Ui;
use jc3vr_body::controllers::locomotion::{
    LocomotionStatus,
    anchor::{AnchorStatus, remeasure},
    strokes::StrokeStatus,
};

use crate::{
    status::controllers::{ACT_FILTER, ACT_WATCHDOG, NEUTRAL_HEAD, STEADY_VIEW, STICK_VELOCITY},
    ui::{controllers::vec3, status_row::headlines},
};

pub(super) fn locomotion_readout(ui: &mut Ui, stick: &LocomotionStatus, anchor: &AnchorStatus) {
    headlines(
        ui,
        &[
            &STICK_VELOCITY,
            &ACT_FILTER,
            &ACT_WATCHDOG,
            &STEADY_VIEW,
            &NEUTRAL_HEAD,
        ],
    );
    let now = Instant::now();
    if !stick.fresh(now) {
        ui.label("Inactive: the on-foot movement task has not run recently.");
    }
    ui.label(format!(
        "Stick ({:+.2}, {:+.2}) · game target speed {} · root speed {:.2} m/s",
        stick.stick.x,
        stick.stick.y,
        stick
            .game_speed
            .map_or_else(|| "none".to_owned(), |s| format!("{s:.2} m/s")),
        stick.root_speed,
    ));
    ui.label(format!(
        "Task velocity {:.2} m/s · target {:.2} m/s",
        stick.task_velocity.length(),
        stick.target.length(),
    ));
    ui.separator();
    ui.label(format!(
        "View anchor height smoothing {:+.3} m",
        anchor.height_offset
    ));
    if let Some(neutral) = anchor.neutral {
        ui.label(format!(
            "Neutral head {} · neck {} · eyes {}",
            vec3(neutral.head),
            vec3(neutral.neck),
            vec3(neutral.eyes),
        ));
    }
    if ui.button("Measure the neutral head again").clicked() {
        remeasure();
    }
}

pub(super) fn strokes_readout(ui: &mut Ui, strokes: &StrokeStatus) {
    if !strokes.fresh(Instant::now()) {
        ui.label(format!(
            "Arm strokes: not swimming · strokes caught left {} · right {}",
            strokes.strokes[0], strokes.strokes[1],
        ));
        return;
    }
    let s = &strokes.step;
    let hand = |i: usize| {
        format!(
            "{} push ({:+.2}, {:+.2})",
            if s.in_water[i] { "in water" } else { "dry" },
            s.push[i].x,
            s.push[i].y,
        )
    };
    ui.label(format!(
        "Arm strokes: left {} · right {} · caught left {} · right {}",
        hand(0),
        hand(1),
        strokes.strokes[0],
        strokes.strokes[1],
    ));
    ui.label(format!(
        "Stroke speed {:.2} m/s · swim core {:.2} m/s, written {:.2} m/s · turning push {:+.3} m²/s · turn {:+.0}°/tick",
        s.speed,
        strokes.core_mps,
        strokes.written_mps,
        s.torque,
        s.turn_right.to_degrees(),
    ));
}
