//! The live loadout table: each hand, each body socket with its docked item, the weapons lying in
//! the world that the pins keep, and the safe items. Each weapon on the body is marked with the
//! game's slot that holds it, or as one of the carry set's, beyond the slots.

use egui::{CollapsingHeader, Grid, Ui};

use crate::controllers::{
    frame::Hand,
    loadout::{
        Binding, Departure, ItemKind, ItemView, Place, Socket, live::LiveStatus, slot_label,
    },
    weapons::inventory::{InventoryReadout, game_view::GameInventory},
};

pub(super) fn table_section(ui: &mut Ui, live: &LiveStatus, inventory: Option<&InventoryReadout>) {
    let Some(table) = live.snapshot.as_deref() else {
        ui.label("Not running: no player yet, or the VR inventory is off.");
        return;
    };
    ui.label(format!(
        "Dominant hand: {} · primary: {} · {} transitions and {} refusals since the payload loaded",
        table.body.dominant().label(),
        table.primary.map_or("none", Hand::label),
        live.transitions,
        live.refusals,
    ));
    let game = inventory.and_then(|readout| readout.game.as_ref());
    if inventory.is_some_and(|readout| readout.status.stashed) {
        ui.label("The carried weapons are stashed while a mission holds the player's weapons.");
    }
    Grid::new("inventory_table_items")
        .striped(true)
        .show(ui, |ui| {
            for header in ["Place", "Item", "Kind", "Object", "Note"] {
                ui.strong(header);
            }
            ui.end_row();
            for hand in Hand::BOTH {
                let place = format!("{} hand", hand.label());
                let item = table.held(hand);
                item_row(ui, &place, item, body_note(item, game));
            }
            for socket in Socket::ALL {
                let item = table.docked(socket);
                item_row(ui, socket.label(), item, body_note(item, game));
            }
            for item in &table.items {
                if let Place::World(departure) = item.place {
                    item_row(
                        ui,
                        "world",
                        Some(item),
                        world_note(item, departure, inventory),
                    );
                }
            }
        });
    CollapsingHeader::new("Recent transitions")
        .id_salt("inventory_table_log")
        .show(ui, |ui| {
            if live.log.is_empty() {
                ui.label("None yet.");
            }
            for line in &live.log {
                ui.label(line);
            }
        });
}

fn item_row(ui: &mut Ui, place: &str, item: Option<&ItemView>, note: String) {
    ui.label(place);
    match item {
        Some(item) => {
            ui.label(item.id.to_string());
            ui.label(kind_label(item.kind));
            ui.label(match item.binding {
                Binding::Live(object) => object.to_string(),
                other => other.to_string(),
            });
            let safe = matches!(item.binding, Binding::SafeItem(_));
            ui.label(match (safe, note.is_empty()) {
                (true, true) => "safe item".to_owned(),
                (true, false) => format!("safe item, {note}"),
                (false, _) => note,
            });
        }
        None => {
            ui.weak("empty");
            for _ in 0..3 {
                ui.label("");
            }
        }
    }
    ui.end_row();
}

/// Which of the game's slots holds a weapon on the body, or that the carry set keeps it beyond the
/// slots.
fn body_note(item: Option<&ItemView>, game: Option<&GameInventory>) -> String {
    let (Some(item), Some(game)) = (item, game) else {
        return String::new();
    };
    let Some(object) = item
        .binding
        .address()
        .filter(|_| item.kind.weapon().is_some())
    else {
        return String::new();
    };
    game.slots
        .iter()
        .find(|slot| slot.weapon.is_some_and(|weapon| weapon.object == object))
        .map_or_else(
            || "beyond the slots (carry set)".to_owned(),
            |slot| format!("in the {} slot", slot_label(slot.slot)),
        )
}

/// How a weapon lying in the world came there, whether the pins keep it, and how far it is.
fn world_note(
    item: &ItemView,
    departure: Departure,
    inventory: Option<&InventoryReadout>,
) -> String {
    let mut note = departure.to_string();
    let object = item.binding.address();
    let world = inventory.and_then(|readout| {
        readout
            .status
            .world
            .iter()
            .find(|weapon| Some(weapon.object) == object)
    });
    if let Some(weapon) = world {
        note.push_str(if weapon.pinned {
            ", kept"
        } else {
            ", left to the game"
        });
        if let Some(distance) = weapon.distance_m {
            note.push_str(&format!(", {distance:.1} m away"));
        }
    }
    if inventory.is_some_and(|readout| object.is_some_and(|o| readout.world_singles.contains(&o))) {
        note.push_str(", single pistol");
    }
    note
}

/// A short name for what an item is.
pub(super) fn kind_label(kind: ItemKind) -> String {
    match kind {
        ItemKind::Weapon(weapon) => {
            format!("{} ({})", slot_label(weapon.slot()), weapon.size().label())
        }
        ItemKind::Prop => "prop".to_owned(),
        ItemKind::Grenade => "grenade".to_owned(),
        ItemKind::Charge => "C4 charge".to_owned(),
    }
}
