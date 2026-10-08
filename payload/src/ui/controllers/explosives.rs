//! C4 by hand: the throw from the chest pouch, the placed charges, the detonations, the wrist
//! detonator's press, and their settings.

use egui::Ui;

use crate::{
    controllers::{
        config::WeaponsConfig,
        weapons::{
            c4::{
                self, C4Status,
                wrist::{self, WristStatus},
            },
            config::{C4Config, WristDetonatorConfig},
        },
    },
    ui::controllers::{vec3, vec3_drag, yes_no},
};

/// Everything the C4 readout shows, read before the config is locked for editing.
pub(super) struct ExplosivesReadout {
    c4: C4Status,
    wrist: WristStatus,
}

impl ExplosivesReadout {
    pub(super) fn read() -> Self {
        Self {
            c4: c4::status(),
            wrist: wrist::status(),
        }
    }
}

pub(super) fn explosives_readout(ui: &mut Ui, readout: &ExplosivesReadout) {
    let c = &readout.c4;
    let hand = c
        .stage
        .hand()
        .map_or(String::new(), |h| format!(" in the {} hand", h.label()));
    ui.label(format!(
        "C4: {}{hand} · primed {} · thrown {}{}",
        c.stage.label(),
        c.primed,
        c.thrown,
        if c.weapon_seen {
            ""
        } else {
            " · the C4 weapon has not updated yet"
        }
    ));
    ui.label(format!(
        "  charges placed {} · may plant {} · may detonate {} · game planting mode {}",
        c.charges
            .map_or("unknown".to_owned(), |(live, max)| format!(
                "{live} of {max}"
            )),
        yes_no(c.may_plant),
        yes_no(c.may_detonate),
        yes_no(c.planting_mode)
    ));
    ui.label(format!(
        "  pressed onto surfaces {} · of them onto objects {}",
        c.stuck, c.stuck_on_objects
    ));
    if let Some(v) = c.last_release {
        ui.label(format!(
            "  last release {} ({:.1} m/s)",
            vec3(v),
            v.length()
        ));
    }
    for event in c.events.iter().rev() {
        ui.label(format!(
            "  {}{}",
            event
                .hand
                .map_or(String::new(), |h| format!("{}: ", h.label())),
            event.outcome.label()
        ));
    }
    ui.label(format!(
        "  detonations {} · request pending {} · last: {}",
        c.detonations,
        yes_no(c.detonation_pending),
        c.last_detonation.map_or("none".to_owned(), |d| format!(
            "{} with {} placed{}",
            d.source.label(),
            d.charges,
            if d.started {
                ""
            } else {
                ", not started by the game"
            }
        ))
    ));
    let w = &readout.wrist;
    if !w.acting {
        ui.label("Wrist detonator: inactive");
        return;
    }
    ui.label(format!(
        "Wrist detonator: {} · lit {} · fingertip from {} · presses {} (with nothing placed {})",
        w.reading.phase.label(),
        yes_no(w.lit),
        w.fingertip.map_or("nowhere", |f| f.label()),
        w.presses,
        w.idle_presses
    ));
    if let (Some(distance), Some(height)) = (w.reading.distance_m, w.reading.height_m) {
        ui.label(format!(
            "  fingertip {distance:.3} m from the button, {height:+.3} m in front of its face"
        ));
    }
}

/// The C4 throw's and the wrist detonator's settings.
pub(super) fn explosives_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(&mut w.c4_throw, "Physical C4 throw").on_hover_text(
        "A grip press at the chest pouch primes a C4 charge; opening the grip throws it, and it \
         sticks where it lands.",
    );
    ui.add_enabled_ui(w.c4_throw, |ui| {
        ui.indent("c4", |ui| c4_settings(ui, &mut w.c4));
    });
    ui.checkbox(&mut w.wrist_detonator, "Wrist detonator").on_hover_text(
        "Push the dominant index fingertip into the button on the inside of the off-hand's wrist \
         to detonate the placed charges.",
    );
    ui.add_enabled_ui(w.wrist_detonator, |ui| {
        ui.indent("wrist detonator", |ui| {
            detonator_settings(ui, &mut w.c4.detonator)
        });
    });
}

fn c4_settings(ui: &mut Ui, c: &mut C4Config) {
    vec3_drag(
        ui,
        "Hold offset (grip frame, m)",
        &mut c.hold_offset_m,
        0.005,
    );
    ui.add(egui::Slider::new(&mut c.release_window_s, 0.02..=0.3).text("Release window (s)"));
    ui.add(egui::Slider::new(&mut c.release_peak_span_s, 0.0..=0.05).text("Release peak span (s)"));
    ui.add(egui::Slider::new(&mut c.velocity_scale, 0.25..=3.0).text("Velocity scale"));
    ui.add(egui::Slider::new(&mut c.max_speed_mps, 5.0..=60.0).text("Fastest throw (m/s)"));
}

fn detonator_settings(ui: &mut Ui, d: &mut WristDetonatorConfig) {
    vec3_drag(ui, "Button (left grip frame, m)", &mut d.offset_m, 0.002);
    vec3_drag(ui, "Button normal (left grip frame)", &mut d.normal, 0.01);
    ui.add(egui::Slider::new(&mut d.radius_m, 0.01..=0.08).text("Button radius (m)"));
    ui.add(egui::Slider::new(&mut d.touch_radius_m, 0.005..=0.08).text("Touch radius (m)"));
    ui.add(
        egui::Slider::new(&mut d.release_radius_m, d.touch_radius_m..=0.15)
            .text("Release radius (m)"),
    );
    ui.add(egui::Slider::new(&mut d.max_behind_m, 0.0..=0.05).text("Deepest behind the face (m)"));
    ui.add(
        egui::Slider::new(&mut d.min_entry_speed_mps, 0.0..=1.0).text("Quick press speed (m/s)"),
    );
    ui.add(egui::Slider::new(&mut d.dwell_s, 0.0..=1.0).text("Slow press dwell (s)"));
    vec3_drag(
        ui,
        "Fingertip pointing (right aim frame, m)",
        &mut d.fingertip_pointing_m,
        0.002,
    );
    vec3_drag(
        ui,
        "Fingertip on the trigger (right aim frame, m)",
        &mut d.fingertip_resting_m,
        0.002,
    );
    ui.checkbox(&mut d.overlay, "Draw the button");
}
