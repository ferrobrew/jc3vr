//! The VR inventory's feature: its switches, and a read-only view of three sources side by side,
//! each read from its owner's publication: the game's own inventory ([`game`]), the live loadout
//! table, which places the player's weapons ([`table`]), and what each hand holds ([`hands`]).
//! Where the game's slots and the table disagree ([`mismatch`]) the rows are marked.

mod game;
mod hands;
mod mismatch;
mod table;

use std::{sync::Arc, time::Instant};

use egui::{CollapsingHeader, Ui};

use crate::{
    controllers::{
        config::WeaponsConfig,
        fingers::grip::{self, GripSource},
        frame::Hand,
        loadout::live::{self, LiveStatus},
        physical_hands::{self, PhysicalHandsStatus},
        weapons::inventory::{self as controller_inventory, InventoryReadout, StatusUnavailable},
    },
    ui::controllers::{inventory::mismatch::Mismatches, pouch_models::pouch_model_settings},
};

/// Everything the inventory view shows, read from the publications before it is drawn.
pub(super) struct InventoryView {
    inventory: Result<Arc<InventoryReadout>, StatusUnavailable>,
    live: LiveStatus,
    physical: PhysicalHandsStatus,
    /// Each hand's declared grip sources, by [`Hand::index`], the winning one first.
    grips: [Vec<GripSource>; 2],
}

impl InventoryView {
    pub(super) fn read() -> Self {
        let now = Instant::now();
        Self {
            inventory: controller_inventory::peek_status(),
            live: live::status(),
            physical: physical_hands::status(),
            grips: Hand::BOTH.map(|hand| grip::declared(hand, now)),
        }
    }
}

pub(super) fn inventory_view(ui: &mut Ui, view: &InventoryView) {
    let inventory = view.inventory.as_ref().ok().map(Arc::as_ref);
    let game = inventory.and_then(|readout| readout.game.as_ref());
    let table = view.live.snapshot.as_deref();
    let mismatches = match (game, table) {
        (Some(game), Some(table)) => Mismatches::find(game, table),
        _ => Mismatches::default(),
    };
    if mismatches.any() {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "The game's inventory disagrees with the table on the marked rows.",
        );
    }
    CollapsingHeader::new("The game's inventory")
        .default_open(true)
        .show(ui, |ui| match &view.inventory {
            Ok(readout) => game::game_section(ui, readout, table, &mismatches),
            Err(reason) => unavailable(ui, *reason),
        });
    CollapsingHeader::new("The loadout table")
        .default_open(true)
        .show(ui, |ui| table::table_section(ui, &view.live, inventory));
    CollapsingHeader::new("The hands")
        .default_open(true)
        .show(ui, |ui| {
            hands::hands_section(ui, table, &view.physical, &view.grips)
        });
}

pub(super) fn inventory_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    let c = &mut w.inventory;
    ui.checkbox(&mut c.enabled, "Track the player's weapons");
    ui.add_enabled_ui(c.enabled, |ui| {
        ui.checkbox(
            &mut c.project_slots,
            "The game's slots follow the hands and the body",
        )
        .on_hover_text(
            "The loadout table alone moves the weapons, and the game's slots, its wielded \
             weapon, and the weapons armed in the hands follow it. Off, the table is put back \
             into the slots and the game's own one-weapon model runs.",
        );
        ui.checkbox(&mut c.return_lost, "Bring lost weapons back")
            .on_hover_text(
                "A weapon that fell out of the world, or that was let go of at speed and left \
                 behind, comes back to a free socket, else the feet.",
            );
        ui.checkbox(&mut c.carry, "Carry weapons beyond the slots on the body");
        ui.checkbox(
            &mut c.persist,
            "Keep the carried weapons across saves and loads",
        );
    });
    pouch_model_settings(ui, &mut w.pouches.models);
}

fn unavailable(ui: &mut Ui, reason: StatusUnavailable) {
    ui.label(match reason {
        StatusUnavailable::Uninitialized => "Not published yet: no player yet.",
        StatusUnavailable::Busy => "Busy: the publication is being replaced.",
    });
}
