//! What each hand holds: the table's item, and every grip source declaring the hand, which names
//! what the hand holds beyond the table (a loose prop, a magazine, a door or a handle, a parachute
//! handle, or a climbing hold), with the physical hands' detail of a held object or door.

use egui::{Grid, Ui};

use crate::{
    controllers::{
        fingers::grip::GripSource, frame::Hand, loadout::LoadoutSnapshot,
        physical_hands::PhysicalHandsStatus,
    },
    ui::controllers::inventory::table::kind_label,
};

pub(super) fn hands_section(
    ui: &mut Ui,
    table: Option<&LoadoutSnapshot>,
    physical: &PhysicalHandsStatus,
    grips: &[Vec<GripSource>; 2],
) {
    if let Some(idle) = physical.idle {
        ui.label(format!("The physical hands are idle: {}.", idle.label()));
    }
    Grid::new("inventory_hands").striped(true).show(ui, |ui| {
        for header in ["Hand", "Table", "Grip sources", "Detail"] {
            ui.strong(header);
        }
        ui.end_row();
        for hand in Hand::BOTH {
            ui.label(hand.label());
            match table.and_then(|table| table.held(hand)) {
                Some(item) => ui.label(format!("{} {}", item.id, kind_label(item.kind))),
                None => ui.weak("nothing"),
            };
            let sources = &grips[hand.index()];
            if sources.is_empty() {
                ui.weak("none");
            } else {
                ui.label(
                    sources
                        .iter()
                        .map(|source| source.label())
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            ui.label(detail(physical, hand));
            ui.end_row();
        }
    });
}

/// The physical hands' detail of what `hand` holds: a loose object's mass and hold, or a latched
/// door.
fn detail(physical: &PhysicalHandsStatus, hand: Hand) -> String {
    let status = &physical.hands[hand.index()];
    let mut parts = Vec::new();
    if let Some(held) = &status.held {
        let mut object = format!("an object of {:.1} kg", held.mass_kg);
        if held.joined {
            object.push_str(", through a joint");
        }
        if held.shared {
            object.push_str(", with the other hand");
        }
        if !held.liftable {
            object.push_str(", too heavy to lift");
        }
        parts.push(object);
    }
    if status.latched {
        parts.push("a latched door".to_owned());
    }
    parts.join("; ")
}
