//! The sight tuning tool, in the aiming settings: the held weapon's sight offset and its source,
//! the line-up marker and the capture, fine adjustment, the resets, and copying the entry in the
//! sight table's line format.

use egui::Ui;

use crate::{
    controllers::{
        aim::{
            self,
            sights::{self, SightSource, tuning},
        },
        config::{ControllersConfig, SightOverride, SightTable},
    },
    ui::controllers::{vec3, vec3_drag},
};

/// Called with the config locked for editing: everything read here takes other locks only.
pub(super) fn sight_tuning(ui: &mut Ui, c: &mut ControllersConfig) {
    let t = &mut c.aim.sight_tuning;
    ui.checkbox(&mut t.enabled, "Sight tuning tool")
        .on_hover_text(
            "Line the held gun's sights up on a marker and capture the sight offset that makes its \
         sight line pass through it. Arms the capture chord while the marker is placed.",
        );
    if !t.enabled {
        return;
    }
    ui.add(egui::Slider::new(&mut t.marker_distance_m, 2.0..=50.0).text("Marker distance (m)"));

    let status = tuning::status();
    ui.horizontal(|ui| {
        let place = if status.marker.is_some() {
            "Place the marker again"
        } else {
            "Line up"
        };
        if ui
            .button(place)
            .on_hover_text("Place the marker straight ahead of the head.")
            .clicked()
        {
            tuning::place_marker();
        }
        if ui
            .add_enabled(status.marker.is_some(), egui::Button::new("Stop"))
            .clicked()
        {
            tuning::clear_marker();
        }
        if ui
            .add_enabled(status.marker.is_some(), egui::Button::new("Capture"))
            .on_hover_text(
                "Store the offset that makes the dominant hand's sight line pass through the \
                 marker. With a controller: hold both grips (the dominant one first) and press \
                 the dominant A.",
            )
            .clicked()
        {
            tuning::request_capture();
        }
    });
    ui.label(match status.marker {
        Some(marker) => format!("Marker at {}", vec3(marker)),
        None => "No marker".to_owned(),
    });
    match &status.last {
        Some(Ok(sight)) => ui.label(format!(
            "Captured {} · offset {:+.4}, {:+.4}, {:+.4} m · {} stored",
            weapon_label(sight.weapon_hash),
            sight.offset_m.x,
            sight.offset_m.y,
            sight.offset_m.z,
            status.captures
        )),
        Some(Err(e)) => ui.label(e.to_string()),
        None => ui.label("No capture yet"),
    };

    ui.separator();
    let Some(hash) = aim::status().weapon.map(|w| w.hash) else {
        ui.label("No weapon wielded");
        return;
    };
    let resolved = sights::resolve(&c.aim.sights, hash);
    let source = match resolved.source {
        SightSource::Config => {
            let beneath = sights::resolve(&SightTable::EMPTY, hash).source;
            format!("config (over {})", beneath.label())
        }
        source => source.label(),
    };
    ui.label(format!(
        "Wielded {} · sight from {source}",
        weapon_label(hash)
    ));
    if let Some(entry) = sights::table_entry(hash).filter(|e| !e.grade.trusted()) {
        ui.label(format!(
            "The embedded table's entry is graded {} and not used.",
            entry.grade.label()
        ));
    }

    let mut sight = resolved.sight.unwrap_or(SightOverride::identity(hash));
    let before = sight;
    vec3_drag(
        ui,
        "Offset (fire-point frame, m)",
        &mut sight.offset_m,
        0.0005,
    );
    ui.horizontal(|ui| {
        ui.label("Pitch and yaw (°)");
        ui.add(egui::DragValue::new(&mut sight.angles_deg.x).speed(0.02));
        ui.add(egui::DragValue::new(&mut sight.angles_deg.y).speed(0.02));
    });
    if sight != before && !c.aim.sights.set(sight) {
        tracing::warn!(target: "controllers", "aim: the sight table is full");
    }

    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                resolved.source == SightSource::Config,
                egui::Button::new("Reset to the table"),
            )
            .on_hover_text(
                "Remove the config's entry: the embedded table's entry applies when it is trusted, \
                 and the fire point otherwise.",
            )
            .clicked()
        {
            c.aim.sights.remove(hash);
        }
        if ui
            .add_enabled(
                resolved.sight.is_some(),
                egui::Button::new("Reset to the fire point"),
            )
            .on_hover_text("Store an entry with no offset and no angles.")
            .clicked()
            && !c.aim.sights.set(SightOverride::identity(hash))
        {
            tracing::warn!(target: "controllers", "aim: the sight table is full");
        }
        if ui
            .button("Copy as JSON")
            .on_hover_text(
                "Copy the weapon's entry in the line format of tools/sights/sights.json, graded \
                 tuned. Local tuning is saved with the settings; copy this entry to update the \
                 embedded table.",
            )
            .clicked()
        {
            let line = tuning::json_line(&sight);
            match clipboard_win::set_clipboard_string(&line) {
                Ok(()) => tracing::info!(target: "controllers", "sight tuning: copied {line}"),
                Err(e) => tracing::warn!(
                    target: "controllers",
                    "sight tuning: could not copy to the clipboard: {e}"
                ),
            }
        }
    });
}

/// The weapon's name from the embedded table, with its hash.
fn weapon_label(hash: u32) -> String {
    match sights::table_entry(hash) {
        Some(entry) => format!("{} ({hash:08x})", entry.weapon),
        None => format!("{hash:08x}"),
    }
}
