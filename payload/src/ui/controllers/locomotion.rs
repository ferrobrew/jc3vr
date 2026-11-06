//! The stick locomotion: its status rows, the stick and the movement task's velocities, the on-foot
//! view anchor's height smoothing and neutral head, the arm strokes in water, and the on-foot
//! switches, run speed, and stroke tuning.

use std::time::Instant;

use egui::Ui;

use crate::{
    controllers::locomotion::{
        LocomotionStatus,
        anchor::{AnchorStatus, remeasure},
        config::{ActFilterConfig, SpeedSource, StableViewConfig, StickConfig, SwimStrokeConfig},
        strokes::StrokeStatus,
    },
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

pub(super) fn locomotion_settings(
    ui: &mut Ui,
    stick: &mut StickConfig,
    acts: &mut ActFilterConfig,
    view: &mut StableViewConfig,
    strokes: &mut SwimStrokeConfig,
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
    strokes_settings(ui, strokes);
}

fn strokes_settings(ui: &mut Ui, s: &mut SwimStrokeConfig) {
    ui.checkbox(&mut s.enabled, "Swim with the arms")
        .on_hover_text(
            "A hand sweeping back through the water, palm first, swims forward, and a stroke on one \
             side turns away from it. The left stick still swims.",
        );
    ui.add_enabled_ui(s.enabled, |ui| {
        ui.add(
            egui::Slider::new(&mut s.speed_per_metre, 0.5..=12.0)
                .text("Stroke speed (m/s per metre of stroke)")
                .fixed_decimals(2),
        );
        ui.add(
            egui::Slider::new(&mut s.decay_s, 0.3..=4.0)
                .text("Glide between strokes (s)")
                .fixed_decimals(2),
        );
        ui.add(
            egui::Slider::new(&mut s.max_speed_mps, 0.5..=10.0)
                .text("Fastest stroke swim (m/s)")
                .fixed_decimals(1),
        );
        ui.add(
            egui::Slider::new(&mut s.min_speed_mps, 0.0..=1.0)
                .text("Slowest stroke (m/s)")
                .fixed_decimals(2),
        );
        ui.add(
            egui::Slider::new(&mut s.palm_cone_deg, 10.0..=89.0)
                .text("Palm facing the stroke within (°)")
                .fixed_decimals(0),
        );
        ui.add(
            egui::Slider::new(&mut s.turn_gain, 0.0..=10.0)
                .text("Stroke turn")
                .fixed_decimals(2),
        );
        ui.add(
            egui::Slider::new(&mut s.turn_smoothing_s, 0.05..=2.0)
                .text("Stroke turn smoothing (s)")
                .fixed_decimals(2),
        );
        ui.add(
            egui::Slider::new(&mut s.max_turn_deg_s, 0.0..=180.0)
                .text("Fastest stroke turn (°/s)")
                .fixed_decimals(0),
        );
        ui.add(
            egui::Slider::new(&mut s.catch_mps, 0.1..=2.0)
                .text("Stroke knock from (m/s)")
                .fixed_decimals(2),
        );
        ui.add(
            egui::Slider::new(&mut s.water_below_eyes_m, 0.0..=0.6)
                .text("Water line below the eyes at the surface (m)")
                .fixed_decimals(2),
        );
    });
}
