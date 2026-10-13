//! The weapon-agnostic body: whether it applies, which layers it leaves out of the pose, and its
//! settings.

use std::time::Duration;

use egui::Ui;

use crate::{
    controllers::{
        body::unarmed::{
            MASKABLE_LAYERS, UnarmedBodyConfig, UnarmedStatus, UnarmedVerdict, WeaponAnimations,
            layer_label,
        },
        frame::Hand,
        hands::ik::HandIkStatus,
    },
    ui::controllers::yes_no,
};

pub(super) fn unarmed_readout(ui: &mut Ui, unarmed: &UnarmedStatus, hand_ik: &[HandIkStatus; 2]) {
    if !unarmed.at.is_some_and(|at| at.elapsed() < STALE) {
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

pub(super) fn unarmed_settings(ui: &mut Ui, c: &mut UnarmedBodyConfig) {
    ui.horizontal(|ui| {
        ui.label("Weapon animations on foot:");
        for setting in WeaponAnimations::ALL {
            ui.selectable_value(&mut c.weapon_animations, setting, setting.label());
        }
    })
    .response
    .on_hover_text(
        "Unarmed: the body plays its unarmed animation whatever is held, and the controllers \
         alone place the arms and the guns. Native: the game's weapon stances and actions.",
    );
    ui.add_enabled_ui(c.weapon_animations == WeaponAnimations::Unarmed, |ui| {
        ui.indent("unarmed", |ui| {
            ui.checkbox(&mut c.mask_actions, "Hide the upper-body actions")
                .on_hover_text(
                    "Draw, holster, switch, reload, pump, bolt, pickup, and throws still run \
                     and keep their timing, but leave the body alone.",
                );
            ui.checkbox(&mut c.mask_synced_partial, "Hide the synced upper body")
                .on_hover_text("The turns toward the aim target and the locomotion partials.");
            ui.checkbox(&mut c.mask_additive, "Hide the additive layer")
                .on_hover_text(
                    "Leans, landings, and the grapple, grenade, and launcher fire additives.",
                );
            ui.checkbox(&mut c.release_weapon_grip, "Release the game's weapon grip")
                .on_hover_text(
                    "Never put a hand on a two-handed weapon's grip; only the foregrip grab does.",
                );
        });
    });
}

/// How old a readout may be before it counts as inactive.
const STALE: Duration = Duration::from_millis(500);
