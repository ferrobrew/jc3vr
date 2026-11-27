//! The collider view: its show and hide button, what it read, and the body under the gaze.

use egui::Ui;
use jc3gi::physics::CustomGroupCollisionFilter;
use jc3vr_physical::controllers::physical_hands::{
    collider_view,
    collider_view::{ColliderViewStatus, model::BodyKind},
};
use jc3vr_tooling::director::{keymap, keymap::Action};

pub(super) fn collider_view_readout(ui: &mut Ui, status: &ColliderViewStatus) {
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
