//! The rig's torso: the player's estimated torso, the body turning under the view, the chest turn,
//! and how far the character's skeleton is from the player's body; and the torso's and the
//! pelvis's settings.

use std::time::Duration;

use egui::Ui;

use crate::{
    controllers::body::{
        procedural,
        rig::{
            TorsoStatus,
            config::{PelvisConfig, TorsoConfig},
            skeleton::{SKELETON_TARGET, SkeletonStatus},
        },
    },
    logging,
    ui::controllers::{vec3, yes_no},
};

pub(super) fn torso_readout(ui: &mut Ui, status: &TorsoStatus, skeleton: &SkeletonStatus) {
    let mut verbose = logging::is_target_debug(SKELETON_TARGET);
    if ui
        .checkbox(&mut verbose, "Log the divergence to the session log (1 Hz)")
        .on_hover_text(
            "Turns this subsystem up to DEBUG. Equivalent to a body_alignment=debug directive in \
             the Diagnostics tab's filter box.",
        )
        .changed()
        && let Err(error) = logging::set_target_debug(SKELETON_TARGET, verbose)
    {
        tracing::warn!(%error, "could not change the body-alignment log level");
    }

    let deg = |r: f32| format!("{:+.1}°", r.to_degrees());
    let opt_deg = |r: Option<f32>| r.map_or_else(|| "n/a".to_owned(), deg);
    match &status.estimate {
        None => {
            ui.label("Torso estimate: none (no headset frame).");
        }
        Some(e) => {
            ui.label(format!(
                "Torso: heading {} · head {} · neck twist {} · hands {} (weight {:.2})",
                deg(e.yaw),
                deg(e.head_yaw),
                deg(e.neck_twist),
                opt_deg(e.hands_yaw),
                e.hands_weight,
            ));
            ui.label(format!(
                "Chest {} · pelvis {} (cockpit frame)",
                vec3(e.chest),
                vec3(e.pelvis),
            ));
        }
    }
    ui.label(format!(
        "Body follows the torso: {} · body from view {} · target {}",
        yes_no(status.following),
        deg(status.follow_offset),
        deg(status.follow_target),
    ));
    ui.label(format!(
        "Chest turn queued: {} · motion lean {:.1}°",
        opt_deg(status.chest_turn),
        procedural::published_status().motion_lean_deg,
    ));
    ui.label(format!(
        "Holsters ride: {}",
        skeleton.volume_anchor.unwrap_or("nothing yet")
    ));
    let skeleton_fresh = skeleton
        .at
        .is_some_and(|at| at.elapsed() < Duration::from_millis(500));
    match (&skeleton.divergence, skeleton_fresh) {
        (Some(d), true) => {
            ui.label(
                "Divergence after the solve, the player less Rico, in his model space (+Y up, -Z \
                 his forward):",
            );
            ui.label(format!(
                "Turns: torso from facing {} · from chest {} · from pelvis {}",
                deg(d.root_turn),
                opt_deg(d.chest_turn),
                opt_deg(d.pelvis_turn),
            ));
            ui.label(format!(
                "Chest {} · pelvis {}",
                vec3(d.chest),
                vec3(d.pelvis)
            ));
            ui.label(format!(
                "Shoulders L {} R {} · hips L {} R {}",
                vec3(d.shoulders[0]),
                vec3(d.shoulders[1]),
                vec3(d.hips[0]),
                vec3(d.hips[1]),
            ));
        }
        _ => {
            ui.label("Divergence: no fresh skeleton (not on foot in gameplay under the headset).");
        }
    }
}

pub(super) fn torso_settings(ui: &mut Ui, t: &mut TorsoConfig, p: &mut PelvisConfig) {
    ui.checkbox(&mut t.follow, "Body faces the torso")
        .on_hover_text("On foot, turn the character to face the player's torso under the view.");
    ui.add(egui::Slider::new(&mut t.follow_deadband_deg, 0.0..=45.0).text("Follow deadband (°)"));
    ui.add(egui::Slider::new(&mut t.chest_limit_deg, 0.0..=90.0).text("Chest turn limit (°)"));
    ui.add(egui::Slider::new(&mut t.head_weight, 0.0..=2.0).text("Head weight against the hands"));
    ui.add(egui::Slider::new(&mut t.neck_limit_deg, 20.0..=120.0).text("Neck limit (°)"));
    ui.add(egui::Slider::new(&mut t.follow_s, 0.0..=0.5).text("Torso follow time (s)"));
    ui.separator();
    ui.add(egui::Slider::new(&mut p.follow_s, 0.0..=0.5).text("Pelvis follow time (s)"));
    ui.add(egui::Slider::new(&mut p.hinge_share, 0.0..=1.0).text("Hip hinge share of the lean"));
    ui.add(egui::Slider::new(&mut p.speed_lean, 0.0..=0.2).text("Lean per m/s (rad)"));
    ui.add(egui::Slider::new(&mut p.max_speed_lean, 0.0..=0.5).text("Most speed lean (rad)"));
    ui.add(egui::Slider::new(&mut p.accel_lean, 0.0..=0.1).text("Lean per m/s² (rad)"));
    ui.add(
        egui::Slider::new(&mut p.max_accel_lean, 0.0..=0.5).text("Most acceleration lean (rad)"),
    );
    ui.add(egui::Slider::new(&mut p.max_roll, 0.0..=0.3).text("Most roll (rad)"));
    ui.add(egui::Slider::new(&mut p.motion_smoothing_s, 0.0..=0.5).text("Motion smoothing (s)"));
    if ui.button("Reset the torso and the pelvis").clicked() {
        *t = TorsoConfig::new();
        *p = PelvisConfig::new();
    }
}
