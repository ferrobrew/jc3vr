//! Per-hand aiming: each hand's ray and hit, the grapple's target, the per-weapon aim points, the
//! gamepad-semantics pin, and the aim settings with the sight tuning tool
//! ([`crate::ui::controllers::sights`]), the one editor of the per-weapon sight table.

use egui::Ui;

use crate::{
    controllers::{
        aim::{AimIdle, AimStatus, HandAim, HandSource, gun::GunCounters},
        config::ControllersConfig,
        frame::Hand,
        gamepad_setting::SettingStatus,
    },
    ui::controllers::{sights, vec3, yes_no},
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
            "Grapple: from {} · point {} · hit {} · valid {} · tetherable {} · assist {}",
            vec3(g.ray.origin),
            vec3(g.point),
            yes_no(g.hit),
            yes_no(g.valid),
            yes_no(g.tetherable),
            g.assist.map_or("none".to_owned(), |a| format!(
                "{:.1}° onto {}",
                a.angle_rad.to_degrees(),
                vec3(a.target)
            ))
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
    ui.add(
        egui::Slider::new(&mut a.grapple_assist_strength, 0.0..=1.0).text("Grapple aim assist"),
    )
    .on_hover_text(
        "How far the grapple's ray turns onto the grapple target nearest it in the cone when the \
         ray is not on an object: one snaps onto it, zero turns the assist off.",
    );
    ui.add(
        egui::Slider::new(&mut a.grapple_assist_cone_deg, 0.0..=30.0)
            .text("Grapple assist cone (°)"),
    );
    egui::CollapsingHeader::new("Sight tuning").show(ui, |ui| sights::sight_tuning(ui, c));
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
        AimIdle::Disabled => "the controllers are off",
        AimIdle::GamepadScheme => "the gamepad scheme is in effect",
        AimIdle::NoHands => "outside gameplay, or no tracked hands",
    }
}
