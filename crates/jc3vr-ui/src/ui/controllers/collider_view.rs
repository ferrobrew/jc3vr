//! The collider view: its status row, its switch, what it read, the body under the gaze, and what it
//! draws.

use egui::Ui;
use jc3gi::physics::CustomGroupCollisionFilter;

use crate::{
    controllers::physical_hands::collider_view::{
        self, ColliderViewConfig, ColliderViewStatus, model::BodyKind,
    },
    director::keymap::{self, Action},
    status::controllers::COLLIDER_VIEW,
    ui::status_row::headlines,
};

pub(super) fn collider_view_readout(ui: &mut Ui, status: &ColliderViewStatus) {
    headlines(ui, &[&COLLIDER_VIEW]);
    let label = if status.enabled {
        "Hide the colliders".to_owned()
    } else {
        match keymap::chord_label(Action::ToggleColliderView) {
            Some(key) => format!("Show the colliders ({key})"),
            None => "Show the colliders".to_owned(),
        }
    };
    if ui.button(label).clicked() {
        collider_view::toggle();
    }
    if !status.enabled {
        return;
    }
    ui.label(format!(
        "{} steps · {} reads · last read {} slots in {} µs",
        status.steps, status.reads, status.scanned, status.read_us
    ));
    let kinds = BodyKind::ALL
        .into_iter()
        .filter_map(|kind| {
            let count = status.drawn.kinds[kind.index()];
            (count > 0).then(|| format!("{count} {}", kind.label()))
        })
        .collect::<Vec<_>>();
    if !kinds.is_empty() {
        ui.label(kinds.join(" · "));
    }
    match &status.gaze {
        Some(gaze) => {
            ui.label(format!(
                "Gaze: body {} · {} · {} motion · layer {} · group {} · {} · {} · {:.1} m",
                gaze.id,
                gaze.kind.label(),
                gaze.motion.label(),
                gaze.filter_info & CustomGroupCollisionFilter::LAYER_MASK,
                gaze.filter_info >> CustomGroupCollisionFilter::SYSTEM_GROUP_SHIFT,
                gaze.mass_kg
                    .map_or_else(|| "infinite mass".to_owned(), |m| format!("{m:.1} kg")),
                gaze.shape,
                gaze.distance_m
            ));
        }
        None => {
            ui.label("Gaze: no body");
        }
    }
    // Before the first read, the row carries the error as its note.
    if let Some(error) = &status.last_error
        && status.reads > 0
    {
        ui.label(format!("Last error: {error}"));
    }
}

pub(super) fn collider_view_settings(ui: &mut Ui, v: &mut ColliderViewConfig) {
    ui.add_enabled_ui(v.enabled, |ui| {
        ui.checkbox(&mut v.boxes, "Boxes");
        ui.checkbox(&mut v.shapes, "Shapes (spheres, capsules, convex hulls)");
        ui.checkbox(&mut v.static_bodies, "Static bodies");
        ui.checkbox(&mut v.contacts, "Contacts and the player's ground");
        ui.checkbox(&mut v.joints, "The hands' and guns' joints");
        ui.checkbox(&mut v.gaze, "The body under the gaze");
    });
}
