//! Physical hands: each palm's contacts and push, what each hand reaches and holds, the last
//! strike, and the palm and pickup settings.

use egui::Ui;

use crate::{
    controllers::{
        frame::Hand,
        physical_hands::{
            PhysicalHandsConfig, PhysicalHandsStatus,
            config::{PickupConfig, StrikeTuning},
            grasp::TargetKind,
        },
    },
    ui::controllers::{vec3_drag, weapons::pulse_settings, yes_no},
};

pub(super) fn physical_hands_readout(ui: &mut Ui, status: &PhysicalHandsStatus) {
    match status.idle {
        Some(idle) => ui.label(format!("Idle: {}", idle.label())),
        None => ui.label(format!(
            "Acting · {} casts · push ({:+.2}, {:+.2}) m/s",
            status.casts, status.push.x, status.push.y
        )),
    };
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let s = &status.hands[hand.index()];
            ui.strong(hand.label());
            if !s.acting {
                ui.label("not acting (holding something, yielded, or untracked)");
                continue;
            }
            if let Some(held) = s.held {
                ui.label(format!(
                    "holding {:.1} kg{}{} · {:.0} mm off",
                    held.mass_kg,
                    if held.shared { " with both hands" } else { "" },
                    if held.lifted { "" } else { " (too heavy)" },
                    held.lag_m * 1000.0
                ));
                continue;
            }
            ui.label(match s.reach {
                None => "reaches nothing".to_owned(),
                Some(reach) => match reach.kind {
                    TargetKind::Weapon => {
                        format!("reaches a weapon {:.0} mm away", reach.distance * 1000.0)
                    }
                    TargetKind::Prop { mass_kg } => format!(
                        "reaches a {mass_kg:.1} kg object {:.0} mm away",
                        reach.distance * 1000.0
                    ),
                },
            });
            ui.label(match s.press {
                None => "in the open".to_owned(),
                Some((depth, object)) => format!(
                    "pressed {:.0} mm into {}",
                    depth * 1000.0,
                    if object { "an object" } else { "the world" }
                ),
            });
            ui.label(format!(
                "speed {:.1} m/s · {} · gate armed {}",
                s.speed,
                s.kind.map_or("no curl", |k| k.label()),
                yes_no(s.armed)
            ));
            ui.label(format!("push ({:+.2}, {:+.2}) m/s", s.push.x, s.push.y));
        }
    });
    ui.label(format!(
        "Strikes {} · object pushes {} · off-thread ticks {}",
        status.strikes, status.object_pushes, status.off_thread
    ));
    let p = &status.pickup;
    ui.label(format!(
        "Pickups: {} holds · {} weapons · {} let go · {} lost{}",
        p.holds,
        p.weapons,
        p.releases,
        p.lost,
        p.last_throw_mps
            .map_or(String::new(), |v| format!(" · last throw {v:.1} m/s"))
    ));
    if let Some(error) = &p.last_error {
        ui.label(format!("Last pickup error: {error}"));
    }
    if let Some(strike) = status.last_strike {
        ui.label(format!(
            "Last strike: {} {} at {:.1} m/s, strength {:.2}, damage {} (took {}), impulse {:.1}{}",
            strike.hand.label(),
            strike.plan.kind.label(),
            strike.impact_mps,
            strike.plan.strength,
            strike.plan.damage,
            strike.dealt,
            strike.plan.impulse,
            if strike.plan.stumble { ", stumble" } else { "" }
        ));
    }
    if let Some(error) = &status.last_error {
        ui.label(format!("Last error: {error}"));
    }
}

pub(super) fn physical_hands_settings(ui: &mut Ui, p: &mut PhysicalHandsConfig) {
    ui.checkbox(&mut p.enabled, "Physical palms");
    ui.add_enabled_ui(p.enabled, |ui| {
        vec3_drag(ui, "Palm half size (m)", &mut p.palm_half_extents_m, 0.002);
        vec3_drag(
            ui,
            "Palm centre (wrist frame, m)",
            &mut p.palm_offset_m,
            0.002,
        );
        ui.checkbox(&mut p.show_boxes, "Draw the palm boxes");
        ui.checkbox(&mut p.clamp_hand, "Hold the hand at surfaces");
        ui.checkbox(&mut p.push, "Push the body off surfaces");
        ui.add(egui::Slider::new(&mut p.push_deadband_m, 0.0..=0.1).text("Push deadband (m)"));
        ui.add(
            egui::Slider::new(&mut p.push_stiffness_per_s, 0.0..=29.0).text("Push stiffness (1/s)"),
        );
        ui.add(egui::Slider::new(&mut p.max_push_speed_mps, 0.0..=6.0).text("Fastest push (m/s)"));
        ui.add(
            egui::Slider::new(&mut p.min_push_horizontal, 0.0..=1.0)
                .text("Least horizontal normal"),
        );
        ui.separator();
        ui.checkbox(&mut p.strikes, "Strike characters");
        ui.checkbox(
            &mut p.replace_melee_gesture,
            "Replace the melee gesture for acting palms",
        );
        ui.add(
            egui::Slider::new(&mut p.min_strike_speed_mps, 0.5..=8.0).text("Slowest strike (m/s)"),
        );
        ui.add(
            egui::Slider::new(&mut p.full_strike_speed_mps, 1.0..=12.0)
                .text("Full-strength strike (m/s)"),
        );
        ui.add(egui::Slider::new(&mut p.rearm_speed_mps, 0.1..=4.0).text("Rearm speed (m/s)"));
        ui.add(egui::Slider::new(&mut p.strike_cooldown_s, 0.0..=2.0).text("Strike cooldown (s)"));
        ui.add(egui::Slider::new(&mut p.fist_curl, 0.0..=1.0).text("Fist from curl"));
        strike_settings(ui, "Punch", &mut p.punch);
        strike_settings(ui, "Slap", &mut p.slap);
        ui.separator();
        ui.checkbox(&mut p.object_impulses, "Push objects");
        ui.add(
            egui::Slider::new(&mut p.min_object_speed_mps, 0.0..=6.0)
                .text("Slowest object push (m/s)"),
        );
        ui.add(
            egui::Slider::new(&mut p.object_impulse, 0.0..=4.0).text("Object impulse (× melee)"),
        );
        ui.separator();
        ui.checkbox(&mut p.haptics, "Contact pulses");
        pulse_settings(ui, "Contact pulse", &mut p.contact_pulse);
        pulse_settings(ui, "Strike pulse", &mut p.strike_pulse);
        ui.separator();
        pickup_settings(ui, &mut p.pickup);
    });
}

fn pickup_settings(ui: &mut Ui, p: &mut PickupConfig) {
    ui.checkbox(&mut p.enabled, "Pick up objects with the grip");
    ui.add_enabled_ui(p.enabled, |ui| {
        ui.checkbox(&mut p.weapons, "Grab weapons to pick them up");
        ui.checkbox(&mut p.show_targets, "Mark what each hand reaches");
        ui.add(egui::Slider::new(&mut p.reach_m, 0.0..=0.4).text("Reach (m)"));
        ui.add(egui::Slider::new(&mut p.one_hand_mass_kg, 0.0..=200.0).text("One hand lifts (kg)"));
        ui.add(egui::Slider::new(&mut p.two_hand_mass_kg, 0.0..=400.0).text("Two hands lift (kg)"));
        ui.add(
            egui::Slider::new(&mut p.stiffness_per_s, 0.0..=60.0).text("Follow stiffness (1/s)"),
        );
        ui.add(egui::Slider::new(&mut p.max_speed_mps, 0.0..=60.0).text("Fastest follow (m/s)"));
        ui.add(
            egui::Slider::new(&mut p.angular_stiffness_per_s, 0.0..=60.0)
                .text("Turn stiffness (1/s)"),
        );
        ui.add(
            egui::Slider::new(&mut p.max_angular_speed_rps, 0.0..=60.0)
                .text("Fastest turn (rad/s)"),
        );
        ui.add(
            egui::Slider::new(&mut p.heavy_response_s, 0.0..=1.0)
                .text("Response at the one-hand limit (s)"),
        );
        ui.add(egui::Slider::new(&mut p.break_distance_m, 0.05..=3.0).text("Let go past (m)"));
        ui.add(egui::Slider::new(&mut p.throw_scale, 0.0..=4.0).text("Throw (× hand)"));
        ui.add(
            egui::Slider::new(&mut p.max_throw_speed_mps, 0.0..=80.0).text("Fastest throw (m/s)"),
        );
        pulse_settings(ui, "Grab pulse", &mut p.grab_pulse);
        pulse_settings(ui, "Release pulse", &mut p.release_pulse);
    });
}

fn strike_settings(ui: &mut Ui, label: &str, s: &mut StrikeTuning) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::Slider::new(&mut s.damage, 0.0..=3.0).text("damage (× melee)"));
        ui.add(egui::Slider::new(&mut s.impulse, 0.0..=4.0).text("impulse (× melee)"));
        ui.add(egui::Slider::new(&mut s.stumble_strength, 0.0..=1.01).text("stumble from"));
    });
}
