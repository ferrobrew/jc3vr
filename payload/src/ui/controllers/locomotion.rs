//! The stick locomotion: the velocity it wrote in the movement task's place, the act filter's
//! counters, where the on-foot view's anchor comes from, and the on-foot switches and run speed.

use std::time::Instant;

use egui::Ui;

use crate::{
    controllers::locomotion::{
        LocomotionStatus,
        acts::ActFilterStatus,
        anchor::{AnchorStatus, remeasure},
        config::{ActFilterConfig, SpeedSource, StableViewConfig, StickConfig},
    },
    ui::controllers::{vec3, yes_no},
};

pub(super) fn locomotion_readout(
    ui: &mut Ui,
    stick: &LocomotionStatus,
    acts: &ActFilterStatus,
    anchor: &AnchorStatus,
) {
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
        "Task velocity {:.2} m/s · target {:.2} m/s · written {}",
        stick.task_velocity.length(),
        stick.target.length(),
        stick.velocity.map_or_else(
            || "nothing (off)".to_owned(),
            |v| format!("{:.2} m/s", v.length())
        ),
    ));
    ui.separator();
    ui.label(format!(
        "Act filter: {} dropped · {} straightened · {} refused by the state machine · {} watchdog releases{}",
        acts.dropped,
        acts.replaced,
        acts.refused,
        acts.releases,
        if acts.released { " (released now)" } else { "" },
    ));
    ui.separator();
    ui.label(format!(
        "View anchor: root weight {:.2} · eligible {} · height smoothing {:+.3} m",
        anchor.weight,
        yes_no(anchor.eligible),
        anchor.height_offset,
    ));
    match anchor.neutral {
        Some(neutral) => {
            ui.label(format!(
                "Neutral head {} · neck {} · eyes {}",
                vec3(neutral.head),
                vec3(neutral.neck),
                vec3(neutral.eyes),
            ));
        }
        None => {
            ui.label(format!(
                "Neutral head not measured yet: {:.1} s of standing still sampled",
                anchor.capture_progress,
            ));
        }
    }
    if ui.button("Measure the neutral head again").clicked() {
        remeasure();
    }
}

pub(super) fn locomotion_settings(
    ui: &mut Ui,
    stick: &mut StickConfig,
    acts: &mut ActFilterConfig,
    view: &mut StableViewConfig,
) {
    ui.checkbox(&mut view.enabled, "Steady view on foot")
        .on_hover_text(
            "Anchor the view to Rico's root and his neutral standing head on foot, so no animation \
             bob, lean, or lurch reaches it.",
        );
    ui.checkbox(&mut acts.enabled, "No turning animations")
        .on_hover_text(
            "Drop the turn-in-place acts and swap the turning starts and stops for their straight \
             variants.",
        );
    ui.horizontal(|ui| {
        ui.label("Run speed from");
        ui.selectable_value(&mut stick.speed, SpeedSource::Game, "The game")
            .on_hover_text("The game's walk, run, and backpedal speeds for the stick.");
        ui.selectable_value(&mut stick.speed, SpeedSource::Fixed, "Fixed");
    });
    ui.add_enabled(
        stick.speed == SpeedSource::Fixed,
        egui::Slider::new(&mut stick.run_speed_mps, 0.5..=15.0).text("Full-stick speed (m/s)"),
    );
}
