//! The VR inventory's feature: its status rows and two views, each read from its
//! owner's publication: the live loadout, edited in place, with one dropdown per hand and per body
//! socket that shows what is there and gives another weapon there ([`loadout`]), and the game's own
//! inventory ([`game`]). Where the game's slots and the table disagree ([`mismatch`]) the rows are
//! marked.

mod game;
mod loadout;
mod mismatch;

use std::{sync::Arc, time::Instant};

use egui::{CollapsingHeader, Ui};
use jc3vr_body::controllers::fingers::{grip, grip::GripSource};
use jc3vr_input::frame::Hand;
use jc3vr_physical::controllers::{physical_hands, physical_hands::PhysicalHandsStatus};
use jc3vr_weapons::controllers::{
    loadout::{live, live::LiveStatus},
    weapons::{
        inventory as controller_inventory,
        inventory::{InventoryReadout, StatusUnavailable},
    },
};

use crate::{
    status::inventory::{CARRIED, LAST_LOAD, LOADOUT, PINNED, SEPARATE_PISTOLS, SIDECAR, TRACKED},
    ui::{
        controllers::{
            inventory::{
                loadout::{LoadoutInputs, loadout_section},
                mismatch::Mismatches,
            },
            pouch_models::pouch_model_readout,
        },
        status_row::headlines,
    },
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
    headlines(
        ui,
        &[
            &TRACKED,
            &CARRIED,
            &SEPARATE_PISTOLS,
            &SIDECAR,
            &LAST_LOAD,
            &PINNED,
            &LOADOUT,
        ],
    );
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
    CollapsingHeader::new("The loadout")
        .default_open(true)
        .show(ui, |ui| {
            loadout_section(
                ui,
                &LoadoutInputs {
                    live: &view.live,
                    inventory,
                    physical: &view.physical,
                    grips: &view.grips,
                },
            )
        });
    CollapsingHeader::new("The game's inventory")
        .default_open(true)
        .show(ui, |ui| match &view.inventory {
            Ok(readout) => game::game_section(ui, readout, table, &mismatches),
            Err(reason) => unavailable(ui, *reason),
        });
    pouch_model_readout(ui);
}

fn unavailable(ui: &mut Ui, reason: StatusUnavailable) {
    ui.label(match reason {
        StatusUnavailable::Uninitialized => "Not published yet: no player yet.",
        StatusUnavailable::Busy => "Busy: the publication is being replaced.",
    });
}
