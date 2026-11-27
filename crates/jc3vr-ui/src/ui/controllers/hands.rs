//! Controller-owned hands: the render-rate body's row, the IK, tier, and late-update readouts, and
//! the grip offset.

use egui::Ui;
use jc3vr_body::controllers::{
    body,
    hands::{
        config::{GripOffset, HandsConfig},
        ik::HandIkStatus,
        late::{HandLateStatus, LateStatus},
        parts, rig,
        tiers::{TierFrame, TierState},
    },
};
use jc3vr_input::frame::Hand;
use jc3vr_weapons::controllers::weapons::in_hand;

use crate::{
    status::body::RENDER_RATE_BODY,
    ui::{
        controllers::{vec3_drag, yes_no},
        status_row::headlines,
    },
};

pub(super) fn hands_readout(
    ui: &mut Ui,
    ik: &[HandIkStatus; 2],
    tiers: &TierFrame,
    late: &LateStatus,
) {
    headlines(ui, &[&RENDER_RATE_BODY]);
    ui.label(format!(
        "State: {}",
        tiers.state.map_or("none yet", TierState::label)
    ));
    let arms = rig::status();
    let player_arm = body::player().arm_reach();
    ui.label(format!(
        "Player reach (shoulder to wrist, forward or down, from the body dimensions): {:.0} mm",
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
        "Late-updated gun matrices {} · gun parts {} · held explosives {} · palette uploads \
         edited {}",
        late.weapons_updated,
        parts::parts_updated(),
        in_hand::updated(),
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

pub(super) fn grip_settings(ui: &mut Ui, h: &mut HandsConfig) {
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
}
