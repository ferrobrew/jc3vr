//! The native map: what the controller pointer hands the map on each of its input dispatches, and
//! what the map did with it.

use std::time::Instant;

use egui::Ui;

use crate::{
    controllers::{
        frame::Hand,
        map::{CLICK_SLOP, MapDelivery, MapReport, zoom_label},
    },
    hud::pointer::{ControllerPointer, RoutedPointer},
    ui::controllers::yes_no,
};

pub(super) fn map_readout(
    ui: &mut Ui,
    report: &MapReport,
    pointer: Option<ControllerPointer>,
    menu_trigger: Option<Hand>,
) {
    ui.label(match pointer {
        Some(pointer) => format!(
            "Pointer: {:?} hand at ({:.3}, {:.3}), trigger {}",
            pointer.hand,
            pointer.uv.0,
            pointer.uv.1,
            if pointer.buttons & 1 != 0 {
                "down"
            } else {
                "up"
            },
        ),
        None => "Pointer: not on the game UI".to_owned(),
    });
    ui.label(match menu_trigger {
        Some(hand) => format!("The pointer claims the {hand:?} trigger from the menu bindings"),
        None => "The pointer claims no trigger; the triggers page the menus".to_owned(),
    });
    ui.label(format!(
        "Map dispatches {} · trigger selections {} · drags {} · context actions from A {}",
        report.dispatches, report.selections, report.drags, report.context_actions
    ));
    let Some(last) = &report.last else {
        ui.label("The map has not handled input yet.");
        return;
    };
    ui.label(format!(
        "Last dispatch {:.1} s ago: {}",
        Instant::now()
            .saturating_duration_since(last.at)
            .as_secs_f32(),
        last.outcome.describe()
    ));
    ui.label(format!("Route: {}", route_label(last.route)));
    if let Some(delivery) = &last.delivery {
        delivery_readout(ui, delivery);
    }
    if let Some(cursor) = last.cursor_stage {
        ui.label(format!(
            "Map cursor (stage): ({:.0}, {:.0})",
            cursor.x, cursor.y
        ));
    }
    if last.context_from_confirm {
        ui.label("A was read this dispatch: the context menu opens at the cursor");
    }
    if let Some(map) = &last.map {
        ui.label(format!(
            "Map: dragging {} · selected this dispatch {} · context menu requested {} · \
             selection point (stage) ({:.0}, {:.0})",
            yes_no(map.dragging),
            yes_no(map.selected),
            yes_no(map.context_menu),
            map.selection.x,
            map.selection.y
        ));
    }
}

fn delivery_readout(ui: &mut Ui, delivery: &MapDelivery) {
    ui.label(format!(
        "Delivered: UV ({:.3}, {:.3}){} · primary {} · secondary {} · zoom {}{}",
        delivery.uv.x,
        delivery.uv.y,
        if delivery.held_at_press {
            format!(" held at the press point (slop {CLICK_SLOP} UV)")
        } else {
            String::new()
        },
        button_label(
            delivery.primary,
            delivery.primary_pressed,
            delivery.primary_released
        ),
        button_label(
            delivery.secondary,
            delivery.secondary_pressed,
            delivery.secondary_released
        ),
        zoom_label(delivery.zoom),
        if delivery.cancelled {
            " · cancellation"
        } else {
            ""
        },
    ));
}

fn route_label(route: Option<RoutedPointer>) -> String {
    match route {
        None => "the mod does not route the UI mouse".to_owned(),
        Some(RoutedPointer::Parked) => "parked".to_owned(),
        Some(RoutedPointer::Controller { uv, buttons }) => {
            format!(
                "controller at ({:.3}, {:.3}), buttons {buttons:#b}",
                uv.0, uv.1
            )
        }
        Some(RoutedPointer::Desktop { buttons }) => format!("desktop mouse, buttons {buttons:#b}"),
    }
}

fn button_label(held: bool, pressed: bool, released: bool) -> &'static str {
    match (pressed, released, held) {
        (true, _, _) => "clicked",
        (_, true, _) => "released",
        (_, _, true) => "held",
        _ => "up",
    }
}
