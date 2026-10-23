//! Scoped weapons: whether a scope is detected, the eyepiece's distance from the nearest eye, the
//! lens path in use, the precision-aim suppression's counts, and the scope settings.

use egui::Ui;

use crate::{
    controllers::{
        aim::sights::SightSource,
        frame::Hand,
        weapons::{
            ads::{
                self, AdsStatus,
                config::{AdsConfig, AimingEye},
            },
            collide::{self, HandCollideStatus, config::CollideConfig},
            config::{LensPath, LensSource, ScopeConfig},
            scope::{
                self, ScopeStatus,
                suppress::{self, SuppressCounters},
            },
        },
    },
    ui::controllers::vec3_drag,
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
        "Down the sights: {} · stock {:.0}% · sights {:.0}% · weld {:.0}% · eye off the line {} · \
         sighted {} · shouldered {} · welded {}",
        match (ads.weapon, ads.scoped) {
            (false, _) => "no long gun measured",
            (true, false) => "long gun",
            (true, true) => "scoped long gun",
        },
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
    let path = if status.scene_render_fallback {
        "resample (the scene render is not implemented)"
    } else {
        "resample"
    };
    ui.label(match (status.idle, status.eyes) {
        (Some(idle), _) => format!("Lens: {} · path {path}", idle.label()),
        (None, Some((sighting, source))) if sighting == source => {
            format!("Lens: eye {sighting} through its own image · path {path}")
        }
        (None, Some((sighting, source))) => {
            format!("Lens: eye {sighting} through eye {source}'s image · path {path}")
        }
        (None, None) => format!("Lens: at the eye, waiting for an eye image · path {path}"),
    });
    ui.label(format!("Frames drawn: {}", status.frames));
    if let Some(error) = &status.error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
}

pub(super) fn scope_settings(ui: &mut Ui, scopes: &mut bool, s: &mut ScopeConfig) {
    ui.checkbox(&mut s.suppress_precision_aim, "Suppress precision aim")
        .on_hover_text(
            "Refuse the game's precision aim (its zoom, sniper overlay, and hidden body) under the \
             controller scheme.",
        );
    ui.checkbox(scopes, "Scope lenses")
        .on_hover_text("Draw a magnified image on a scoped weapon's lens while it is at an eye.");
    ui.add_enabled_ui(*scopes, |ui| {
        ui.horizontal(|ui| {
            ui.label("Path");
            ui.selectable_value(&mut s.path, LensPath::Resample, "Resample");
            ui.selectable_value(&mut s.path, LensPath::SceneRender, "Scene render")
                .on_hover_text("Not implemented: the resample is drawn instead.");
        });
        ui.horizontal(|ui| {
            ui.label("Image");
            ui.selectable_value(&mut s.source, LensSource::OtherEye, "Other eye")
                .on_hover_text("The other eye's image, which the scope does not block.");
            ui.selectable_value(&mut s.source, LensSource::OwnEye, "Own eye")
                .on_hover_text("The sighting eye's own image, which shows the scope's model.");
        });
        ui.add(egui::Slider::new(&mut s.zoom, 1.0..=12.0).text("Zoom"));
        ui.add(egui::Slider::new(&mut s.lens_radius_m, 0.005..=0.05).text("Lens radius (m)"));
        ui.add(
            egui::Slider::new(&mut s.eyepiece_back_m, 0.0..=1.5)
                .text("Eyepiece behind the sight origin (m)"),
        );
        ui.add(
            egui::Slider::new(&mut s.max_eye_distance_m, 0.02..=0.5)
                .text("Eye distance to show the lens (m)"),
        );
        ui.add(egui::Slider::new(&mut s.vignette_start, 0.0..=0.99).text("Vignette start"));
        ui.checkbox(&mut s.reticle, "Reticle");
    });
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
        ui.add(egui::Slider::new(&mut a.half_ipd_m, 0.02..=0.045).text("Half the eye spacing (m)"));
        ui.add(egui::Slider::new(&mut a.engage_m, 0.01..=0.15).text("Engage (m)"));
        ui.add(egui::Slider::new(&mut a.lock_m, 0.0..=0.1).text("Lock (m)"));
        ui.add(
            egui::Slider::new(&mut a.max_eye_behind_m, 0.1..=0.8).text("Eye behind the grip (m)"),
        );
        ui.add(egui::Slider::new(&mut a.max_view_angle_deg, 5.0..=60.0).text("View angle (°)"));
        ui.add(
            egui::Slider::new(&mut a.pivot_ahead_m, 0.1..=0.8).text("Pivot ahead of the grip (m)"),
        );
        ui.checkbox(&mut a.shoulder, "Seat the stock in the shoulder");
        ui.add(
            egui::Slider::new(&mut a.butt_behind_grip_m, 0.05..=0.5)
                .text("Butt behind the grip (m)"),
        );
        vec3_drag(
            ui,
            "Pocket offset (torso frame, m)",
            &mut a.pocket_offset_m,
            0.005,
        );
        ui.add(egui::Slider::new(&mut a.shoulder_engage_m, 0.02..=0.3).text("Shoulder engage (m)"));
        ui.add(egui::Slider::new(&mut a.shoulder_lock_m, 0.0..=0.2).text("Shoulder lock (m)"));
        ui.checkbox(&mut a.cheek_weld, "Cheek weld for scopes");
        ui.add(egui::Slider::new(&mut a.weld_engage_m, 0.01..=0.15).text("Weld engage (m)"));
        ui.add(egui::Slider::new(&mut a.weld_lock_m, 0.0..=0.1).text("Weld lock (m)"));
    });
}

/// The settings of held guns against walls.
pub(super) fn collide_settings(ui: &mut Ui, c: &mut CollideConfig) {
    ui.checkbox(&mut c.enabled, "Held long guns stop at walls");
    ui.add_enabled_ui(c.enabled, |ui| {
        ui.add(egui::Slider::new(&mut c.tip_margin_m, 0.0..=0.15).text("Past the muzzle (m)"));
        ui.add(egui::Slider::new(&mut c.max_turn_deg, 0.0..=90.0).text("Furthest turn (°)"));
        ui.add(egui::Slider::new(&mut c.max_retract_m, 0.0..=0.6).text("Furthest pull back (m)"));
        ui.add(egui::Slider::new(&mut c.keep_s, 0.0..=1.0).text("Wall kept for (s)"));
    });
}
