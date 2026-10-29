//! The live inventory table and its slot and store projections.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    loadout::{LoadoutSnapshot, SlotContent, StoreProjection, live::LiveStatus, slot_label},
};

pub(super) fn live_readout(ui: &mut Ui, live: &LiveStatus) {
    ui.label(
        "The live table, written beside the old weapon handling, which still moves the weapons; \
         only the pins of the weapons lying in the world read it yet.",
    );
    match &live.snapshot {
        None => {
            ui.label("Not running: no player yet, or the VR inventory is off.");
        }
        Some(snapshot) => {
            ui.label(format!(
                "{} transitions and {} refusals since the payload loaded",
                live.transitions, live.refusals,
            ));
            table_readout(ui, snapshot);
            if let Some(store) = &live.last_store {
                store_readout(ui, store);
            }
            for line in &live.log {
                ui.label(line);
            }
        }
    }
}

/// The table's items, its primary hand, and its slots in play.
fn table_readout(ui: &mut Ui, snapshot: &LoadoutSnapshot) {
    ui.label(format!(
        "Dominant hand: {} · primary: {} · {} items",
        snapshot.body.dominant().label(),
        snapshot.primary.map_or("none", Hand::label),
        snapshot.items.len(),
    ));
    for (index, item) in snapshot.items.iter().enumerate() {
        ui.label(format!(
            "{index}: {} {} · in {} · {}",
            item.id, item.kind, item.place, item.binding
        ));
    }
    let projection = &snapshot.projection;
    ui.label(format!(
        "Slots in play (selected: {}): {}",
        projection.selected.map_or("none", slot_label),
        projection
            .slots
            .iter()
            .filter_map(|fill| match fill.content {
                SlotContent::Weapon { item, .. } =>
                    Some(format!("{} {item}", slot_label(fill.slot))),
                SlotContent::Empty => Some(format!("{} empty", slot_label(fill.slot))),
                SlotContent::Game => None,
            })
            .collect::<Vec<_>>()
            .join(" · ")
    ));
}

/// A store's slots and sidecar.
fn store_readout(ui: &mut Ui, store: &StoreProjection) {
    ui.label(format!(
        "Last store (selected: {}): {}",
        store.projection.selected.map_or("none", slot_label),
        store
            .sidecar
            .iter()
            .map(|entry| format!(
                "{} {} in {:?}, {}",
                entry.item,
                entry.weapon,
                entry.place,
                entry.slot.map_or("beyond the slots", slot_label)
            ))
            .collect::<Vec<_>>()
            .join(" · ")
    ));
}
