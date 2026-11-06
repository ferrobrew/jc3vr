//! The loadout as a live view to edit: one dropdown per hand and per body socket, whose selected
//! value is what is there now, by the weapon's name, or nothing. Choosing another weapon gives it
//! there, and choosing nothing clears the socket or lets go of the hand's weapon; a definition a
//! give has shown cannot be spawned is greyed, with the reason on hover. The grenade stock and the
//! placed C4 charges have their own rows. The requests go to the inventory's give ([`give`]), which
//! carries them out on the next input tick. Below, what each hand's grip holds beyond the table,
//! the weapons the table keeps lying in the world, and the table's recent transitions.

use egui::{Button, CollapsingHeader, ComboBox, Grid, Ui};

use crate::{
    controllers::{
        fingers::grip::GripSource,
        frame::Hand,
        loadout::{
            Binding, Departure, ItemKind, ItemView, LoadoutSnapshot, Place, Socket,
            live::LiveStatus, slot_label,
        },
        physical_hands::PhysicalHandsStatus,
        weapons::{
            c4,
            grenade::stock,
            inventory::{
                InventoryReadout,
                game_view::GameInventory,
                restore::give::{self, GiveRequest, GiveTarget},
            },
        },
    },
    director::spawn::SpawnDefinition,
};

/// What the loadout section reads besides the table.
pub(super) struct LoadoutInputs<'a> {
    pub live: &'a LiveStatus,
    pub inventory: Option<&'a InventoryReadout>,
    pub physical: &'a PhysicalHandsStatus,
    /// Each hand's declared grip sources, by [`Hand::index`], the winning one first.
    pub grips: &'a [Vec<GripSource>; 2],
}

pub(super) fn loadout_section(ui: &mut Ui, inputs: &LoadoutInputs) {
    let Some(table) = inputs.live.snapshot.as_deref() else {
        ui.label("Not running: no player yet, or the VR inventory is off.");
        return;
    };
    ui.label(format!(
        "Dominant hand: {} · primary: {} · {} transitions since the payload loaded",
        table.body.dominant().label(),
        table.primary.map_or("none", Hand::label),
        inputs.live.transitions,
    ));
    if let Some(idle) = inputs.physical.idle {
        ui.label(format!("The physical hands are idle: {}.", idle.label()));
    }
    let game = inputs.inventory.and_then(|readout| readout.game.as_ref());
    Grid::new("inventory_loadout").striped(true).show(ui, |ui| {
        for header in ["Place", "Holds", "Note"] {
            ui.strong(header);
        }
        ui.end_row();
        for hand in Hand::BOTH {
            let item = table.held(hand);
            ui.label(format!("{} hand", hand.label()))
                .on_hover_text(HAND_HOVER);
            choice(
                ui,
                GiveTarget::Hand(hand),
                item,
                game,
                give::weapons_for_hand(),
            );
            ui.label(join(&[
                pending_note(GiveTarget::Hand(hand)),
                hand_note(inputs, hand),
            ]));
            ui.end_row();
        }
        for socket in Socket::ALL {
            let item = table.docked(socket);
            ui.label(socket.label());
            choice(
                ui,
                GiveTarget::Socket(socket),
                item,
                game,
                give::weapons_for(socket),
            );
            ui.label(join(&[
                pending_note(GiveTarget::Socket(socket)),
                body_note(item, game),
            ]));
            ui.end_row();
        }
        grenade_row(ui);
        charge_row(ui);
    });
    ui.label(
        "A give spawns the weapon and puts it there; a socket that holds a weapon drops it first, \
         where it can be picked up.",
    );
    if let Some(outcome) = give::last_outcome() {
        ui.label(outcome);
    }
    world_section(ui, table, inputs.inventory);
    CollapsingHeader::new("Recent transitions")
        .id_salt("inventory_table_log")
        .show(ui, |ui| {
            if inputs.live.log.is_empty() {
                ui.label("None yet.");
            }
            for line in &inputs.live.log {
                ui.label(line);
            }
        });
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

const HAND_HOVER: &str = "A weapon given to a hand comes onto a socket that fits it, and the hand \
                          draws it off there if its grip is closed when it arrives; otherwise it \
                          stays on the socket. Nothing lets go of the hand's weapon.";

/// The dropdown for `target`, showing `item` and offering nothing and each of `offered`.
fn choice(
    ui: &mut Ui,
    target: GiveTarget,
    item: Option<&ItemView>,
    game: Option<&GameInventory>,
    offered: Vec<&'static SpawnDefinition>,
) {
    let current = item.map(|item| current_name(item, game));
    let current_hash = item.and_then(|item| name_hash(item, game));
    let salt = match target {
        GiveTarget::Hand(hand) => ("inventory_hand", hand.index()),
        GiveTarget::Socket(socket) => ("inventory_socket", socket.index()),
    };
    ComboBox::from_id_salt(salt)
        .selected_text(current.as_deref().unwrap_or("nothing"))
        .width(260.0)
        .show_ui(ui, |ui| {
            if ui
                .add_enabled(
                    item.is_some(),
                    Button::selectable(item.is_none(), "nothing"),
                )
                .clicked()
            {
                give::request(match target {
                    GiveTarget::Hand(hand) => GiveRequest::LetGo(hand),
                    GiveTarget::Socket(socket) => GiveRequest::Clear(socket),
                });
            }
            for def in offered {
                let label = give::display_name(def);
                let selected = current_hash == Some(def.name_hash());
                match give::refusal(&def.name) {
                    Some(why) => {
                        ui.add_enabled(false, Button::selectable(selected, label))
                            .on_disabled_hover_text(why);
                    }
                    None => {
                        let response = ui
                            .add(Button::selectable(selected, label))
                            .on_hover_text(def.name.as_str());
                        if response.clicked() && !selected {
                            let name = def.name.clone();
                            give::request(match target {
                                GiveTarget::Hand(hand) => GiveRequest::Hand { hand, name },
                                GiveTarget::Socket(socket) => GiveRequest::Weapon { socket, name },
                            });
                        }
                    }
                }
            }
        });
}

/// The name of what `item` is: a weapon's display name as the game names it, else its
/// definition's name, else its kind.
fn current_name(item: &ItemView, game: Option<&GameInventory>) -> String {
    let weapon = item
        .binding
        .address()
        .and_then(|object| game.and_then(|game| game.weapon(object)));
    let name = weapon.and_then(|weapon| {
        weapon.name.as_ref().map(ToString::to_string).or_else(|| {
            give::weapons_for_hand()
                .into_iter()
                .find(|def| def.name_hash() == weapon.name_hash)
                .map(|def| def.name.clone())
        })
    });
    let name = name.unwrap_or_else(|| kind_label(item.kind));
    if matches!(item.binding, Binding::SafeItem(_)) {
        format!("{name} (safe item)")
    } else {
        name
    }
}

/// The name hash of the weapon `item` is, when the game's inventory has it.
fn name_hash(item: &ItemView, game: Option<&GameInventory>) -> Option<u32> {
    let object = item.binding.address()?;
    Some(game?.weapon(object)?.name_hash)
}

/// The gives on their way to `target`.
fn pending_note(target: GiveTarget) -> String {
    let pending = give::pending(target);
    if pending.is_empty() {
        String::new()
    } else {
        format!("giving {}", pending.join(", "))
    }
}

/// Every grip source declaring `hand`, which names what the hand holds beyond the table (a loose
/// prop, a magazine, a door or a handle, a parachute handle, or a climbing hold), with the physical
/// hands' detail of a held object or door.
fn hand_note(inputs: &LoadoutInputs, hand: Hand) -> String {
    let sources = inputs.grips[hand.index()]
        .iter()
        .map(|source| source.label())
        .collect::<Vec<_>>()
        .join(", ");
    let status = &inputs.physical.hands[hand.index()];
    let mut parts = vec![if sources.is_empty() {
        String::new()
    } else {
        format!("grip: {sources}")
    }];
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
    join(&parts)
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
        .find(|slot| {
            slot.weapon
                .as_ref()
                .is_some_and(|weapon| weapon.object == object)
        })
        .map_or_else(
            || "beyond the slots (carry set)".to_owned(),
            |slot| format!("in the {} slot", slot_label(slot.slot)),
        )
}

/// The non-empty `parts`, joined.
fn join(parts: &[String]) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("; ")
}

/// The grenade stock, set to a count from the dropdown.
fn grenade_row(ui: &mut Ui) {
    ui.label("Grenades");
    let count = stock::last().map(|stock| stock.count());
    let capacity = stock::capacity();
    ComboBox::from_id_salt("inventory_grenades")
        .selected_text(count.map_or_else(|| "unknown".to_owned(), |count| count.to_string()))
        .width(260.0)
        .show_ui(ui, |ui| {
            for option in 0..=capacity.unwrap_or(UNCAPPED_FILL) {
                if ui
                    .add(Button::selectable(
                        count == Some(option),
                        option.to_string(),
                    ))
                    .clicked()
                    && count != Some(option)
                {
                    give::request(GiveRequest::Grenades(option));
                }
            }
        });
    ui.label(match capacity {
        Some(capacity) => format!("up to {capacity}"),
        None => "uncapped".to_owned(),
    });
    ui.end_row();
}

/// The placed C4 charges against their cap, removed by choosing none.
fn charge_row(ui: &mut Ui) {
    ui.label("C4");
    let placed = c4::placed();
    ComboBox::from_id_salt("inventory_c4")
        .selected_text(placed.map_or_else(
            || "unknown".to_owned(),
            |(live, max)| format!("{live} of {max} placed"),
        ))
        .width(260.0)
        .show_ui(ui, |ui| {
            let any = placed.is_none_or(|(live, _)| live > 0);
            if ui
                .add_enabled(any, Button::selectable(!any, "none placed"))
                .clicked()
            {
                give::request(GiveRequest::RemovePlacedC4);
            }
        });
    ui.label("no stock: the cap is on charges placed at once; none removes them unexploded");
    ui.end_row();
}

/// The grenades a fill offers an uncapped stock.
const UNCAPPED_FILL: i32 = 10;

/// The weapons lying in the world that the table keeps.
fn world_section(ui: &mut Ui, table: &LoadoutSnapshot, inventory: Option<&InventoryReadout>) {
    let world: Vec<(&ItemView, Departure)> = table
        .items
        .iter()
        .filter_map(|item| match item.place {
            Place::World(departure) => Some((item, departure)),
            _ => None,
        })
        .collect();
    CollapsingHeader::new(format!("In the world ({})", world.len()))
        .id_salt("inventory_world")
        .show(ui, |ui| {
            if world.is_empty() {
                ui.label("None.");
                return;
            }
            Grid::new("inventory_world_items")
                .striped(true)
                .show(ui, |ui| {
                    for header in ["Item", "Kind", "Note"] {
                        ui.strong(header);
                    }
                    ui.end_row();
                    for (item, departure) in world {
                        ui.label(item.id.to_string());
                        ui.label(kind_label(item.kind));
                        ui.label(world_note(item, departure, inventory));
                        ui.end_row();
                    }
                });
        });
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
