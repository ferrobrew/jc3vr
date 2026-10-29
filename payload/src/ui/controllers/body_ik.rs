//! The headset-driven body IK's readout: whether it queued targets last frame, the effectors it
//! drove, and the values the engine's solver holds.

use egui::Ui;

use crate::hooks;

pub(super) fn body_ik_readout(ui: &mut Ui) {
    match hooks::character::body_ik_status() {
        None => {
            ui.label("Status: the character hook has not run yet.");
        }
        Some(status) => match status.skip {
            Some(skip) => {
                ui.label(format!(
                    "Status: queuing nothing — {} (mode {:?}).",
                    skip.label(),
                    status.mode
                ));
            }
            None => {
                ui.label(format!(
                    "Mode: {:?}   State id: {:#010x}",
                    status.mode, status.state_id
                ));
                ui.label(format!(
                    "Effectors: head {}, chest end {}",
                    status.head_effector, status.chest_effector
                ));
                ui.label(format!(
                    "Head target delta: {:.3} m   Room-scale offset: {:.3} m ({:+.2}, {:+.2}, \
                     {:+.2})",
                    status.target_delta,
                    status.head_offset.length(),
                    status.head_offset.x,
                    status.head_offset.y,
                    status.head_offset.z
                ));
                ui.label(format!(
                    "Torso lean: {:.1}°   Torso yaw: {:+.1}°   Shoulders: {}/2 driven, {}/2 \
                     mapped   Chest driven: {}   Hands pinned: {}/2{}",
                    status.torso_lean_deg,
                    status.torso_yaw_deg,
                    status.shoulders_driven,
                    status.shoulders_mapped,
                    if status.chest_driven { "yes" } else { "no" },
                    status.hands_pinned,
                    if status.deferred_to_aim {
                        "   (deferring to the game's aim)"
                    } else {
                        ""
                    }
                ));
                ui.label(format!(
                    "Engine-side head values: reachT {:.2}, reachR {:.2}, pull {:.2}",
                    status.engine_reach_t, status.engine_reach_r, status.engine_pull
                ));
            }
        },
    }
}
