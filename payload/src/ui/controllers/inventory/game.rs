//! The game's own inventory: each slot's weapon with its class, state, and ammunition, beside what
//! the table projects into the slot, then the selected slot, the wielded weapon, the pistol slot,
//! and the grenade and explosive stock.

use egui::{Grid, RichText, Ui};
use jc3gi::equipment::weapon::{WeaponClass, WeaponState};

use crate::{
    controllers::{
        loadout::{LoadoutSnapshot, SlotContent, slot_label},
        weapons::inventory::{
            InventoryReadout,
            game_view::{GameSlot, Pistols},
        },
    },
    ui::controllers::inventory::mismatch::Mismatches,
};

pub(super) fn game_section(
    ui: &mut Ui,
    readout: &InventoryReadout,
    table: Option<&LoadoutSnapshot>,
    mismatches: &Mismatches,
) {
    let Some(game) = &readout.game else {
        ui.label("Not read yet: no player yet.");
        return;
    };
    ui.label(format!(
        "Published {:.1} s ago.",
        readout.published_at.elapsed().as_secs_f32()
    ));
    Grid::new("inventory_game_slots")
        .striped(true)
        .show(ui, |ui| {
            for header in [
                "Slot", "Weapon", "Class", "State", "Magazine", "Reserve", "Object", "Table",
            ] {
                ui.strong(header);
            }
            ui.end_row();
            for slot in &game.slots {
                slot_row(ui, slot, table, mismatches.slot(slot.slot));
            }
        });
    let wielded = game
        .wielded
        .map_or_else(|| "nothing".to_owned(), |object| object.to_string());
    let wielded = format!(
        "Selected: {} · wielded: {wielded}",
        game.selected.map_or("none", slot_label)
    );
    if mismatches.wielded {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("{wielded} (not the primary hand's weapon)"),
        );
    } else {
        ui.label(wielded);
    }
    ui.label(format!(
        "Pistol slot: {} · grenades {} · explosives {}",
        match game.pistols {
            Pistols::Empty => "empty",
            Pistols::Pair => "the game's pair",
            Pistols::Single => "a single pistol",
        },
        game.grenades,
        game.explosives,
    ));
}

fn slot_row(ui: &mut Ui, slot: &GameSlot, table: Option<&LoadoutSnapshot>, mismatch: bool) {
    let warn = ui.visuals().warn_fg_color;
    let cell = |ui: &mut Ui, text: String| {
        let text = RichText::new(text);
        ui.label(if mismatch { text.color(warn) } else { text });
    };
    let name = match (slot.weapon, slot.pending_spawn) {
        (None, true) => "spawning".to_owned(),
        (None, false) => "empty".to_owned(),
        (Some(weapon), _) => format!("{:#010x}", weapon.name_hash),
    };
    cell(ui, slot_label(slot.slot).to_owned());
    cell(ui, name);
    match slot.weapon {
        Some(weapon) => {
            cell(ui, class_label(weapon.class).to_owned());
            cell(ui, state_label(weapon.state).to_owned());
            cell(ui, weapon.magazine.to_string());
            cell(ui, weapon.reserve.map_or("-".to_owned(), |r| r.to_string()));
            cell(ui, weapon.object.to_string());
        }
        None => {
            for _ in 0..5 {
                cell(ui, String::new());
            }
        }
    }
    cell(ui, projected(table, slot));
    ui.end_row();
}

/// What the table projects into `slot`: an item, nothing, or the slot left to the game.
fn projected(table: Option<&LoadoutSnapshot>, slot: &GameSlot) -> String {
    let Some(table) = table else {
        return "not running".to_owned();
    };
    let content = table
        .projection
        .slots
        .iter()
        .find(|fill| fill.slot == slot.slot)
        .map(|fill| fill.content);
    match content {
        Some(SlotContent::Weapon { item, .. }) => item.to_string(),
        Some(SlotContent::Empty) => "empty".to_owned(),
        Some(SlotContent::Game) | None => "the game's".to_owned(),
    }
}

/// The name of the raw weapon class `raw`.
fn class_label(raw: i32) -> &'static str {
    const CLASSES: [(WeaponClass, &str); 14] = [
        (WeaponClass::E_WEAPON_CLASS_NONE, "none"),
        (WeaponClass::E_WEAPON_CLASS_PISTOL, "pistol"),
        (WeaponClass::E_WEAPON_CLASS_SMG, "submachine gun"),
        (WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE, "assault rifle"),
        (WeaponClass::E_WEAPON_CLASS_SHOTGUN, "shotgun"),
        (WeaponClass::E_WEAPON_CLASS_SNIPER, "sniper"),
        (
            WeaponClass::E_WEAPON_CLASS_GRENADE_LAUNCHER,
            "grenade launcher",
        ),
        (WeaponClass::E_WEAPON_CLASS_RPG, "rocket launcher"),
        (WeaponClass::E_WEAPON_CLASS_MINIGUN, "minigun"),
        (WeaponClass::E_WEAPON_CLASS_AA_GUN, "anti-air gun"),
        (WeaponClass::E_WEAPON_CLASS_MORTAR, "mortar"),
        (WeaponClass::E_WEAPON_CLASS_50CAL, ".50 cal"),
        (WeaponClass::E_WEAPON_CLASS_SPECIAL, "special"),
        (WeaponClass::E_WEAPON_CLASS_REVOLVER, "revolver"),
    ];
    CLASSES
        .iter()
        .find(|(class, _)| *class as i32 == raw)
        .map_or("unknown", |(_, label)| label)
}

/// The name of the raw weapon state `raw`.
fn state_label(raw: i32) -> &'static str {
    const STATES: [(WeaponState, &str); 11] = [
        (WeaponState::E_WS_DROPPED, "dropped"),
        (WeaponState::E_WS_WIELDING, "wielding"),
        (WeaponState::E_WS_DUAL_WIELDING, "dual wielding"),
        (WeaponState::E_WS_HOLSTERED, "holstered"),
        (WeaponState::E_WS_HOLSTERED_INVISIBLE, "holstered, hidden"),
        (WeaponState::E_WS_MOUNTED, "mounted"),
        (WeaponState::E_WS_TURRET, "turret"),
        (WeaponState::E_WS_VEHICLE, "vehicle"),
        (WeaponState::E_WS_PICKING_UP, "picking up"),
        (WeaponState::E_WS_WEAPON_RACK, "on a rack"),
        (WeaponState::E_WS_WINGSUIT_WEAPON, "wingsuit"),
    ];
    STATES
        .iter()
        .find(|(state, _)| *state as i32 == raw)
        .map_or("unknown", |(_, label)| label)
}
