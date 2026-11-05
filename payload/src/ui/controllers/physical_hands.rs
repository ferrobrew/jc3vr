//! Physical hands: each palm's contacts and push, what each hand reaches and holds, and the palm
//! and pickup settings.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        PhysicalHandsConfig, PhysicalHandsStatus, doors, grasp::TargetKind, human_hull, impacts,
        pickup::diagnostics, sounds::SoundsStatus, vault::VaultStatus,
    },
};

pub(super) fn physical_hands_readout(ui: &mut Ui, status: &PhysicalHandsStatus, v: &VaultStatus) {
    ui.label(format!(
        "Human-hull adapter: {:?}; clearance: {:?}.",
        status.human_hull.adapter, status.human_hull.clearance,
    ));
    ui.collapsing("Grip acquisition evidence", |ui| {
        let evidence = diagnostics::status();
        ui.small("A tracking recording and the lookback (Tools > Tracking capture) record each acquisition stage as an event.");
        ui.small("Retained observations identify acquisition stages. Missing provider data is not a scan miss. Drive commands and write results are not post-step motion or proof of an overwrite.");
        for hand in Hand::BOTH {
            let observed = &evidence.hands[hand.index()];
            ui.collapsing(format!("{} hand", hand.label()), |ui| {
                ui.label(format!("Evicted request records: {}.", observed.evicted));
                ui.collapsing("Latest input and scan", |ui| {
                    ui.monospace(format!("{:#?}", observed.latest));
                });
                ui.collapsing("Last grip press", |ui| {
                    ui.monospace(format!("{:#?}", observed.last_press));
                });
                for request in observed.requests.iter().flatten() {
                    ui.collapsing(format!(
                        "Request {}: sample {}, grip episode {}",
                        request.key.request, request.key.sequence, request.key.episode,
                    ), |ui| {
                        ui.monospace(format!("{request:#?}"));
                    });
                }
            });
        }
    });
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
            match s.reach {
                None => {
                    ui.label("reaches nothing");
                }
                Some(reach) => match reach.kind {
                    TargetKind::Weapon => {
                        ui.label(format!(
                            "reaches a weapon {:.0} mm away",
                            reach.distance * 1000.0
                        ));
                    }
                    TargetKind::Prop { mass_kg } => {
                        ui.label(format!(
                            "reaches a {mass_kg:.1} kg object {:.0} mm away",
                            reach.distance * 1000.0
                        ));
                    }
                    TargetKind::Articulated { mass_kg } => {
                        ui.label(format!(
                            "reaches a {mass_kg:.0} kg articulated object {:.0} mm away",
                            reach.distance * 1000.0
                        ));
                    }
                    TargetKind::Character(_) | TargetKind::Door(_) => {}
                },
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
    if let Some(error) = &p.last_error {
        ui.label(format!("Last pickup error: {error}"));
    }
    let impacts = impacts::status();
    ui.label(format!(
        "Held impacts: {} bodies, {} contacts, {} feedback, {} suppressed, {} dropped",
        impacts.listening, impacts.contacts, impacts.feedback, impacts.suppressed, impacts.dropped
    ));
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
    if let Some(error) = &status.last_error {
        ui.label(format!("Last error: {error}"));
    }
}

pub(super) fn physical_hands_settings(ui: &mut Ui, p: &mut PhysicalHandsConfig) {
    let mut hull_enabled = human_hull::status().enabled;
    if ui
        .checkbox(&mut hull_enabled, "Half-width human collision hulls")
        .changed()
    {
        human_hull::set_enabled(hull_enabled);
    }
    ui.label("Restoration waits for full-size clearance. The current hull remains installed while restoration is deferred.");
    ui.checkbox(&mut p.enabled, "Physical palms");
    ui.add_enabled_ui(p.enabled, |ui| {
        ui.checkbox(&mut p.bodies.enabled, "Hand bodies")
            .on_hover_text(
                "Give each empty hand a body in the physics world, in place of the palm probes.",
            );
        ui.checkbox(&mut p.push, "Push off surfaces");
        ui.checkbox(&mut p.pickup.enabled, "Pick up objects with the grip");
        ui.add_enabled_ui(p.pickup.enabled, |ui| {
            ui.add(egui::Slider::new(&mut p.pickup.throw_scale, 0.0..=4.0).text("Throw (× hand)"));
            ui.add(
                egui::Slider::new(&mut p.pickup.carrier_motion_share, 0.0..=1.0)
                    .text("Carry with the body's motion"),
            )
            .on_hover_text(
                "The share of the body's own jumps, landings, and launches a carried object \
                 takes on, so that it stays in the hands rather than being torn away.",
            );
        });
        ui.checkbox(
            &mut p.vault.enabled,
            "Push the body up with the hands pressed down",
        );
        ui.checkbox(&mut p.bodies.reactions, "Characters react to blows");
    });
}

pub(super) fn characters_doors_settings(ui: &mut Ui, p: &mut PhysicalHandsConfig) {
    ui.add_enabled_ui(p.enabled, |ui| {
        ui.checkbox(&mut p.characters.grab, "Take hold of characters on foot");
        ui.checkbox(
            &mut p.characters.hijack,
            "Pull occupants out through open doors",
        );
        ui.checkbox(&mut p.doors.enabled, "Open vehicle doors by hand");
        ui.add_enabled_ui(p.doors.enabled, |ui| {
            ui.checkbox(&mut p.doors.tear, "Tear detachable doors by hand");
            let hinge = &mut p.doors.hinge;
            ui.checkbox(&mut hinge.enabled, "Hold opened doors where they are left")
                .on_hover_text(
                    "Damp a door the hands opened, prop a lid open like a gas strut, and latch \
                     only a door shoved shut or resting shut, never one a hand holds.",
                );
            ui.add_enabled_ui(hinge.enabled, |ui| {
                ui.add(
                    egui::Slider::new(&mut hinge.latch_speed_deg_s, 10.0..=200.0)
                        .text("Latches past (deg/s)"),
                );
                ui.add(
                    egui::Slider::new(&mut hinge.settle_s, 0.2..=5.0)
                        .text("Latches at rest after (s)"),
                );
                ui.add(
                    egui::Slider::new(&mut hinge.damping_per_s, 0.0..=10.0)
                        .text("Door damping (/s)"),
                );
                ui.add(
                    egui::Slider::new(&mut hinge.strut_fraction, 0.0..=1.0)
                        .text("Lid strut above (share of travel)"),
                );
            });
        });
    });
}

pub(super) fn characters_doors_readout(ui: &mut Ui, status: &PhysicalHandsStatus) {
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let s = &status.hands[hand.index()];
            ui.strong(hand.label());
            if !s.acting || s.held.is_some() {
                continue;
            }
            if let Some(reach) = s.reach {
                match reach.kind {
                    TargetKind::Character(target) => {
                        ui.label(format!(
                            "reaches a {} character {:.0} mm away",
                            if target.seated.is_some() {
                                "seated"
                            } else {
                                "standing"
                            },
                            reach.distance * 1000.0
                        ));
                        if let Some(seated) = target.seated {
                            ui.label(format!(
                                "the character sits behind the {} door, open {:.0}°",
                                doors::slot_label(seated.door),
                                seated.open_deg
                            ));
                        }
                    }
                    TargetKind::Door(door) => {
                        ui.label(format!(
                            "reaches the {} door ({}) {:.0} mm away",
                            doors::slot_label(door.slot),
                            if door.latched { "latched" } else { "open" },
                            reach.distance * 1000.0
                        ));
                    }
                    _ => {}
                }
            }
            if s.latched {
                ui.label("grips a latched door");
            }
        }
    });
    let p = &status.pickup;
    ui.label(format!(
        "Doors: {} latched grips · {} unlatched · {} torn · {} tear refusals{}",
        p.door_grips,
        p.unlatches,
        p.tears,
        p.tear_refusals,
        p.last_door.map_or(String::new(), |slot| format!(
            " · last {}",
            doors::slot_label(slot)
        ))
    ));
    ui.label(format!(
        "Characters: {} grips · {} yanked · {} ragdolls taken · {} pulled out of a seat",
        p.character_grips, p.yanks, p.regrips, p.hijacks
    ));
}

pub(super) fn impact_sounds_settings(ui: &mut Ui, p: &mut PhysicalHandsConfig) {
    ui.add_enabled_ui(p.enabled, |ui| {
        ui.checkbox(&mut p.sounds.enabled, "Impact sounds")
            .on_hover_text(
                "The game's melee impact sound where a hand meets something hard enough.",
            );
    });
}

pub(super) fn impact_sounds_readout(ui: &mut Ui, status: &SoundsStatus) {
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
