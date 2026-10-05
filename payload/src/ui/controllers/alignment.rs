//! The body alignment: the player's estimated torso, the body turning under the view, the chest
//! turn, and how far the character's skeleton is from the player's body; and its settings.

use std::time::Duration;

use egui::Ui;
use glam::Vec3;

use crate::{
    controllers::body::{
        alignment::{ALIGNMENT_TARGET, AlignmentConfig, AlignmentStatus},
        torso::TorsoConfig,
    },
    logging,
    ui::controllers::{vec3, yes_no},
};

pub(super) fn alignment_readout(ui: &mut Ui, status: &AlignmentStatus) {
    let mut verbose = logging::is_target_debug(ALIGNMENT_TARGET);
    if ui
        .checkbox(&mut verbose, "Log the alignment to the session log (1 Hz)")
        .on_hover_text(
            "Turns this subsystem up to DEBUG. Equivalent to a body_alignment=debug directive in \
             the Diagnostics tab's filter box.",
        )
        .changed()
        && let Err(error) = logging::set_target_debug(ALIGNMENT_TARGET, verbose)
    {
        tracing::warn!(%error, "could not change the body-alignment log level");
    }

    let deg = |r: f32| format!("{:+.1}°", r.to_degrees());
    let opt_deg = |r: Option<f32>| r.map_or_else(|| "n/a".to_owned(), deg);
    match &status.estimate {
        None => {
            ui.label("Torso estimate: none (no headset frame, or the estimate is off).");
        }
        Some(e) => {
            ui.label(format!(
                "Torso: heading {} · head {} · neck twist {}",
                deg(e.yaw),
                deg(e.head_yaw),
                deg(e.neck_twist),
            ));
            ui.label(format!(
                "Hands: heading {} · trust {:.2} · walk trust {:.2}",
                opt_deg(e.hands_yaw),
                e.hands_confidence,
                e.walk_confidence,
            ));
            ui.label(format!(
                "Chest {} · pelvis {} · lean {:.1}° (cockpit frame)",
                vec3(e.chest),
                vec3(e.pelvis),
                e.spine.angle_between(Vec3::Y).to_degrees(),
            ));
        }
    }
    ui.label(format!(
        "Body follows the torso: {} · body from view {} · target {}",
        yes_no(status.following),
        deg(status.follow_offset),
        deg(status.follow_target),
    ));
    ui.label(format!("Chest turn queued: {}", opt_deg(status.chest_turn)));
    ui.label(format!(
        "Holsters ride: {}",
        status.volume_anchor.unwrap_or("nothing yet")
    ));
    let skeleton_fresh = status
        .skeleton_at
        .is_some_and(|at| at.elapsed() < Duration::from_millis(500));
    match (&status.divergence, skeleton_fresh) {
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

pub(super) fn alignment_settings(ui: &mut Ui, c: &mut AlignmentConfig) {
    ui.checkbox(&mut c.estimate, "Estimate the torso")
        .on_hover_text(
            "From the head and the hands. Feeds the readout, the holsters, and everything below.",
        );
    ui.add_enabled_ui(c.estimate, |ui| {
        ui.checkbox(&mut c.root_follow, "Turn the body to face the torso")
            .on_hover_text(
                "On foot, Rico turns under the view to face where the player's torso faces. \
                 Only the turn input turns the view.",
            );
        ui.add(egui::Slider::new(&mut c.follow_deadband_deg, 0.0..=45.0).text("Body deadband (°)"));
        ui.checkbox(&mut c.drive_chest, "Turn the chest to the torso")
            .on_hover_text(
                "The body IK turns the chest toward the player's torso instead of by a share of \
                 the head's turn. Needs the body IK.",
            );
        ui.add(egui::Slider::new(&mut c.chest_weight, 0.0..=1.0).text("Chest weight"));
        ui.add(egui::Slider::new(&mut c.chest_limit_deg, 0.0..=90.0).text("Chest limit (°)"));
        ui.add(egui::Slider::new(&mut c.stance_turn_share, 0.0..=1.0).text("Squared pelvis share"))
            .on_hover_text(
                "How far the squared stance turns the pelvis and feet toward the torso.",
            );
        ui.checkbox(&mut c.torso_shoulders, "Arm pivots follow the torso")
            .on_hover_text(
                "Estimate the player's shoulders for the hand remap from the torso's heading \
                 rather than the head's.",
            );
        egui::CollapsingHeader::new("Torso estimate")
            .show(ui, |ui| torso_settings(ui, &mut c.torso));
    });
    if ui.button("Reset body alignment").clicked() {
        *c = AlignmentConfig::new();
    }
}

fn torso_settings(ui: &mut Ui, t: &mut TorsoConfig) {
    ui.add(egui::Slider::new(&mut t.deadzone_deg, 0.0..=60.0).text("Head deadzone (°)"));
    ui.add(egui::Slider::new(&mut t.neck_limit_deg, 20.0..=120.0).text("Neck limit (°)"));
    ui.add(egui::Slider::new(&mut t.follow_s, 0.0..=2.0).text("Follow time (s)"));
    ui.add(egui::Slider::new(&mut t.hands_weight, 0.0..=1.0).text("Hands weight"));
    ui.add(
        egui::Slider::new(&mut t.hands_min_spread_m, 0.05..=0.6).text("Hands minimum spread (m)"),
    );
    ui.add(egui::Slider::new(&mut t.walk_weight, 0.0..=1.0).text("Walk weight"));
    ui.add(egui::Slider::new(&mut t.walk_speed_mps, 0.1..=2.0).text("Walk speed (m/s)"));
    ui.add(egui::Slider::new(&mut t.pelvis_follow_s, 0.0..=3.0).text("Pelvis follow time (s)"));
    ui.add(egui::Slider::new(&mut t.max_lean_deg, 0.0..=80.0).text("Lean limit (°)"));
}
