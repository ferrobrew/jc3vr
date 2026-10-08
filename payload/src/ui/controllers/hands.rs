//! Controller-owned hands: the IK, tier, and late-update readouts, and the hand settings.

use egui::Ui;

use crate::{
    controllers::{
        body,
        config::{GripOffset, HandsConfig},
        frame::Hand,
        hands::{
            ik::HandIkStatus,
            late::{HandLateStatus, LateStatus},
            parts,
            reach::{self, ReachConfig},
            rig,
            tiers::{HandTier, HandTierTable, TierState, TierStatus},
        },
    },
    ui::controllers::{vec3_drag, yes_no},
};

pub(super) fn hands_readout(
    ui: &mut Ui,
    ik: &[HandIkStatus; 2],
    tiers: &TierStatus,
    late: &LateStatus,
) {
    ui.label(format!(
        "State: {}",
        tiers.state.map_or("none yet", TierState::label)
    ));
    let arms = rig::status();
    let player_arm = reach::player_arm_length(&body::player());
    ui.label(format!(
        "Player arm (shoulder to wrist, from the body dimensions): {:.0} mm",
        player_arm * 1000.0
    ));
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let i = hand.index();
            let s = &ik[i];
            ui.strong(hand.label());
            ui.label(format!(
                "tier {} (acts as {}) · weight {:.2}",
                tiers.tiers[i].label(),
                tiers.effective[i].label(),
                tiers.weights[i]
            ));
            ui.label(match s.skip {
                Some(skip) => format!("IK: {}", skip.label()),
                None => format!(
                    "IK: queued at {:.2} · reach {:.0} mm · turn {:.0}°",
                    s.weight,
                    s.reach_distance * 1000.0,
                    s.rotation_deg
                ),
            });
            ui.label(format!(
                "secondary off: wrist {} · grip {}{}{}",
                yes_no(s.secondary_wrist_off),
                yes_no(s.secondary_grip_off),
                if s.secondary_wrist_held {
                    " · wrist held"
                } else {
                    ""
                },
                if s.overrode_live_target {
                    " · overrode a game target"
                } else {
                    ""
                }
            ));
            ui.label(match late.deltas[i] {
                Some((shift, turn)) => {
                    format!("late update: {:.1} mm · {:.1}°", shift * 1000.0, turn)
                }
                None => "late update: none".to_owned(),
            });
            arm_readout(ui, &arms[i], s, &late.hands[i], player_arm);
        }
    });
    ui.label(format!(
        "Late-updated gun matrices {} · gun parts {} · palette uploads edited {}",
        late.weapons_updated,
        parts::parts_updated(),
        late.palettes_edited
    ));
}

/// One arm's measurement, remap, pull, and detach readouts.
fn arm_readout(
    ui: &mut Ui,
    arm: &rig::ArmStatus,
    ik: &HandIkStatus,
    late: &HandLateStatus,
    player_arm: f32,
) {
    match arm.arm {
        Some(a) => {
            ui.label(format!(
                "Rico's arm: upper {:.0} · forearm {:.0} · total {:.0} mm (ratio {:.3})",
                a.upper_arm * 1000.0,
                a.forearm * 1000.0,
                a.length() * 1000.0,
                a.length() / player_arm
            ));
            let e = a.eye_to_shoulder * 1000.0;
            ui.label(format!(
                "eyes → shoulder: right {:.0} · up {:.0} · back {:.0} mm",
                e.x, e.y, e.z
            ));
        }
        None => {
            ui.label(if arm.resolved {
                "Rico's arm: no plausible sample yet"
            } else {
                "Rico's arm: joints unresolved"
            });
        }
    }
    ui.label(format!(
        "samples {} · dropped {}",
        arm.samples, arm.rejected
    ));
    if ik.queued {
        ui.label(format!(
            "extension {} · pull {:.2} · shoulder pin released {:.0}%",
            ik.extension
                .map_or("unmeasured".to_owned(), |e| format!("{:.0}%", e * 100.0)),
            ik.pull,
            ik.shoulder_yield * 100.0
        ));
        if ik.at_gate {
            ui.label(format!(
                "on the procedural torso · kept out {} · elbow hint {}",
                mm(Some(ik.kept_out_m)),
                if ik.elbow_hint { "queued" } else { "none" }
            ));
        }
    }
    ui.label(format!(
        "solve miss {} · detach {}",
        mm(late.solve_miss_m),
        mm(late.detach_m)
    ));
}

fn mm(metres: Option<f32>) -> String {
    metres.map_or("none".to_owned(), |m| format!("{:.0} mm", m * 1000.0))
}

fn reach_settings(ui: &mut Ui, r: &mut ReachConfig) {
    ui.checkbox(
        &mut r.measured_remap,
        "Remap about Rico's measured shoulders",
    )
    .on_hover_text(
        "Scale the hands' offsets from your shoulders by Rico's measured arm length over yours, \
             about his animated shoulder joints. Off, the estimated remap applies when \
             \"Remap hand reach\" is on.",
    );
    ui.add(
        egui::Slider::new(&mut r.shoulder_smoothing_s, 0.0..=1.0).text("Shoulder smoothing (s)"),
    );
    ui.checkbox(&mut r.reach_pull, "Pull the torso near full extension")
        .on_hover_text("Ramp the wrist pull up so Rico's shoulder and chest follow a reach.");
    ui.add_enabled_ui(r.reach_pull, |ui| {
        ui.add(egui::Slider::new(&mut r.pull_start, 0.5..=1.2).text("Pull starts at (× arm)"));
        ui.add(egui::Slider::new(&mut r.pull_full, 0.5..=1.5).text("Full pull at (× arm)"));
        ui.add(egui::Slider::new(&mut r.pull_max, 0.0..=1.0).text("Pull at full extension"));
    });
    ui.checkbox(&mut r.detach, "Detach the rendered hand from a short arm")
        .on_hover_text(
            "Move the rendered hand off the end of an arm that cannot reach, toward the \
             controller, up to the cap. Needs the render-time late update.",
        );
    ui.add_enabled_ui(r.detach, |ui| {
        ui.add(
            egui::Slider::new(&mut r.detach_threshold_m, 0.0..=0.05).text("Detach threshold (m)"),
        );
        ui.add(egui::Slider::new(&mut r.detach_cap_m, 0.0..=0.3).text("Detach cap (m)"));
        ui.checkbox(&mut r.stretch_forearm, "Stretch the forearm to the hand");
    });
    if ui.button("Reset reach").clicked() {
        *r = ReachConfig::new();
    }
}

pub(super) fn hands_settings(ui: &mut Ui, h: &mut HandsConfig) {
    ui.checkbox(&mut h.ik, "Hand IK")
        .on_hover_text("Drive the wrists to the controllers with HumanIK.");
    ui.checkbox(&mut h.late_update, "Render-time late update")
        .on_hover_text("Re-pose the hands and the held gun from the latest controller pose.");
    ui.checkbox(
        &mut h.suppress_secondary_ik,
        "Switch off the game's secondary hand IK",
    )
    .on_hover_text("The weapon off-hand grip, the grapple fire arm, and the parachute handles.");
    ui.checkbox(&mut h.rigid_grip, "Rigid grip").on_hover_text(
        "Hold a gun fixed in a controller-driven hand, whatever the animation does.",
    );
    ui.add(egui::Slider::new(&mut h.reach_t, 0.0..=1.0).text("Position reach"));
    ui.add(egui::Slider::new(&mut h.reach_r, 0.0..=1.0).text("Rotation reach"));
    ui.add(egui::Slider::new(&mut h.pull, 0.0..=1.0).text("Pull"));
    ui.add(egui::Slider::new(&mut h.resist, 0.0..=1.0).text("Resist"));
    ui.add(egui::Slider::new(&mut h.yield_blend_s, 0.0..=2.0).text("Yield blend (s)"));
    ui.add(egui::Slider::new(&mut h.return_blend_s, 0.0..=2.0).text("Return blend (s)"));
    ui.label("Grip offset (right hand, grip frame; the left mirrors it)");
    vec3_drag(ui, "Wrist position (m)", &mut h.grip.position_m, 0.005);
    vec3_drag(
        ui,
        "Rotation (°, X then Y then Z)",
        &mut h.grip.rotation_deg,
        1.0,
    );
    if ui.button("Reset grip offset").clicked() {
        h.grip = GripOffset::INDEX_BEST_GUESS;
    }
    egui::CollapsingHeader::new("Arm reach").show(ui, |ui| reach_settings(ui, &mut h.reach));
    egui::CollapsingHeader::new("Hand tiers").show(ui, |ui| {
        for state in TierState::ALL {
            let tier = h.tiers.tier_mut(state);
            egui::ComboBox::from_label(state.label())
                .selected_text(tier.label())
                .show_ui(ui, |ui| {
                    for option in HandTier::ALL {
                        ui.selectable_value(tier, option, option.label());
                    }
                });
        }
        if ui.button("Reset to the plan").clicked() {
            h.tiers = HandTierTable::PLAN;
        }
    });
    let a = &mut h.anchor;
    ui.checkbox(&mut a.enabled, "Anchor the grapple hand to the line")
        .on_hover_text("Off, the grapple states yield the hand to the animation.");
    ui.add_enabled_ui(a.enabled, |ui| {
        ui.indent("anchor", |ui| {
            ui.add(egui::Slider::new(&mut a.blend_s, 0.0..=1.0).text("Anchor blend (s)"));
            ui.checkbox(&mut a.hang_swing, "Swing a hang with the hand");
            ui.add(
                egui::Slider::new(&mut a.swing_deadzone_m, 0.0..=0.2).text("Swing deadzone (m)"),
            );
            ui.add(egui::Slider::new(&mut a.swing_full_m, 0.05..=0.6).text("Full swing at (m)"));
            ui.add(
                egui::Slider::new(&mut a.swing_scale, -3.0..=3.0)
                    .text("Swing strength (× the game's)"),
            );
        });
    });
}
