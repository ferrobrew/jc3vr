//! Controller-owned hands: the IK, tier, and late-update readouts, and the hand settings.

use egui::Ui;

use crate::{
    controllers::{
        config::{GripOffset, HandsConfig},
        frame::Hand,
        hands::{
            ik::HandIkStatus,
            late::LateStatus,
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
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let i = hand.index();
            let s = &ik[i];
            ui.strong(hand.label());
            ui.label(format!(
                "tier {} (acts as {}) · weight {:.2}",
                tiers.tiers[i].label(),
                tiers.tiers[i].effective().label(),
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
                "secondary off: wrist {} · grip {}{}",
                yes_no(s.secondary_wrist_off),
                yes_no(s.secondary_grip_off),
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
        }
    });
    ui.label(format!(
        "Late-updated gun matrices {} · palette uploads edited {}",
        late.weapons_updated, late.palettes_edited
    ));
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
}
