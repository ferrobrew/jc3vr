//! The VR inventory: the carried weapons, the weapons lying in the world and their pins, and its
//! switches.

use std::sync::Arc;

use egui::Ui;

use crate::{
    controllers::{
        config::WeaponsConfig,
        loadout::{slot_label, spawn_transport},
        weapons::inventory::{
            InventoryReadout, StatusUnavailable,
            restore::{PathStatus, RestoreReadout},
            traversal::Task,
        },
    },
    ui::controllers::pouch_models::pouch_model_settings,
};

pub(super) fn inventory_readout(
    ui: &mut Ui,
    inventory: Result<Arc<InventoryReadout>, StatusUnavailable>,
    restore: Result<Arc<RestoreReadout>, StatusUnavailable>,
) {
    let transport = spawn_transport::controller().status();
    ui.label(format!(
        "Source-member spawn admission: {:?}; native callbacks {}, source qualification {}, ammunition {}, staging {}.",
        transport.admission, transport.native_callbacks, transport.source_qualification,
        transport.ammunition, transport.staging,
    ));
    match restore {
        Ok(readout) => {
            let r = &readout.status;
            ui.label(format!(
                "Restore: last published {:.1} s ago; values can be stale.",
                readout.published_at.elapsed().as_secs_f32()
            ));
            let path = match readout.path {
                PathStatus::Uninitialized => "not resolved by a store or a load yet".to_owned(),
                PathStatus::Unavailable => "unavailable: no configuration directory".to_owned(),
                PathStatus::Available => r.path.as_ref().map_or_else(
                    || "unavailable: no published path".to_owned(),
                    |path| path.display().to_string(),
                ),
            };
            ui.label(format!(
                "Save sidecar: {} · writes {} · carried in the last {} · last write error: {}",
                path,
                r.writes,
                r.last_written,
                r.last_write_error.as_deref().unwrap_or("none")
            ));
            ui.label(format!(
                "Last load: {} · last respawn: {} · restored {} · spawning {}",
                r.last_load
                    .as_ref()
                    .map_or("none".to_owned(), ToString::to_string),
                r.last_respawn.map_or("none".to_owned(), |o| format!(
                    "{:?}, {} of {} restored, {} failed",
                    o.origin, o.restored, o.requested, o.failed
                )),
                r.restored,
                r.spawning
            ));
        }
        Err(reason) => {
            unavailable_readout(ui, "Restore", reason);
        }
    }
    let readout = match inventory {
        Ok(readout) => readout,
        Err(reason) => {
            unavailable_readout(ui, "Inventory", reason);
            return;
        }
    };
    let s = &readout.status;
    ui.label(format!(
        "Inventory: last published {:.1} s ago; values can be stale.",
        readout.published_at.elapsed().as_secs_f32()
    ));
    ui.label(format!(
        "Active: {} · {} on the body · {} of {} in the world kept · pins {} · left to the game {} \
         · resets {}",
        super::yes_no(s.active),
        s.owned,
        s.pinned(),
        s.world.len(),
        s.pins,
        s.forgotten,
        s.resets
    ));
    let l = &readout.lost;
    ui.label(format!(
        "Lost weapons: speed {:.1} m/s{} · returned out of the world {}, left behind {}, to the feet {}",
        l.speed_mps,
        if l.fast { " (fast)" } else { "" },
        l.out_of_world,
        l.left_behind,
        l.to_feet
    ));
    let t = &readout.traversal;
    ui.label(format!(
        "Traversal: {} · refused {} · forced neutral states {}, kept in the hand {}",
        if t.keeping { "kept" } else { "the game's" },
        Task::ALL
            .iter()
            .map(|task| format!("{} {}", task.label(), t.refused[task_index(*task)]))
            .collect::<Vec<_>>()
            .join(", "),
        t.neutral,
        t.neutral_kept
    ));
    ui.label(format!(
        "Swimming pickups that kept the slot's weapon: {}",
        readout.swim_vacates
    ));
    ui.label(format!(
        "Carried: {}{} · kept from pickups {} · with others {}",
        s.carried.len(),
        if s.stashed {
            " (stashed for a mission)"
        } else {
            ""
        },
        s.carried_replaced,
        s.with_others
    ));
    if let Some(reason) = readout.member_closure {
        ui.label(format!(
            "Paired rack-member requests disabled until the game restarts: {}. Ordinary game gives remain enabled.",
            reason.label()
        ));
    }
    let p = &readout.single;
    ui.label(format!(
        "Single pistols: {} · active {} · slot's single {} · second pistol {:?} · split {} · \
         arrived as one {} · taken as one {} · pairs kept {}",
        p.singles,
        super::yes_no(p.active),
        super::yes_no(p.slot_single),
        p.twin,
        p.splits,
        p.ones,
        p.picked,
        p.pairs
    ));
    if let Some(last) = &p.last {
        ui.label(format!("Last: {last}"));
    }
    for (slot, socket) in &s.carried {
        ui.label(format!(
            "Carried: {} on the {}",
            slot_label(*slot),
            socket.label()
        ));
    }
    for weapon in &s.world {
        ui.label(format!(
            "In the world: object {}{}{}{}",
            weapon.object,
            if weapon.pinned { " · kept" } else { "" },
            if readout.world_singles.contains(&weapon.object) {
                " · single pistol"
            } else {
                ""
            },
            weapon
                .distance_m
                .map_or(String::new(), |d| format!(" · {d:.1} m away"))
        ));
    }
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

fn unavailable_readout(ui: &mut Ui, owner: &str, reason: StatusUnavailable) {
    let reason = match reason {
        StatusUnavailable::Uninitialized => "uninitialized: no published status yet",
        StatusUnavailable::Busy => "busy: the publication is being replaced",
    };
    ui.label(format!("{owner} status unavailable: {reason}."));
}

fn task_index(task: Task) -> usize {
    Task::ALL.iter().position(|t| *t == task).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use egui::{Context, RawInput};

    use super::*;

    #[test]
    fn unavailable_readout_renders_without_querying_owners_or_actions() {
        let context = Context::default();
        let output = context.run_ui(RawInput::default(), |ui| {
            inventory_readout(
                ui,
                Err(StatusUnavailable::Busy),
                Err(StatusUnavailable::Uninitialized),
            );
        });
        assert!(!output.shapes.is_empty());
        let source = include_str!("inventory.rs");
        let readout = source
            .split("pub(super) fn inventory_readout(")
            .nth(1)
            .unwrap()
            .split("pub(super) fn inventory_settings(")
            .next()
            .unwrap();
        for forbidden in [
            "Config::",
            "paths::",
            "lost::status",
            "single::status",
            "single::is",
            "use_item::",
            "restore::status",
        ] {
            assert!(
                !readout.contains(forbidden),
                "the readout contains {forbidden}"
            );
        }
    }
}
