//! The collider view: its switch, what it read and drew, the body under the gaze, and its
//! settings.

use egui::Ui;
use jc3gi::physics::CustomGroupCollisionFilter;

use crate::controllers::physical_hands::collider_view::{
    self, ColliderViewConfig, ColliderViewStatus, model::BodyKind,
};

pub(super) fn collider_view_readout(ui: &mut Ui, status: &ColliderViewStatus) {
    let label = if status.enabled {
        "Hide the colliders"
    } else {
        "Show the colliders (Ctrl+Numpad0)"
    };
    if ui.button(label).clicked() {
        collider_view::toggle();
    }
    if !status.enabled {
        return;
    }
    ui.label(format!(
        "Step counter {} · {} steps · {} reads · last read {} slots in {} µs",
        if status.registered {
            "registered"
        } else {
            "not registered"
        },
        status.steps,
        status.reads,
        status.scanned,
        status.read_us
    ));
    ui.label(format!(
        "{} bodies in range · {} drawn · {} over budget · {} primitives",
        status.in_range, status.drawn.bodies, status.drawn.dropped, status.primitives
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
    if let Some(error) = &status.last_error {
        ui.label(format!("Last error: {error}"));
    }
}

pub(super) fn collider_view_settings(ui: &mut Ui, v: &mut ColliderViewConfig) {
    ui.checkbox(&mut v.enabled, "Draw the physics colliders near the player");
    ui.add_enabled_ui(v.enabled, |ui| {
        ui.add(egui::Slider::new(&mut v.radius_m, 0.5..=50.0).text("Radius (m)"));
        ui.add(egui::Slider::new(&mut v.max_bodies, 1..=512).text("Most bodies"));
        ui.add(
            egui::Slider::new(
                &mut v.max_primitives,
                16..=collider_view::draw::PRIMITIVE_CEILING as u32,
            )
            .text("Most primitives"),
        );
        ui.checkbox(&mut v.boxes, "Boxes");
        ui.checkbox(&mut v.shapes, "Shapes (spheres, capsules, convex hulls)");
        ui.checkbox(&mut v.static_bodies, "Static bodies");
        ui.checkbox(&mut v.contacts, "Contacts and the player's ground");
        ui.checkbox(&mut v.joints, "The hands' and guns' joints");
        ui.checkbox(&mut v.gaze, "The body under the gaze");
    });
}
