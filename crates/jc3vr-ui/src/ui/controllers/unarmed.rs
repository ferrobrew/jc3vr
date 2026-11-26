//! The weapon-agnostic body: whether it applies and which layers it leaves out of the pose.

use std::time::Duration;

use egui::Ui;
use jc3vr_body::controllers::{
    body::unarmed::{MASKABLE_LAYERS, UnarmedStatus, UnarmedVerdict, layer_label},
    hands::ik::HandIkStatus,
};
use jc3vr_input::frame::Hand;

use crate::{tuning::UI, ui::controllers::yes_no};

pub(super) fn unarmed_readout(ui: &mut Ui, unarmed: &UnarmedStatus, hand_ik: &[HandIkStatus; 2]) {
    if !unarmed
        .at
        .is_some_and(|at| at.elapsed() < Duration::from_secs_f32(UI.get().readouts.unarmed_stale_s))
    {
        ui.label("Weapon animations: the local player's pose finalization has not run recently.");
        return;
    }
    ui.label(format!(
        "Weapon animations: {}",
        match unarmed.verdict {
            UnarmedVerdict::Applies => "unarmed body applies".to_owned(),
            UnarmedVerdict::StandDown(reason) => format!("native ({})", reason.label()),
            UnarmedVerdict::NotRun => "not run".to_owned(),
        }
    ));
    ui.label(format!(
        "Weapon modifiers kept off the body: {}",
        yes_no(unarmed.weapon_suppressed)
    ));
    ui.label(format!(
        "Relaxed locomotion: {} ({} wielded checks read unarmed)",
        yes_no(unarmed.wielded_relaxed),
        unarmed.relaxed_evaluations
    ));
    let layers = MASKABLE_LAYERS
        .iter()
        .enumerate()
        .map(|(i, layer)| {
            format!(
                "{} {} ({} skipped)",
                layer_label(*layer),
                if unarmed.masked[i] { "masked" } else { "shown" },
                unarmed.skipped[i],
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");
    ui.label(format!("Layers ({} in all): {layers}", unarmed.layers));
    ui.label(format!(
        "Game weapon grip switched off: left {} · right {}",
        yes_no(hand_ik[Hand::Left.index()].secondary_grip_off),
        yes_no(hand_ik[Hand::Right.index()].secondary_grip_off),
    ));
}
