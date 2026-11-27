//! Scoped weapons: the aiming rows, each hand against walls, the shares down the sights, whether a
//! scope is detected, the eyepiece's distance from the nearest eye, the lens's eyes, and the
//! precision-aim suppression's counts.

use egui::Ui;
use jc3vr_body::controllers::aim::sights::SightSource;
use jc3vr_input::frame::Hand;
use jc3vr_weapons::controllers::weapons::{
    ads,
    ads::AdsStatus,
    collide,
    collide::HandCollideStatus,
    scope,
    scope::{ScopeStatus, suppress, suppress::SuppressCounts},
};

use crate::{
    status::explosives::{DOWN_THE_SIGHTS, GUN_AGAINST_WALLS},
    ui::status_row::headlines,
};

/// Everything the scope section shows, read before the config is locked for editing.
pub(super) struct ScopeReadout {
    status: ScopeStatus,
    counts: SuppressCounts,
    suppressing: bool,
    ads: AdsStatus,
}

impl ScopeReadout {
    pub(super) fn read() -> Self {
        Self {
            status: scope::status(),
            counts: suppress::counts(),
            suppressing: suppress::suppressing(),
            ads: ads::status(),
        }
    }
}

pub(super) fn scope_readout(ui: &mut Ui, readout: &ScopeReadout) {
    let ScopeReadout {
        status,
        counts,
        suppressing,
        ads,
    } = readout;
    headlines(ui, &[&DOWN_THE_SIGHTS, &GUN_AGAINST_WALLS]);
    let c = collide::status();
    let against = |h: &HandCollideStatus| match (h.weapon, h.touching) {
        (false, _) => "no long gun".to_owned(),
        (true, false) => "clear".to_owned(),
        (true, true) => format!(
            "against a wall, turned {:.0}°, pulled back {:.1} cm",
            h.turn_deg,
            h.retract_m * 100.0
        ),
    };
    ui.label(format!(
        "Against walls: left {} · right {} · probes {}",
        against(&c.hands[Hand::Left.index()]),
        against(&c.hands[Hand::Right.index()]),
        c.casts
    ));
    ui.label(format!(
        "Down the sights: stock {:.0}% · sights {:.0}% · weld {:.0}%",
        ads.shoulder * 100.0,
        ads.eye * 100.0,
        ads.weld * 100.0,
    ));
    ui.label(format!(
        "Precision aim: {} · refused {} · left {}",
        if *suppressing {
            "suppressed"
        } else {
            "allowed"
        },
        counts.refused,
        counts.exited
    ));
    match &status.scope {
        Some(s) => ui.label(format!(
            "Scope detected: {:#010x} in the {:?} hand · sight line from {}",
            s.hash,
            s.hand,
            match s.sight {
                SightSource::FirePoint =>
                    "the fire point (along the barrel, below the scope)".to_owned(),
                source => source.label(),
            }
        )),
        None => ui.label("Scope detected: no"),
    };
    ui.label(match status.eyepiece_distance_m {
        Some(d) => format!("Eyepiece distance: {:.3} m from the nearest eye", d),
        None => "Eyepiece distance: none".to_owned(),
    });
    ui.label(match (status.idle, status.eyes) {
        (Some(idle), _) => format!("Lens: {}", idle.label()),
        (None, Some((sighting, source))) if sighting == source => {
            format!("Lens: eye {sighting} through its own image")
        }
        (None, Some((sighting, source))) => {
            format!("Lens: eye {sighting} through eye {source}'s image")
        }
        (None, None) => "Lens: at the eye, waiting for an eye image".to_owned(),
    });
    ui.label(format!("Frames drawn: {}", status.frames));
    if let Some(error) = &status.error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
}
