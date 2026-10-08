//! Hand bodies: each hand body's phase and drive, the push they give the player, and their
//! settings.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    physical_hands::physics::{BodyPhase, HandBodiesConfig, HandBodiesStatus, HandHitsConfig},
};

pub(super) fn hand_bodies_readout(ui: &mut Ui, status: &HandBodiesStatus) {
    ui.label(format!(
        "Callback {} · {} steps · player group {} · push ({:+.2}, {:+.2}) m/s",
        if status.registered {
            "registered"
        } else {
            "not registered"
        },
        status.steps,
        status
            .system_group
            .map_or_else(|| "none".to_owned(), |g| g.to_string()),
        status.push.x,
        status.push.y
    ));
    ui.label(format!(
        "{} created · {} destroyed · {} contacts ({} dropped) · {} blows",
        status.created, status.destroyed, status.contacts, status.dropped_contacts, status.blows
    ));
    if let Some(blow) = status.last_blow {
        ui.label(format!(
            "Last blow: {} hand · {:.1} N·s · {} · {:.0}% of a knockdown · {}",
            blow.hand.label(),
            blow.impulse_ns,
            if blow.hostile {
                "hostile"
            } else {
                "not hostile"
            },
            blow.strength * 100.0,
            match blow.damage {
                Some(damage) => format!("{damage} damage, {} dealt", blow.dealt),
                None => "no damage".to_owned(),
            }
        ));
    }
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let s = &status.hands[hand.index()];
            ui.strong(hand.label());
            ui.label(match (s.body_id, s.filter_info) {
                (Some(id), Some(filter)) => {
                    format!("{} · body {id} · filter {filter:#010x}", s.phase.label())
                }
                (Some(id), None) => format!("{} · body {id}", s.phase.label()),
                _ => s.phase.label().to_owned(),
            });
            if s.phase == BodyPhase::Acting {
                ui.label(format!(
                    "lag {:.0} mm · {:.2} m/s · pull {:.0}% · twist {:.0}%",
                    s.lag_m * 1000.0,
                    s.speed_mps,
                    s.force_share * 100.0,
                    s.torque_share * 100.0
                ));
            }
            ui.label(match s.joint {
                Some(constraint) => {
                    format!("{} resets · joined (constraint {constraint})", s.resets)
                }
                None => format!("{} resets", s.resets),
            });
        }
    });
    if let Some(error) = &status.last_error {
        ui.label(format!("Last error: {error}"));
    }
}

pub(super) fn hand_bodies_settings(ui: &mut Ui, b: &mut HandBodiesConfig) {
    ui.checkbox(&mut b.enabled, "Hand bodies (in place of the palm probes)");
    ui.add_enabled_ui(b.enabled, |ui| {
        ui.add(egui::Slider::new(&mut b.strength, 0.0..=3.0).text("Strength (×)"));
        ui.add(egui::Slider::new(&mut b.mass_kg, 1.0..=50.0).text("Hand mass (kg)"));
        ui.add(egui::Slider::new(&mut b.max_force_n, 0.0..=20_000.0).text("Strongest pull (N)"));
        ui.add(egui::Slider::new(&mut b.max_torque_nm, 0.0..=2000.0).text("Strongest twist (N·m)"));
        ui.add(egui::Slider::new(&mut b.linear_gain, 0.0..=1.0).text("Position gain"));
        ui.add(egui::Slider::new(&mut b.angular_gain, 0.0..=1.0).text("Rotation gain"));
        ui.add(egui::Slider::new(&mut b.max_speed_mps, 0.0..=50.0).text("Fastest hand (m/s)"));
        ui.add(
            egui::Slider::new(&mut b.max_angular_speed_rps, 0.0..=100.0)
                .text("Fastest turn (rad/s)"),
        );
        ui.add(egui::Slider::new(&mut b.radius_m, 0.01..=0.1).text("Capsule radius (m)"));
        ui.add(egui::Slider::new(&mut b.half_length_m, 0.0..=0.1).text("Capsule half length (m)"));
        ui.add(egui::Slider::new(&mut b.break_distance_m, 0.1..=2.0).text("Break distance (m)"));
        ui.checkbox(&mut b.render_at_body, "Draw the hands at their bodies");
        ui.checkbox(
            &mut b.joint_grabs,
            "Hold grabbed objects with a joint to the hand's body (experimental)",
        );
        ui.separator();
        ui.checkbox(&mut b.push, "Push the body off what the hands press");
        ui.add(egui::Slider::new(&mut b.body_mass_kg, 10.0..=300.0).text("Body mass (kg)"));
        ui.add(egui::Slider::new(&mut b.push_deadband_m, 0.0..=0.1).text("Push deadband (m)"));
        ui.add(egui::Slider::new(&mut b.max_push_speed_mps, 0.0..=6.0).text("Fastest push (m/s)"));
        ui.add(
            egui::Slider::new(&mut b.min_push_horizontal, 0.0..=1.0).text("Least horizontal lag"),
        );
        ui.add(egui::Slider::new(&mut b.push_smoothing_s, 0.0..=0.5).text("Push smoothing (s)"));
        ui.checkbox(&mut b.pressure_haptics, "Pressure pulses");
        ui.add(
            egui::Slider::new(&mut b.pressure_full_lag_m, 0.01..=0.5)
                .text("Lag of the strongest pulse (m)"),
        );
        ui.separator();
        hits_settings(ui, &mut b.hits);
    });
}

fn hits_settings(ui: &mut Ui, h: &mut HandHitsConfig) {
    ui.checkbox(&mut h.enabled, "Listen for the hands' contacts");
    ui.add_enabled_ui(h.enabled, |ui| {
        ui.checkbox(&mut h.haptics, "Hit pulses");
        ui.add(
            egui::Slider::new(&mut h.full_pulse_impulse_ns, 1.0..=200.0)
                .text("Impulse of the strongest pulse (N·s)"),
        );
        ui.checkbox(&mut h.reactions, "Characters react to blows");
        ui.add(egui::Slider::new(&mut h.min_impulse_ns, 0.0..=50.0).text("Weakest blow (N·s)"));
        ui.add(
            egui::Slider::new(&mut h.knockdown_impulse_ns, 1.0..=300.0)
                .text("Knockdown blow (N·s)"),
        );
        ui.add(egui::Slider::new(&mut h.cooldown_s, 0.0..=2.0).text("One blow per character (s)"));
        ui.add(
            egui::Slider::new(&mut h.min_approach_speed_mps, 0.0..=6.0)
                .text("Slowest swing that strikes (m/s)"),
        );
        ui.add(egui::Slider::new(&mut h.blow_mass_kg, 1.0..=30.0).text("Arm mass of a blow (kg)"));
        ui.add(egui::Slider::new(&mut h.follow_through_s, 0.0..=0.5).text("Follow-through (s)"));
        ui.add(
            egui::Slider::new(&mut h.follow_through_gain, 0.0..=1.0)
                .text("Drive kept in a follow-through"),
        );
        ui.checkbox(&mut h.damage, "Hard blows deal damage");
        ui.add(
            egui::Slider::new(&mut h.damage_cooldown_s, 0.0..=3.0)
                .text("One damaging blow per character (s)"),
        );
        ui.add(
            egui::Slider::new(&mut h.damage_impulse_ns, 0.0..=300.0)
                .text("Weakest damaging blow (N·s)"),
        );
        ui.add(
            egui::Slider::new(&mut h.damage_scale, 0.0..=5.0)
                .text("Knockdown damage (× melee damage)"),
        );
    });
}
