//! Giving weapons: for each body socket, a choice of the hand weapons the game ships that fit it,
//! given straight onto the socket, and a clear; and the grenade stock set or emptied. The requests
//! go to the inventory's give ([`give`]), which carries them out on the next input tick.

use egui::{Button, ComboBox, Grid, Ui};
use parking_lot::Mutex;

use crate::controllers::{
    loadout::{LoadoutSnapshot, Socket},
    weapons::{
        c4,
        grenade::stock,
        inventory::restore::give::{self, GiveRequest},
    },
};

pub(super) fn give_section(ui: &mut Ui, table: Option<&LoadoutSnapshot>) {
    let Some(table) = table else {
        ui.label("Not running: no player yet, or the VR inventory is off.");
        return;
    };
    ui.label(
        "A give spawns the weapon and puts it on the socket, or on the next free socket that \
         fits it; a socket that holds a weapon drops it first.",
    );
    let mut chosen = CHOSEN.lock();
    Grid::new("inventory_give").striped(true).show(ui, |ui| {
        for header in ["Socket", "Holds", "Weapon", "", ""] {
            ui.strong(header);
        }
        ui.end_row();
        for socket in Socket::ALL {
            let choice = &mut chosen[socket.index()];
            ui.label(socket.label());
            ui.label(table.docked(socket).map_or_else(
                || "nothing".to_owned(),
                |item| super::table::kind_label(item.kind),
            ));
            ComboBox::from_id_salt(("inventory_give_weapon", socket.index()))
                .selected_text(choice.unwrap_or("choose a weapon"))
                .width(260.0)
                .show_ui(ui, |ui| {
                    for def in give::weapons_for(socket) {
                        ui.selectable_value(choice, Some(def.name.as_str()), def.name.as_str());
                    }
                });
            if ui
                .add_enabled(choice.is_some(), Button::new("Give"))
                .clicked()
                && let Some(name) = choice
            {
                give::request(GiveRequest::Weapon {
                    socket,
                    name: (*name).to_owned(),
                });
            }
            if ui
                .add_enabled(table.docked(socket).is_some(), Button::new("Clear"))
                .clicked()
            {
                give::request(GiveRequest::Clear(socket));
            }
            ui.end_row();
        }
        ui.label("Grenades");
        ui.label(stock::last().map_or_else(|| "unknown".to_owned(), |s| s.count().to_string()));
        ui.label(match stock::capacity() {
            Some(capacity) => format!("up to {capacity}"),
            None => "uncapped".to_owned(),
        });
        if ui.button("Fill").clicked() {
            give::request(GiveRequest::Grenades(
                stock::capacity().unwrap_or(UNCAPPED_FILL),
            ));
        }
        if ui.button("Clear").clicked() {
            give::request(GiveRequest::Grenades(0));
        }
        ui.end_row();
        ui.label("C4");
        ui.label(
            c4::charges().map_or_else(|| "unknown".to_owned(), |live| format!("{live} placed")),
        );
        ui.label("no stock: the game caps the charges placed at once");
        ui.end_row();
    });
    if let Some(outcome) = give::last_outcome() {
        ui.label(outcome);
    }
}

/// The grenades a fill gives an uncapped stock.
const UNCAPPED_FILL: i32 = 10;

/// Each socket's chosen weapon, by [`Socket::index`].
static CHOSEN: Mutex<[Option<&'static str>; Socket::COUNT]> = Mutex::new([None; Socket::COUNT]);
