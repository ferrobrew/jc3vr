//! Weapon handling: the grab volume each hand is in, the wielded weapon, the grabs and what they
//! did, the foregrip, the hand tiers, and the holster and foregrip settings.

use egui::Ui;

use crate::{
    controllers::{
        config::WeaponsConfig,
        frame::Hand,
        hands::tiers::{TierState, TierStatus},
        weapons::{
            WeaponsStatus,
            config::{ForegripConfig, HolstersConfig, VolumeSpec},
            foregrip::ForegripStatus,
            holster::slot_label,
        },
    },
    ui::controllers::{vec3, vec3_drag, yes_no},
};

pub(super) fn weapons_readout(
    ui: &mut Ui,
    status: &WeaponsStatus,
    foregrip: &ForegripStatus,
    tiers: &TierStatus,
) {
    if !status.active {
        ui.label("Inactive: the bindings are not injecting.");
    }
    ui.label(format!(
        "State: {} · volumes placed {} · fire held back {}",
        tiers.state.map_or("none yet", TierState::label),
        status.candidates,
        yes_no(status.fire_blocked)
    ));
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            ui.strong(hand.label());
            ui.label(format!(
                "in: {}",
                status.volumes[hand.index()].map_or("no volume", |v| v.label())
            ));
            ui.label(format!(
                "tier: {} (acts as {})",
                tiers.tiers[hand.index()].label(),
                tiers.tiers[hand.index()].effective().label()
            ));
        }
    });
    match &status.wielded {
        Some(w) => {
            let hands = Hand::BOTH
                .iter()
                .filter(|h| w.weapon.wielded.hands[h.index()])
                .map(|h| h.label())
                .collect::<Vec<_>>()
                .join(" and ");
            ui.label(format!(
                "Wielded: {} in {} · two-handed {}",
                slot_label(w.weapon.wielded.slot),
                if hands.is_empty() { "no hand" } else { &hands },
                yes_no(w.weapon.two_handed)
            ));
            if let Some(g) = w.geometry {
                ui.label(format!(
                    "Foregrip {} in the wrist frame ({}) · barrel {}",
                    vec3(g.foregrip),
                    if g.native {
                        "the weapon's grip"
                    } else {
                        "fallback"
                    },
                    vec3(g.barrel)
                ));
            } else if w.weapon.two_handed {
                ui.label("Foregrip: not measured yet");
            }
        }
        None => {
            ui.label("Wielded: nothing");
        }
    }
    ui.label(match foregrip.hold {
        Some(h) if h.engaged => format!(
            "Foregrip: held by the {} hand · weight {:.2}",
            h.holder.other().label(),
            h.weight
        ),
        Some(h) => format!("Foregrip: letting go · weight {:.2}", h.weight),
        None => "Foregrip: not held".to_owned(),
    });
    ui.label(format!(
        "Foregrip grabs {} · releases {}{}",
        foregrip.grabs,
        foregrip.releases,
        foregrip
            .last_release
            .map_or(String::new(), |r| format!(" (last: {})", r.label()))
    ));
    ui.label(format!("Grabs: {}", status.grabs));
    for event in status.events.iter().rev() {
        ui.label(format!(
            "  {} at the {}: {}",
            event.hand.label(),
            event.volume.label(),
            event.outcome.label()
        ));
    }
}

pub(super) fn weapons_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(&mut w.holsters.enabled, "Holsters")
        .on_hover_text("Draw and holster weapons with a grip press at the body holsters.");
    ui.add_enabled_ui(w.holsters.enabled, |ui| {
        ui.indent("holsters", |ui| holster_settings(ui, &mut w.holsters));
    });
    ui.checkbox(&mut w.foregrip.enabled, "Foregrip")
        .on_hover_text("Let the off-hand grab a two-handed weapon's foregrip.");
    ui.add_enabled_ui(w.foregrip.enabled, |ui| {
        ui.indent("foregrip", |ui| foregrip_settings(ui, &mut w.foregrip));
    });
    ui.checkbox(
        &mut w.block_fire_during_actions,
        "Hold fire back during reloads and draws",
    );
}

fn holster_settings(ui: &mut Ui, h: &mut HolstersConfig) {
    ui.checkbox(&mut h.stick_click_cycles, "Move stick click cycles weapons")
        .on_hover_text("The fallback for drawing without reaching a holster.");
    ui.checkbox(&mut h.grenade_pouch, "Grenade pouch")
        .on_hover_text("A grab volume for the throw, which is not built yet.");
    ui.add(egui::Slider::new(&mut h.crouch_deadband_m, 0.0..=0.5).text("Crouch deadband (m)"));
    ui.label("Offsets from the landmark, right side (+X right, +Y up, +Z back)");
    volume_settings(ui, "Hip", &mut h.hip);
    volume_settings(ui, "Shoulder", &mut h.shoulder);
    volume_settings(ui, "Chest", &mut h.chest);
    volume_settings(ui, "Pouch", &mut h.pouch);
    if ui.button("Reset holsters").clicked() {
        *h = HolstersConfig {
            enabled: h.enabled,
            ..HolstersConfig::new()
        };
    }
}

fn volume_settings(ui: &mut Ui, label: &str, v: &mut VolumeSpec) {
    vec3_drag(ui, &format!("{label} offset (m)"), &mut v.offset_m, 0.005);
    ui.add(egui::Slider::new(&mut v.radius_m, 0.02..=0.3).text(format!("{label} radius (m)")));
}

fn foregrip_settings(ui: &mut Ui, f: &mut ForegripConfig) {
    ui.add(egui::Slider::new(&mut f.radius_m, 0.02..=0.3).text("Grab radius (m)"));
    ui.add(egui::Slider::new(&mut f.release_distance_m, 0.05..=0.6).text("Release distance (m)"));
    ui.add(egui::Slider::new(&mut f.blend_s, 0.0..=0.5).text("Blend (s)"));
    ui.add(
        egui::Slider::new(&mut f.fallback_distance_m, 0.1..=0.8)
            .text("Fallback foregrip distance (m)"),
    );
    ui.add(egui::Slider::new(&mut f.min_span_m, 0.0..=0.3).text("Shortest span (m)"));
}
