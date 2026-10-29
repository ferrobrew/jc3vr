//! The rig's torso: the player's estimated torso, the body turning under the view, the chest turn,
//! and how far the character's skeleton is from the player's body.

use std::time::Duration;

use egui::Ui;

use crate::{
    controllers::body::{
        procedural,
        rig::{TorsoStatus, skeleton::SkeletonStatus},
    },
    ui::controllers::{vec3, yes_no},
};

pub(super) fn torso_readout(ui: &mut Ui, status: &TorsoStatus, skeleton: &SkeletonStatus) {
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
