//! The standing stance: what the stance logic decided on its last tick, and its settings.

use std::time::{Duration, Instant};

use egui::Ui;

use crate::{
    controllers::roomscale::stance::{StanceConfig, StanceStatus, StateName, logic::Verdict},
    ui::controllers::yes_no,
};

pub(super) fn stance_readout(ui: &mut Ui, status: &StanceStatus) {
    let fresh = |at: Option<Instant>| at.is_some_and(|at| at.elapsed() < STALE);
    if !fresh(status.at) {
        ui.label("Inactive: the local player's pose finalization has not run recently.");
    }
    ui.label(format!(
        "State: {}",
        match status.state {
            StateName::Unreadable => "unreadable".to_owned(),
            StateName::Named(name) => name.to_owned(),
            StateName::Hash(hash) => format!("{hash:#010x} (not a listed idle or move state)"),
        }
    ));
    ui.label(format!(
        "Decision: {} · speed {} · blend {:.2}",
        match status.verdict {
            Verdict::StandingStill => "standing still".to_owned(),
            Verdict::StandDown(reason) => format!("stand down ({})", reason.label()),
            Verdict::NotRun => "not run".to_owned(),
        },
        status
            .speed_mps
            .map_or_else(|| "unknown".to_owned(), |s| format!("{s:.2} m/s")),
        status.weight,
    ));
    let m = &status.measured;
    let deg = |d: Option<f32>| d.map_or_else(|| "n/a".to_owned(), |d| format!("{d:+.1}°"));
    ui.label(format!(
        "Animated turns: pelvis {} · chest {} · feet {} / {}",
        deg(m.pelvis_deg),
        deg(m.chest_deg),
        deg(m.foot_deg[0]),
        deg(m.foot_deg[1]),
    ));
    ui.label(match m.feet {
        Some(feet) => format!(
            "Animated feet: {:.2} m apart · right foot {:+.2} m ahead of the left",
            feet.spacing_m, feet.stagger_m
        ),
        None => "Animated feet: unreadable".to_owned(),
    });
    let q = &status.queued;
    ui.label(match q.skipped {
        Some(reason) => format!("Squaring: skipped ({reason})"),
        None if status.weight > 0.0 => format!(
            "Squaring: hips held {} · pelvis {} · chest {:+.1}° (through the body IK) · ankles {} · feet turned {}",
            yes_no(q.hips_position),
            deg(q.pelvis_deg),
            q.chest_deg,
            q.ankles,
            q.feet_turned,
        ),
        None => "Squaring: nothing queued".to_owned(),
    });
    let modifiers = &status.modifiers;
    if fresh(modifiers.at) {
        ui.label(format!(
            "Idle clip: {} · relaxed idle applied {}",
            modifiers.variant.label(),
            yes_no(modifiers.relaxed_applied),
        ));
    } else {
        ui.label("Idle clip: the modifier pass has not run recently.");
    }
}

pub(super) fn stance_settings(ui: &mut Ui, c: &mut StanceConfig) {
    ui.checkbox(&mut c.relaxed_idle, "Relaxed idle while standing still")
        .on_hover_text(
            "Keep the weapon's stance off the body's idle, so it plays the unarmed relaxed clip.",
        );
    ui.checkbox(&mut c.square, "Square the standing stance")
        .on_hover_text(
            "Turn the pelvis, chest, and feet to face forward and set the feet side by side.",
        );
    ui.add_enabled_ui(c.square, |ui| {
        ui.indent("stance", |ui| {
            ui.add(egui::Slider::new(&mut c.settle_s, 0.0..=2.0).text("Settle (s)"));
            ui.add(egui::Slider::new(&mut c.blend_in_s, 0.0..=2.0).text("Blend in (s)"));
            ui.add(egui::Slider::new(&mut c.blend_out_s, 0.0..=1.0).text("Blend out (s)"));
            ui.add(egui::Slider::new(&mut c.still_speed_mps, 0.0..=1.0).text("Still below (m/s)"));
            ui.add(
                egui::Slider::new(&mut c.feet_width_m, 0.0..=0.6)
                    .text("Feet width (m, 0 keeps the animation's)"),
            );
            ui.add(egui::Slider::new(&mut c.pelvis_share, 0.0..=1.0).text("Pelvis share"));
            ui.add(
                egui::Slider::new(&mut c.chest_share, 0.0..=1.0)
                    .text("Chest share (needs the body IK)"),
            );
            ui.add(egui::Slider::new(&mut c.feet_yaw_share, 0.0..=1.0).text("Feet turn share"));
            ui.add(egui::Slider::new(&mut c.ankle_resist, 0.0..=1.0).text("Ankle resist"));
        });
    });
}

/// How old a readout may be before it counts as inactive.
const STALE: Duration = Duration::from_millis(500);
