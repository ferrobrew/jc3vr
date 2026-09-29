//! Per-hand aiming: each hand's ray and hit, the grapple's target, the per-weapon aim points, the
//! gamepad-semantics pin, and the aim settings with the per-weapon sight table and the sight tuning
//! tool ([`crate::ui::controllers::sights`]).

use egui::Ui;

use crate::{
    controllers::{
        aim::{
            self, AimIdle, AimStatus, HandAim, HandSource, gun::GunCounters, sights as aim_sights,
        },
        config::{ControllersConfig, SightOverride, SightTable},
        frame::Hand,
        gamepad_setting::SettingStatus,
    },
    ui::controllers::{sights, vec3, vec3_drag, yes_no},
};

pub(super) fn aim_readout(
    ui: &mut Ui,
    status: &AimStatus,
    counters: GunCounters,
    setting: &SettingStatus,
) {
    if let Some(idle) = status.idle {
        ui.label(format!("Idle: {}", idle_label(idle)));
    }
    ui.label(format!(
        "Aim updates {} · gun points handed over {} · kept the game's point {}",
        status.updates, counters.aimed, counters.kept
    ));
    if let Some(error) = status.raycast_error {
        ui.label(error.to_string());
    }
    ui.label(match status.weapon {
        Some(w) => format!(
            "Weapon {:08x} · guns in hands {} · sight from {}",
            w.hash,
            w.gun_hands
                .iter()
                .map(|h| h.map_or("-", Hand::label))
                .collect::<Vec<_>>()
                .join("/"),
            w.sight.source.label()
        ),
        None => "Weapon: none wielded".to_owned(),
    });
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            ui.strong(hand.label());
            match &status.hands[hand.index()] {
                Some(h) => hand_readout(ui, h),
                None => {
                    ui.label("no ray");
                }
            }
        }
    });
    ui.label(match &status.grapple {
        Some(g) => format!(
            "Grapple: from {} · point {} · hit {} · valid {} · tetherable {}",
            vec3(g.ray.origin),
            vec3(g.point),
            yes_no(g.hit),
            yes_no(g.valid),
            yes_no(g.tetherable)
        ),
        None => "Grapple: on the camera".to_owned(),
    });
    ui.label(format!(
        "Gamepad setting: {} · last seen {} · restores {}",
        if setting.pinned { "pinned" } else { "free" },
        setting.observed.map_or("n/a".to_owned(), |v| v.to_string()),
        setting.saved.map_or("n/a".to_owned(), |v| v.to_string())
    ));
}

pub(super) fn aim_settings(ui: &mut Ui, c: &mut ControllersConfig) {
    let a = &mut c.aim;
    ui.checkbox(&mut a.gun, "Gun aim from the hands")
        .on_hover_text("Each gun aims along its hand's sight line instead of the gaze.");
    ui.checkbox(&mut a.grapple, "Grapple aim from the off-hand")
        .on_hover_text("The grapple targets along the off-hand's ray, with the game's targeting.");
    ui.add(egui::Slider::new(&mut a.assist_strength, 0.0..=1.0).text("Aim assist"))
        .on_hover_text("How far each gun hand's aim moves toward the game's best target along it.");
    ui.add_enabled_ui(a.assist_strength > 0.0, |ui| {
        ui.add(
            egui::Slider::new(&mut a.assist_max_distance_m, 5.0..=500.0)
                .logarithmic(true)
                .text("Aim assist reach (m)"),
        );
        ui.add(
            egui::Slider::new(&mut a.assist_occlusion_tolerance_m, 0.0..=3.0)
                .text("Aim assist line-of-sight tolerance (m)"),
        );
    });
    ui.add(
        egui::Slider::new(&mut a.max_distance_m, 10.0..=2000.0)
            .logarithmic(true)
            .text("Ray length (m)"),
    );
    ui.add(
        egui::Slider::new(&mut a.min_target_distance_m, 0.5..=10.0)
            .text("Nearest gun aim point (m)"),
    );
    vec3_drag(
        ui,
        "Grapple origin (wrist frame, m)",
        &mut a.grapple_origin_m,
        0.005,
    );
    ui.checkbox(&mut c.gamepad_semantics, "Gamepad semantics").on_hover_text(
        "Hold the game's active-input-device setting at the gamepad under the controller scheme: \
         aim assist, tap-style melee, and gamepad prompts.",
    );
    egui::CollapsingHeader::new("Sight tuning").show(ui, |ui| sights::sight_tuning(ui, c));
    egui::CollapsingHeader::new("Sight corrections")
        .show(ui, |ui| sight_table(ui, &mut c.aim.sights));
}

fn sight_table(ui: &mut Ui, table: &mut SightTable) {
    let held = aim::status().weapon.map(|w| w.hash);
    ui.horizontal(|ui| {
        let label = held.map_or("No weapon wielded".to_owned(), |hash| {
            format!("Wielded weapon {hash:08x}")
        });
        ui.label(label);
        if let Some(hash) = held
            && ui.button("Add a correction").clicked()
            && !table.set(
                aim_sights::resolve(table, hash)
                    .sight
                    .unwrap_or(SightOverride::identity(hash)),
            )
        {
            tracing::warn!(target: "controllers", "aim: the sight table is full");
        }
    });
    let mut remove = None;
    for entry in table.entries.iter_mut().flatten() {
        ui.separator();
        ui.horizontal(|ui| {
            let marker = if held == Some(entry.weapon_hash) {
                " (wielded)"
            } else {
                ""
            };
            ui.label(format!("{:08x}{marker}", entry.weapon_hash));
            if ui.button("Remove").clicked() {
                remove = Some(entry.weapon_hash);
            }
        });
        vec3_drag(
            ui,
            "Offset (fire-point frame, m)",
            &mut entry.offset_m,
            0.001,
        );
        ui.horizontal(|ui| {
            ui.label("Pitch and yaw (°)");
            ui.add(egui::DragValue::new(&mut entry.angles_deg.x).speed(0.05));
            ui.add(egui::DragValue::new(&mut entry.angles_deg.y).speed(0.05));
        });
    }
    if let Some(hash) = remove {
        table.remove(hash);
    }
}

fn hand_readout(ui: &mut Ui, h: &HandAim) {
    ui.label(match h.source {
        HandSource::AimPose => "ray: controller aim".to_owned(),
        HandSource::Gun { gun } => format!("ray: gun {gun} sight"),
    });
    ui.label(format!("from {}", vec3(h.ray.origin)));
    ui.label(match &h.hit {
        Some(hit) => format!(
            "hit at {:.1} m{}",
            hit.distance,
            if hit.object.is_some() {
                " (object)"
            } else {
                ""
            }
        ),
        None => "no hit".to_owned(),
    });
    ui.label(format!("aim point {}", vec3(h.target)));
}

fn idle_label(idle: AimIdle) -> &'static str {
    match idle {
        AimIdle::Disabled => "the gun and grapple aim are off",
        AimIdle::GamepadScheme => "the gamepad scheme is in effect",
        AimIdle::NoHands => "outside gameplay, or no tracked hands",
    }
}
