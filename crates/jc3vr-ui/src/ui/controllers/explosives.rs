//! C4 by hand: the throw from the chest pouch, the placed charges, the detonator, the detonations,
//! and their switches.

use egui::Ui;
use jc3vr_weapons::controllers::weapons::{
    c4,
    c4::{C4Status, detonator},
    config::WeaponsConfig,
};

use crate::ui::controllers::{vec3, yes_no};

/// Everything the C4 readout shows, read before the config is locked for editing.
pub(super) struct ExplosivesReadout {
    c4: C4Status,
    detonator_model: bool,
}

impl ExplosivesReadout {
    pub(super) fn read() -> Self {
        Self {
            c4: c4::status(),
            detonator_model: detonator::model_shown(),
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
        "C4: {}{hand} · primed {}{}",
        c.stage.label(),
        c.primed,
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
        "  put back into the pouch {} · detonated from a held charge {} · taken back from where they \
         were placed {}",
        c.returned, c.held_presses, c.taken
    ));
    ui.label(format!(
        "  detonator: {} · model drawn {} · presses {} (with nothing to detonate {})",
        c.detonator.map_or("on the chest".to_owned(), |h| format!(
            "in the {} hand",
            h.label()
        )),
        yes_no(readout.detonator_model),
        c.detonator_presses,
        c.idle_presses
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
        "  detonations {} · request pending {} · in flight {} · last: {}",
        c.detonations,
        yes_no(c.detonation_pending),
        yes_no(c.detonation_in_flight),
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
}

/// The switches of the C4 throw and the press onto surfaces.
pub(super) fn explosives_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(&mut w.c4_throw, "Physical C4 throw").on_hover_text(
        "A grip press at the chest pouch primes a C4 charge; opening the grip throws it, and it \
         sticks where it lands. The trigger of a hand holding a charge swaps it for the detonator, \
         whose trigger detonates the placed charges.",
    );
    ui.checkbox(&mut w.c4.stick.enabled, "Press C4 onto surfaces")
        .on_hover_text("A held charge pressed onto a surface sticks there.");
}
