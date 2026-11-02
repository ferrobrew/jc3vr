//! C4 by hand: the throw from the chest pouch, the placed charges, the detonations, the wrist
//! detonator's press, and their switches.

use egui::Ui;

use crate::{
    controllers::{
        config::WeaponsConfig,
        weapons::c4::{
            self, C4Status,
            wrist::{self, WristStatus},
        },
    },
    ui::controllers::{vec3, yes_no},
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
        "  pressed onto surfaces {} · of them onto objects {} · put back into the pouch {}",
        c.stuck, c.stuck_on_objects, c.returned
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

/// The switches of the C4 throw, the press onto surfaces, and the wrist detonator.
pub(super) fn explosives_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(&mut w.c4_throw, "Physical C4 throw").on_hover_text(
        "A grip press at the chest pouch primes a C4 charge; opening the grip throws it, and it \
         sticks where it lands.",
    );
    ui.checkbox(&mut w.c4.stick.enabled, "Press C4 onto surfaces")
        .on_hover_text("A held charge pressed onto a surface sticks there.");
    ui.checkbox(&mut w.wrist_detonator, "Wrist detonator").on_hover_text(
        "Push the dominant index fingertip into the button on the inside of the off-hand's wrist \
         to detonate the placed charges.",
    );
}
