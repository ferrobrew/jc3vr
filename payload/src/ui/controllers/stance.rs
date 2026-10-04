//! The standing stance: what the stance logic decided on its last tick, and its settings.

use std::time::{Duration, Instant};

use egui::Ui;

use crate::{
    controllers::{
        frame::Hand,
        hands::ik::HandIkStatus,
        roomscale::stance::{
            StanceConfig, StanceStatus, StateName,
            config::{UnarmedBodyConfig, WeaponAnimations},
            logic::Verdict,
            unarmed::{MASKABLE_LAYERS, UnarmedStatus, UnarmedVerdict, layer_label},
        },
    },
    ui::controllers::yes_no,
};

pub(super) fn stance_readout(
    ui: &mut Ui,
    status: &StanceStatus,
    unarmed: &UnarmedStatus,
    hand_ik: &[HandIkStatus; 2],
) {
    let fresh = |at: Option<Instant>| at.is_some_and(|at| at.elapsed() < STALE);
    unarmed_readout(ui, unarmed, status, hand_ik);
    ui.separator();
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
            "Idle clip: {} · relaxed idle applied {} · unarmed body applied {}",
            modifiers.variant.label(),
            yes_no(modifiers.relaxed_applied),
            yes_no(modifiers.unarmed_applied),
        ));
    } else {
        ui.label("Idle clip: the modifier pass has not run recently.");
    }
}

pub(super) fn stance_settings(ui: &mut Ui, c: &mut StanceConfig) {
    unarmed_settings(ui, &mut c.unarmed);
    ui.add_enabled(
        c.unarmed.weapon_animations == WeaponAnimations::Native,
        egui::Checkbox::new(&mut c.relaxed_idle, "Relaxed idle while standing still"),
    )
    .on_hover_text(
        "Keep the weapon's stance off the body's idle, so it plays the unarmed relaxed clip. The \
         unarmed weapon animations already do this on foot.",
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

fn unarmed_readout(
    ui: &mut Ui,
    unarmed: &UnarmedStatus,
    stance: &StanceStatus,
    hand_ik: &[HandIkStatus; 2],
) {
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
        yes_no(stance.modifiers.unarmed_applied)
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

fn unarmed_settings(ui: &mut Ui, c: &mut UnarmedBodyConfig) {
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
