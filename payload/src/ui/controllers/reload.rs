//! The true reload: the reloaded weapon, its magazine, chambers, and split, the round in hand, the
//! belt pushes, the action's work, the counters, and the reload's settings.

use egui::Ui;

use crate::{
    controllers::{
        frame::Hand,
        weapons::reload::{
            ReloadStatus,
            belt::PushStage,
            chamber::{Chamber, WorkStage},
            config::{ActionConfig, BeltConfig, ReloadButton, ReloadConfig, ReloadHaptics},
            insert::InsertStage,
            wells,
        },
    },
    ui::controllers::{
        vec3_drag,
        weapons::{pulse_settings, volume_settings},
    },
};

pub(super) fn reload_readout(ui: &mut Ui, r: &ReloadStatus) {
    let c = &r.counts;
    ui.label(format!(
        "True reload: {} · ejects {} · seats {} · belt reloads {} · actions worked {} · dry \
         fires {} · empty pouch {} · game reloads stopped {}",
        if r.active { "acting" } else { "inactive" },
        c.ejects,
        c.seats,
        c.belt_reloads,
        c.racks,
        c.dry_fires,
        c.empty_pouch,
        c.refused
    ));
    let Some(w) = &r.weapon else {
        return;
    };
    ui.indent("reload", |ui| {
        ui.label(format!(
            "weapon {:#010x} · {} · action {} · well {} · travel {:.3} m",
            w.name_hash,
            w.plan.load.label(),
            w.plan.action.label(),
            wells::lookup(w.name_hash).map_or("at the wrist".to_owned(), |e| format!(
                "{} ({}, graded {})",
                e.weapon,
                e.kind.label(),
                e.grade.label()
            )),
            w.plan.travel_m
        ));
        ui.label(format!(
            "magazine {}/{}{} · reserve {}",
            w.magazine,
            w.capacity,
            if w.out { " (out)" } else { "" },
            if w.unlimited {
                "unlimited".to_owned()
            } else {
                w.reserve.to_string()
            }
        ));
        for hand in Hand::BOTH {
            let i = hand.index();
            let Some(chamber) = w.chambers[i] else {
                continue;
            };
            let split = w.split.map_or(String::new(), |s| {
                let h = s.hands[i];
                format!(
                    " · split magazine {}{}",
                    h.rounds,
                    if h.out { " (out)" } else { "" }
                )
            });
            ui.label(format!(
                "{} gun: chamber {}{split} · belt {} · fire {} · turn {:.0}°/s",
                hand.label(),
                match chamber {
                    Chamber::Chambered => "loaded",
                    Chamber::Empty => "empty",
                },
                match r.belt[i] {
                    PushStage::Outside => "clear",
                    PushStage::Pushing => "pushing",
                    PushStage::Spent => "in the pouch",
                },
                if r.fire_held[i] { "held" } else { "free" },
                r.flick_dps[i]
            ));
        }
        let held = match r.insertion.stage {
            InsertStage::Idle => "nothing in hand".to_owned(),
            InsertStage::Holding { elapsed_s } => format!("holding for {elapsed_s:.1} s"),
            InsertStage::Captured { depth_m, .. } => format!("caught, {depth_m:.3} m in"),
        };
        let hands = r.insert_hands.map_or(String::new(), |(hand, gun)| {
            format!(" ({} hand for the {} gun)", hand.label(), gun.label())
        });
        ui.label(format!(
            "round: {held}{hands}{} · taken {} · seated {} · dropped {}",
            r.insertion
                .distance_m
                .map_or(String::new(), |d| format!(", {d:.2} m from the mouth")),
            r.insertion.taken,
            r.insertion.seated,
            r.insertion.dropped
        ));
        ui.label(match r.work.stage {
            WorkStage::Idle => format!("action: idle · worked {}", r.work.worked),
            WorkStage::Held { pulled, .. } => format!(
                "action: held, {:.0}% back{} · worked {}",
                r.work.progress * 100.0,
                if pulled { ", pulled" } else { "" },
                r.work.worked
            ),
        });
        for event in &r.events {
            ui.label(format!("  {event}"));
        }
    });
}

pub(super) fn reload_settings(ui: &mut Ui, r: &mut ReloadConfig) {
    ui.checkbox(&mut r.enabled, "True reload").on_hover_text(
        "Eject, load from the belt pouch, and chamber by hand. Off, the reload button plays the \
         game's timed reload.",
    );
    ui.add_enabled_ui(r.enabled, |ui| {
        ui.indent("reload", |ui| settings(ui, r));
    });
}

fn settings(ui: &mut Ui, r: &mut ReloadConfig) {
    egui::ComboBox::from_label("Reload button")
        .selected_text(r.button.label())
        .show_ui(ui, |ui| {
            for button in ReloadButton::ALL {
                ui.selectable_value(&mut r.button, button, button.label());
            }
        });
    ui.add(egui::Slider::new(&mut r.tap_s, 0.1..=0.6).text("Tap length (s)"));
    ui.checkbox(&mut r.flick_eject, "Flick the gun to eject");
    ui.add_enabled_ui(r.flick_eject, |ui| {
        ui.add(egui::Slider::new(&mut r.flick_speed_dps, 300.0..=2000.0).text("Flick speed (°/s)"));
        ui.add(egui::Slider::new(&mut r.flick_cooldown_s, 0.1..=2.0).text("Flick cooldown (s)"));
    });
    volume_settings(ui, "Pouch", &mut r.pouch);
    vec3_drag(
        ui,
        "Fallback well (wrist frame, m)",
        &mut r.well_offset_m,
        0.005,
    );
    ui.add(egui::Slider::new(&mut r.held_top_m, 0.0..=0.2).text("Held round's top (m)"));
    ui.add(egui::Slider::new(&mut r.capture_radius_m, 0.01..=0.15).text("Capture radius (m)"));
    ui.add(egui::Slider::new(&mut r.capture_angle_deg, 5.0..=90.0).text("Capture angle (°)"));
    ui.add(egui::Slider::new(&mut r.seat_depth_m, 0.005..=0.1).text("Seat depth (m)"));
    ui.add(egui::Slider::new(&mut r.timeout_s, 1.0..=30.0).text("Round timeout (s)"));
    ui.checkbox(&mut r.show_well, "Ring the well while holding a round");
    belt_settings(ui, &mut r.belt);
    ui.checkbox(&mut r.chamber, "Work the action after running dry");
    ui.checkbox(
        &mut r.manual_action,
        "Work the bolt and the pump after every shot",
    );
    action_settings(ui, &mut r.action);
    ui.checkbox(&mut r.dual_split, "A magazine per dual-wielded pistol");
    haptics_settings(ui, &mut r.haptics);
}

fn belt_settings(ui: &mut Ui, b: &mut BeltConfig) {
    ui.checkbox(&mut b.enabled, "Belt reload");
    ui.add_enabled_ui(b.enabled, |ui| {
        ui.add(egui::Slider::new(&mut b.max_tilt_deg, 5.0..=90.0).text("Belt: largest tilt (°)"));
        ui.add(
            egui::Slider::new(&mut b.min_push_speed_mps, 0.0..=2.0).text("Belt: push speed (m/s)"),
        );
        ui.add(egui::Slider::new(&mut b.depth_m, 0.0..=0.2).text("Belt: push depth (m)"));
        ui.add(egui::Slider::new(&mut b.after_draw_s, 0.0..=3.0).text("Belt: after a draw (s)"));
    });
}

fn action_settings(ui: &mut Ui, a: &mut ActionConfig) {
    ui.add(egui::Slider::new(&mut a.grab_radius_m, 0.02..=0.2).text("Action grab radius (m)"));
    ui.add(egui::Slider::new(&mut a.pull_fraction, 0.3..=1.0).text("Action pull share"));
    ui.add(egui::Slider::new(&mut a.return_fraction, 0.0..=0.8).text("Action return share"));
    ui.add(
        egui::Slider::new(&mut a.release_distance_m, 0.05..=0.4)
            .text("Action release distance (m)"),
    );
    ui.add(egui::Slider::new(&mut a.default_travel_m, 0.01..=0.2).text("Default travel (m)"));
}

fn haptics_settings(ui: &mut Ui, h: &mut ReloadHaptics) {
    pulse_settings(ui, "Eject pulse", &mut h.eject);
    pulse_settings(ui, "Pouch pulse", &mut h.pouch_take);
    pulse_settings(ui, "Empty pouch pulse", &mut h.pouch_empty);
    pulse_settings(ui, "Catch pulse", &mut h.capture);
    pulse_settings(ui, "Seat pulse", &mut h.seat);
    pulse_settings(ui, "Belt seat pulse", &mut h.belt_seat);
    pulse_settings(ui, "Rack pulse", &mut h.rack);
    pulse_settings(ui, "Chambered pulse", &mut h.chambered);
}
