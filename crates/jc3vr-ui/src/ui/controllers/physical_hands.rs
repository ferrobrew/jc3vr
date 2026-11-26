//! Physical hands: each palm's contacts and push, what each hand reaches and holds, and the human
//! hull's switches.

use egui::Ui;
use jc3vr_body::controllers::grab::world_reach::{SeatAccess, TargetKind};
use jc3vr_input::frame::Hand;
use jc3vr_physical::controllers::physical_hands::{
    PhysicalHandsStatus, doors, human_hull, human_hull::step_up, impacts, pickup::diagnostics,
    sounds::SoundsStatus,
};

pub(super) fn physical_hands_readout(ui: &mut Ui, status: &PhysicalHandsStatus) {
    let hull = &status.human_hull;
    ui.label(format!(
        "Human hulls: {:?}, clearance {:?}, {} settings tables, {} narrowed bodies.",
        hull.adapter, hull.clearance, hull.settings, hull.migrated_bodies,
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
    if status.idle.is_none() {
        ui.label(format!(
            "Push ({:+.2}, {:+.2}) m/s",
            status.push.x, status.push.y
        ));
    }
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
        "Object knocks {} · shove ticks {}",
        status.object_pushes, status.shoves
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
    let impacts = impacts::status();
    ui.label(format!(
        "Held impacts: {} bodies, {} contacts, {} feedback, {} suppressed, {} dropped",
        impacts.listening, impacts.contacts, impacts.feedback, impacts.suppressed, impacts.dropped
    ));
    if let Some(error) = &status.last_error {
        ui.label(format!("Last error: {error}"));
    }
}

pub(super) fn physical_hands_settings(ui: &mut Ui) {
    let mut hull_enabled = human_hull::status().enabled;
    if ui
        .checkbox(&mut hull_enabled, "Half-width human collision hulls")
        .changed()
    {
        human_hull::set_enabled(hull_enabled);
    }
    ui.label("Restoration waits for full-size clearance. The current hull remains installed while restoration is deferred.");
    let step_up = step_up::status();
    let mut step_up_enabled = step_up.enabled;
    if ui
        .checkbox(
            &mut step_up_enabled,
            "Step the narrowed player up taller steps",
        )
        .on_hover_text(
            "The half-width hull steps up edges only half as high as the game's own. While the \
             player walks into a step the game's hull would climb, lift them onto it.",
        )
        .changed()
    {
        step_up::set_enabled(step_up_enabled);
    }
    ui.label(format!(
        "Steps lifted onto: {}, last {}, lifting now: {}.",
        step_up.steps,
        step_up
            .last_height_m
            .map_or_else(|| "none".to_owned(), |h| format!("{h:.2} m")),
        step_up.lifting
    ));
}

pub(super) fn characters_doors_readout(ui: &mut Ui, status: &PhysicalHandsStatus) {
    let pressure = doors::pressure::status();
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let s = &status.hands[hand.index()];
            ui.strong(hand.label());
            let p = pressure[hand.index()];
            ui.label(format!(
                "Pressure: {} · {} · {:.0}/{:.0} mm",
                p.reason.label(),
                p.slot.map(doors::slot_label).unwrap_or("no door"),
                p.displacement_m * 1000.0,
                doors::tuning::DOORS.get().grip.unlatch_pull_m * 1000.0,
            ));
            if let doors::pressure::Reason::NativeFailure(error) = p.reason {
                ui.label(error.to_string());
            }
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
                            match seated.access {
                                SeatAccess::Door { slot, open_deg } => {
                                    ui.label(format!(
                                        "the character sits behind the {} door, open {:.0}°",
                                        doors::slot_label(slot),
                                        open_deg
                                    ));
                                }
                                SeatAccess::Pane => {
                                    ui.label(
                                        "the character sits behind a window the glass brought down",
                                    );
                                }
                                SeatAccess::Exposed => {
                                    ui.label("the character is an exposed motorcycle occupant");
                                }
                            }
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
        "Doors torn: {} · tear refusals {}",
        p.tears, p.tear_refusals
    ));
}

pub(super) fn impact_sounds_readout(ui: &mut Ui, status: &SoundsStatus) {
    ui.label(format!(
        "Last sound: {}",
        status
            .last
            .map_or("none".to_owned(), |(material, kind)| format!(
                "{} on material {material}",
                kind.label()
            ))
    ));
}
