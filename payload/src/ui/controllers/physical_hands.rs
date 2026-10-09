//! Physical hands: each palm's contacts and push, what each hand reaches and holds, and the palm
//! and pickup settings.

use egui::Ui;

use crate::{
    controllers::{
        frame::Hand,
        physical_hands::{
            PhysicalHandsConfig, PhysicalHandsStatus,
            characters::CharactersConfig,
            config::{PickupConfig, ShoveConfig},
            doors::{self, DoorsConfig},
            grasp::TargetKind,
            sounds::{self, ImpactSoundsConfig},
            vault::{self, VaultConfig},
        },
    },
    ui::controllers::vec3_drag,
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
                    "{} {:.1} kg{}{} · {:.0} mm off",
                    if held.joined { "joined to" } else { "holding" },
                    held.mass_kg,
                    if held.shared { " with both hands" } else { "" },
                    if held.liftable {
                        ""
                    } else {
                        " (too heavy to lift)"
                    },
                    held.lag_m * 1000.0
                ));
                ui.label(format!(
                    "pull {:.0}% · twist {:.0}% · push ({:+.2}, {:+.2}) m/s",
                    held.load * 100.0,
                    held.twist * 100.0,
                    s.push.x,
                    s.push.y
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
                    TargetKind::Character(target) => format!(
                        "reaches a {} character {:.0} mm away",
                        if target.seated.is_some() {
                            "seated"
                        } else {
                            "standing"
                        },
                        reach.distance * 1000.0
                    ),
                    TargetKind::Articulated { mass_kg } => format!(
                        "reaches a {mass_kg:.0} kg articulated object {:.0} mm away",
                        reach.distance * 1000.0
                    ),
                    TargetKind::Door(door) => format!(
                        "reaches the {} door ({}) {:.0} mm away",
                        doors::slot_label(door.slot),
                        if door.latched { "latched" } else { "open" },
                        reach.distance * 1000.0
                    ),
                },
            });
            if let Some(Some(target)) = s.reach.map(|r| match r.kind {
                TargetKind::Character(target) => Some(target),
                _ => None,
            }) && let Some(seated) = target.seated
            {
                ui.label(format!(
                    "the character sits behind the {} door, open {:.0}°",
                    doors::slot_label(seated.door),
                    seated.open_deg
                ));
            }
            if s.latched {
                ui.label("grips a latched door");
            }
            ui.label(match s.press {
                None => "in the open".to_owned(),
                Some((depth, object)) => format!(
                    "pressed {:.0} mm into {}",
                    depth * 1000.0,
                    if object { "an object" } else { "the world" }
                ),
            });
            ui.label(format!("speed {:.1} m/s", s.speed));
            ui.label(format!("push ({:+.2}, {:+.2}) m/s", s.push.x, s.push.y));
            if let Some(shove) = s.shove {
                ui.label(format!(
                    "shoving {:.2} N·s · load {:.0}% · resists {:.0}%",
                    shove.impulse.length(),
                    shove.load * 100.0,
                    shove.resistance * 100.0
                ));
            }
        }
    });
    ui.label(format!(
        "Object knocks {} · shove ticks {} · off-thread ticks {}",
        status.object_pushes, status.shoves, status.off_thread
    ));
    let p = &status.pickup;
    ui.label(format!(
        "Pickups: {} holds ({} articulated) · {} weapons · {} let go · {} lost{}",
        p.holds,
        p.articulated_grips,
        p.weapons,
        p.releases,
        p.lost,
        p.last_throw_mps
            .map_or(String::new(), |v| format!(" · last throw {v:.1} m/s"))
    ));
    ui.label(format!(
        "Doors: {} latched grips · {} unlatched{}",
        p.door_grips,
        p.unlatches,
        p.last_door.map_or(String::new(), |slot| format!(
            " · last {}",
            doors::slot_label(slot)
        ))
    ));
    ui.label(format!(
        "Characters: {} grips · {} yanked · {} ragdolls taken · {} pulled out of a seat",
        p.character_grips, p.yanks, p.regrips, p.hijacks
    ));
    let v = vault::status();
    ui.label(format!(
        "Push up: {} pushes{}{}",
        v.pushes,
        if v.lifting {
            format!(" · lifting {:.2} m/s", v.lift_mps)
        } else {
            String::new()
        },
        v.last_rise_m
            .map_or(String::new(), |r| format!(" · last rise {r:.2} m"))
    ));
    ui.label(format!(
        "Push up refused off the ground or moving down: {} · at the feet: {}",
        v.refusals, v.at_the_feet
    ));
    if let Some(error) = &p.last_error {
        ui.label(format!("Last pickup error: {error}"));
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
        ui.add(
            egui::Slider::new(&mut p.full_hit_speed_mps, 1.0..=12.0)
                .text("Full-strength hit (m/s)"),
        );
        ui.separator();
        shove_settings(ui, &mut p.shove);
        ui.checkbox(&mut p.object_impulses, "Knock objects on fast hits");
        ui.add(
            egui::Slider::new(&mut p.min_object_speed_mps, 0.0..=6.0)
                .text("Slowest object push (m/s)"),
        );
        ui.add(
            egui::Slider::new(&mut p.object_impulse, 0.0..=4.0).text("Object impulse (× melee)"),
        );
        ui.separator();
        sounds_settings(ui, &mut p.sounds);
        ui.separator();
        pickup_settings(ui, &mut p.pickup);
        ui.separator();
        doors_settings(ui, &mut p.doors);
        ui.separator();
        characters_settings(ui, &mut p.characters);
        ui.separator();
        vault_settings(ui, &mut p.vault);
    });
}

fn vault_settings(ui: &mut Ui, v: &mut VaultConfig) {
    ui.checkbox(
        &mut v.enabled,
        "Push the body up with the hands pressed down",
    );
    ui.add_enabled_ui(v.enabled, |ui| {
        ui.add(egui::Slider::new(&mut v.deadband_m, 0.0..=0.2).text("Press left alone (m)"));
        ui.add(egui::Slider::new(&mut v.stiffness_per_s, 0.0..=40.0).text("Lift stiffness (1/s)"));
        ui.add(egui::Slider::new(&mut v.max_lift_mps, 0.0..=8.0).text("Fastest lift (m/s)"));
        ui.add(egui::Slider::new(&mut v.min_vertical, 0.0..=1.0).text("Least downward press"));
        ui.add(egui::Slider::new(&mut v.max_rise_m, 0.0..=3.0).text("Highest rise (m)"));
        ui.add(
            egui::Slider::new(&mut v.min_height_m, 0.0..=1.5)
                .text("Lowest surface above the feet (m)"),
        );
        ui.add(egui::Slider::new(&mut v.hold_s, 0.0..=0.6).text("Press held before lifting (s)"));
        ui.add(
            egui::Slider::new(&mut v.fall_tolerance_mps, 0.0..=3.0)
                .text("Fastest downward motion that still lifts (m/s)"),
        );
    });
}

fn characters_settings(ui: &mut Ui, c: &mut CharactersConfig) {
    ui.checkbox(&mut c.grab, "Take hold of characters on foot");
    ui.add_enabled_ui(c.grab, |ui| {
        ui.add(egui::Slider::new(&mut c.yank_strength, 0.0..=3.0).text("Yank (× knockdown)"));
        ui.add(egui::Slider::new(&mut c.regrip_s, 0.0..=5.0).text("Look for the ragdoll (s)"));
        ui.add(
            egui::Slider::new(&mut c.shove_min_speed_mps, 0.0..=8.0)
                .text("Slowest shove on letting go (m/s)"),
        );
    });
    ui.checkbox(&mut c.hijack, "Pull occupants out through open doors");
    ui.add(egui::Slider::new(&mut c.yank_m, 0.02..=0.6).text("Yank past (m)"));
    ui.add_enabled_ui(c.hijack, |ui| {
        ui.add(
            egui::Slider::new(&mut c.hijack_min_open_deg, 0.0..=90.0)
                .text("Door open at least (°)"),
        );
        ui.add(
            egui::Slider::new(&mut c.hijack_door_reach_m, 0.0..=3.0).text("Open door within (m)"),
        );
    });
}

fn doors_settings(ui: &mut Ui, d: &mut DoorsConfig) {
    ui.checkbox(&mut d.enabled, "Open vehicle doors by hand");
    ui.add_enabled_ui(d.enabled, |ui| {
        ui.add(egui::Slider::new(&mut d.reach_m, 0.0..=0.3).text("Door reach (m)"));
        ui.add(
            egui::Slider::new(&mut d.handle_share, 0.0..=1.0).text("Handle area from the hinge"),
        );
        ui.add(egui::Slider::new(&mut d.unlatch_pull_m, 0.0..=0.3).text("Pull to unlatch (m)"));
    });
}

fn pickup_settings(ui: &mut Ui, p: &mut PickupConfig) {
    ui.checkbox(&mut p.enabled, "Pick up objects with the grip");
    ui.add_enabled_ui(p.enabled, |ui| {
        ui.checkbox(&mut p.weapons, "Grab weapons to pick them up");
        ui.add(egui::Slider::new(&mut p.reach_m, 0.0..=0.4).text("Reach (m)"));
        ui.add(egui::Slider::new(&mut p.max_mass_kg, 0.0..=5000.0).text("Heaviest held (kg)"));
        ui.checkbox(
            &mut p.articulated,
            "Take hinged and jointed objects whatever their weight",
        );
        ui.add(
            egui::Slider::new(&mut p.max_articulated_mass_kg, 0.0..=50_000.0)
                .text("Heaviest hinged object (kg)"),
        );
        ui.add(egui::Slider::new(&mut p.hand_force_n, 0.0..=2000.0).text("Hand pull (N)"));
        ui.add(egui::Slider::new(&mut p.hand_torque_nm, 0.0..=200.0).text("Hand twist (N·m)"));
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
            egui::Slider::new(&mut p.two_hand_angular_share, 0.0..=1.0)
                .text("Twist share with two hands"),
        );
        ui.add(egui::Slider::new(&mut p.gravity_mps2, 0.0..=20.0).text("Gravity (m/s²)"));
        ui.checkbox(&mut p.resist, "Push the body off what the hand cannot move");
        ui.add(
            egui::Slider::new(&mut p.keep_out_radius_m, 0.0..=1.0)
                .text("Keep held objects out of the body (m)"),
        );
        ui.add(egui::Slider::new(&mut p.break_distance_m, 0.05..=3.0).text("Let go past (m)"));
        ui.add(egui::Slider::new(&mut p.throw_scale, 0.0..=4.0).text("Throw (× hand)"));
        ui.add(
            egui::Slider::new(&mut p.max_throw_speed_mps, 0.0..=80.0).text("Fastest throw (m/s)"),
        );
    });
}

fn shove_settings(ui: &mut Ui, s: &mut ShoveConfig) {
    ui.checkbox(&mut s.enabled, "Shove objects the palm presses into");
    ui.add_enabled_ui(s.enabled, |ui| {
        ui.add(egui::Slider::new(&mut s.stiffness_per_s, 0.0..=40.0).text("Shove stiffness (1/s)"));
        ui.add(egui::Slider::new(&mut s.deadband_m, 0.0..=0.05).text("Shove deadband (m)"));
        ui.add(egui::Slider::new(&mut s.max_speed_mps, 0.0..=10.0).text("Fastest shove (m/s)"));
        ui.add(egui::Slider::new(&mut s.max_force_n, 0.0..=3000.0).text("Palm push (N)"));
        ui.add(egui::Slider::new(&mut s.friction, 0.0..=2.0).text("Palm friction"));
        ui.add(egui::Slider::new(&mut s.give_smoothing_s, 0.0..=0.5).text("Give smoothing (s)"));
    });
}

fn sounds_settings(ui: &mut Ui, s: &mut ImpactSoundsConfig) {
    let status = sounds::status();
    ui.checkbox(&mut s.enabled, "Impact sounds")
        .on_hover_text("The game's melee impact sound where a hand meets something hard enough.");
    ui.add(
        egui::Slider::new(&mut s.min_touch_speed_mps, 0.0..=8.0)
            .text("Slowest sounding touch (m/s)"),
    );
    ui.add(egui::Slider::new(&mut s.min_strike_share, 0.0..=1.0).text("Weakest sounding strike"));
    ui.add(egui::Slider::new(&mut s.hand_cooldown_s, 0.0..=2.0).text("Hand cooldown (s)"));
    ui.add(egui::Slider::new(&mut s.min_interval_s, 0.0..=1.0).text("Shared interval (s)"));
    ui.label(format!(
        "Sounds: {} played · {} held back · {} silent · last {}",
        status.played,
        status.limited,
        status.silent,
        status
            .last
            .map_or("-".to_owned(), |(material, kind)| format!(
                "{} on material {material}",
                kind.label()
            ))
    ));
}
