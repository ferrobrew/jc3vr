//! Finger curls: the per-hand readout and the finger settings.

use egui::Ui;

use crate::{
    controllers::{
        fingers::{
            HandFingersStatus,
            config::{FingerSource, FingersConfig, GripPosesConfig},
            curl::Finger,
        },
        frame::Hand,
    },
    ui::controllers::yes_no,
};

pub(super) fn fingers_readout(ui: &mut Ui, status: &[HandFingersStatus; 2], conforming: bool) {
    ui.label(format!(
        "Joints located with the controller-conforming motion range: {}",
        yes_no(conforming)
    ));
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let s = &status[hand.index()];
            ui.strong(hand.label());
            let Some(source) = s.source else {
                ui.label("not curled");
                continue;
            };
            ui.label(format!(
                "source {} · weight {:.2}{}",
                source.label(),
                s.weight,
                if s.holding { " · holding" } else { "" }
            ));
            match s.grip {
                Some(grip) => {
                    ui.label(format!(
                        "holds: {} · {} · {} · share {:.2}{}",
                        grip.source.label(),
                        grip.pose.label(),
                        grip.mode.label(),
                        grip.share,
                        if grip.fitted > 0 {
                            format!(" · {} fitted", grip.fitted)
                        } else {
                            String::new()
                        }
                    ));
                }
                None => {
                    ui.label("holds nothing");
                }
            }
            if s.unresolved {
                ui.label("finger bones unresolved");
            } else if !s.applied {
                ui.label("not written");
            }
            for finger in Finger::ALL {
                let i = finger.index();
                ui.label(format!(
                    "{:<6} curl {:.2} · splay {:+.0}° · hook {:+.2}{}",
                    finger.label(),
                    s.shape.curl[i],
                    s.shape.splay[i].to_degrees(),
                    s.shape.hook[i],
                    if s.driven[i] { "" } else { " · animated" }
                ));
            }
        }
    });
}

pub(super) fn fingers_settings(ui: &mut Ui, f: &mut FingersConfig) {
    ui.checkbox(&mut f.enabled, "Finger curls");
    ui.add_enabled_ui(f.enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Source");
            for source in FingerSource::ALL {
                ui.selectable_value(&mut f.source, source, source.label());
            }
        })
        .response
        .on_hover_text(
            "Auto uses hand tracking for a hand the runtime tracks and the estimate from the \
             inputs otherwise.",
        );
        ui.checkbox(
            &mut f.drive_all_when_holding,
            "Drive every finger of a hand holding a weapon",
        )
        .on_hover_text("Off, only the trigger finger moves and the game's grip wraps the weapon.");
        ui.label("Bend at open and closed (share of the full bend from straight)");
        for finger in Finger::ALL {
            let range = &mut f.curl_ranges[finger.index()];
            ui.horizontal(|ui| {
                ui.label(finger.label());
                ui.add(
                    egui::DragValue::new(&mut range.open)
                        .speed(0.01)
                        .range(-1.0..=2.0)
                        .prefix("open "),
                );
                ui.add(
                    egui::DragValue::new(&mut range.closed)
                        .speed(0.01)
                        .range(-1.0..=2.0)
                        .prefix("closed "),
                );
            });
        }
        joint_degrees(ui, "Finger full bend (°)", &mut f.finger_bend_deg);
        joint_degrees(ui, "Thumb full bend (°)", &mut f.thumb_bend_deg);
        ui.label("Rest offset per joint (°, toward the palm)")
            .on_hover_text(
                "Corrects the straight finger the curls are measured from. Raise a joint whose \
                 open finger bends backward, lower one that stays bent with the hand open.",
            );
        for finger in Finger::ALL {
            rest_offsets(ui, finger.label(), &mut f.rest_offset_deg[finger.index()]);
        }
        ui.add(
            egui::Slider::new(&mut f.thumb_spread_deg, 0.0..=90.0).text("Open thumb spread (°)"),
        )
        .on_hover_text("The open thumb's angle from the fingers toward the thumb side.");
        ui.add(egui::Slider::new(&mut f.splay_scale, 0.0..=2.0).text("Splay share"));
        ui.add(egui::Slider::new(&mut f.max_splay_deg, 0.0..=45.0).text("Largest splay (°)"));
        ui.add(egui::Slider::new(&mut f.smoothing_s, 0.0..=0.3).text("Smoothing (s)"));
        grip_settings(ui, &mut f.grip);
        if ui.button("Reset finger tuning").clicked() {
            *f = FingersConfig {
                enabled: f.enabled,
                source: f.source,
                drive_all_when_holding: f.drive_all_when_holding,
                grip: GripPosesConfig {
                    enabled: f.grip.enabled,
                    weapons: f.grip.weapons,
                    contact_fit: f.grip.contact_fit,
                    ..GripPosesConfig::new()
                },
                ..FingersConfig::new()
            };
        }
    });
}

fn grip_settings(ui: &mut Ui, g: &mut GripPosesConfig) {
    ui.checkbox(&mut g.enabled, "Grip poses")
        .on_hover_text("Wrap the fingers round what the hand holds.");
    ui.add_enabled_ui(g.enabled, |ui| {
        ui.checkbox(&mut g.weapons, "Grip poses on guns")
            .on_hover_text("Off, a gun keeps the game's grip and only the trigger finger moves.");
        ui.checkbox(&mut g.contact_fit, "Close the fingers onto held objects");
        ui.add(egui::Slider::new(&mut g.light_share, 0.0..=1.0).text("Pose share at a light grip"));
        ui.add(
            egui::Slider::new(&mut g.firm_squeeze, 0.05..=1.0).text("Squeeze for the full pose"),
        );
        ui.add(egui::Slider::new(&mut g.blend_s, 0.0..=0.5).text("Pose blend (s)"));
        ui.add(
            egui::Slider::new(&mut g.max_opposition_deg, 0.0..=90.0)
                .text("Thumb opposition at most (°)"),
        );
    });
}

fn rest_offsets(ui: &mut Ui, label: &str, degrees: &mut [f32; 3]) {
    ui.horizontal(|ui| {
        ui.label(label);
        for (value, prefix) in degrees.iter_mut().zip(["base ", "middle ", "tip "]) {
            ui.add(
                egui::DragValue::new(value)
                    .speed(0.5)
                    .range(-90.0..=90.0)
                    .prefix(prefix),
            );
        }
    });
}

fn joint_degrees(ui: &mut Ui, label: &str, degrees: &mut [f32; 3]) {
    ui.horizontal(|ui| {
        ui.label(label);
        for (value, prefix) in degrees.iter_mut().zip(["base ", "middle ", "tip "]) {
            ui.add(
                egui::DragValue::new(value)
                    .speed(1.0)
                    .range(0.0..=150.0)
                    .prefix(prefix),
            );
        }
    });
}
