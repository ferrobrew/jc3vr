//! Scoped weapons: whether a scope is detected, the eyepiece's distance from the nearest eye, the
//! lens path in use, the precision-aim suppression's counts, and the scope settings.

use egui::Ui;

use crate::controllers::{
    aim::sights::SightSource,
    weapons::{
        config::{LensPath, LensSource, ScopeConfig},
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
}

impl ScopeReadout {
    pub(super) fn read() -> Self {
        Self {
            status: scope::status(),
            counters: suppress::counters(),
            suppressing: suppress::suppressing(),
        }
    }
}

pub(super) fn scope_readout(ui: &mut Ui, readout: &ScopeReadout) {
    let ScopeReadout {
        status,
        counters,
        suppressing,
    } = readout;
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
