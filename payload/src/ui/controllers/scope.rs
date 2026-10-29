//! Scoped weapons: whether a scope is detected, the eyepiece's distance from the nearest eye, the
//! lens's eyes, the precision-aim suppression's counts, and the aim-and-sights switches.

use egui::Ui;

use crate::controllers::{
    aim::sights::SightSource,
    frame::Hand,
    weapons::{
        ads::{
            self, AdsStatus,
            config::{AdsConfig, AimingEye},
        },
        collide::{self, HandCollideStatus, config::CollideConfig},
        scope::{
            self, ScopeStatus,
            suppress::{self, SuppressCounters},
        },
    },
};

/// Everything the scope section shows, read before the config is locked for editing.
pub(super) struct ScopeReadout {
    status: ScopeStatus,
    counters: SuppressCounters,
    suppressing: bool,
    ads: AdsStatus,
}

impl ScopeReadout {
    pub(super) fn read() -> Self {
        Self {
            status: scope::status(),
            counters: suppress::counters(),
            suppressing: suppress::suppressing(),
            ads: ads::status(),
        }
    }
}

pub(super) fn scope_readout(ui: &mut Ui, readout: &ScopeReadout) {
    let ScopeReadout {
        status,
        counters,
        suppressing,
        ads,
    } = readout;
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
        "Against walls: left {} · right {} · contacts {} · probes {}",
        against(&c.hands[Hand::Left.index()]),
        against(&c.hands[Hand::Right.index()]),
        c.contacts,
        c.casts
    ));
    ui.label(format!(
        "Down the sights: {} · raised {} · stock {:.0}% · sights {:.0}% · weld {:.0}% · eye off \
         the line {} · sighted {} · shouldered {} · welded {}",
        match (ads.weapon, ads.scoped) {
            (false, _) => "no long gun measured",
            (true, false) => "long gun",
            (true, true) => "scoped long gun",
        },
        ads.raised.map_or("none", |h| h.label()),
        ads.shoulder * 100.0,
        ads.eye * 100.0,
        ads.weld * 100.0,
        ads.eye_offset_m
            .map_or("-".to_owned(), |m| format!("{:.1} cm", m * 100.0)),
        ads.sighted,
        ads.shouldered,
        ads.welded
    ));
    ui.label(format!(
        "Precision aim: {} · refused {} · left {}",
        if *suppressing {
            "suppressed"
        } else {
            "allowed"
        },
        counters.refused,
        counters.exited
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

pub(super) fn scope_settings(ui: &mut Ui, scopes: &mut bool) {
    ui.checkbox(scopes, "Scope lenses")
        .on_hover_text("Draw a magnified image on a scoped weapon's lens while it is at an eye.");
}

/// The settings of aiming a long gun down its sights.
pub(super) fn ads_settings(ui: &mut Ui, a: &mut AdsConfig) {
    ui.checkbox(&mut a.enabled, "Line the sights up with the eye")
        .on_hover_text(
            "A long gun raised to the eye turns so its sight line passes through the aiming eye; \
             the shot follows the sights.",
        );
    ui.add_enabled_ui(a.enabled, |ui| {
        egui::ComboBox::from_label("Aiming eye")
            .selected_text(a.eye.label())
            .show_ui(ui, |ui| {
                for eye in AimingEye::ALL {
                    ui.selectable_value(&mut a.eye, eye, eye.label());
                }
            });
        ui.checkbox(&mut a.shoulder, "Seat the stock in the shoulder");
        ui.checkbox(&mut a.cheek_weld, "Cheek weld for scopes");
    });
}

/// The settings of held guns against walls.
pub(super) fn collide_settings(ui: &mut Ui, c: &mut CollideConfig) {
    ui.checkbox(&mut c.enabled, "Held long guns stop at walls");
}
