//! The VR inventory: the carried weapons, the weapons lying in the world and their pins, and its
//! settings.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    loadout::{Socket, slot_label, spawn_transport},
    weapons::{
        dual_wield::single,
        hold::use_item,
        inventory::{
            InventoryStatus,
            config::InventoryConfig,
            lost,
            restore::RestoreStatus,
            traversal::{self, Task},
        },
    },
};

pub(super) fn inventory_readout(ui: &mut Ui, s: &InventoryStatus, r: &RestoreStatus) {
    let transport = spawn_transport::controller().status();
    ui.label(format!(
        "Source-member spawn admission: {:?}; native callbacks {}, source qualification {}, ammunition {}, staging {}.",
        transport.admission, transport.native_callbacks, transport.source_qualification,
        transport.ammunition, transport.staging,
    ));
    ui.label(format!(
        "Save sidecar: {} · writes {} · carried in the last {} · last write error: {}",
        r.path
            .as_ref()
            .map_or("no configuration directory".to_owned(), |p| p
                .display()
                .to_string()),
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
    let l = lost::status();
    ui.label(format!(
        "Lost weapons: speed {:.1} m/s{} · returned out of the world {}, left behind {}, to the feet {}",
        l.speed_mps,
        if l.fast { " (fast)" } else { "" },
        l.out_of_world,
        l.left_behind,
        l.to_feet
    ));
    let t = traversal::status();
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
        use_item::swim_vacates()
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
    let p = single::status();
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
            if single::is(weapon.object.get()) {
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

pub(super) fn inventory_settings(ui: &mut Ui, c: &mut InventoryConfig) {
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
        ui.checkbox(
            &mut c.keep_through_traversal,
            "Keep the hands as they are through traversal",
        )
        .on_hover_text(
            "Vehicles, seats, the wingsuit, falls, dives, and the ragdoll put nothing away: the \
                 graph's holster, hide, stop-use, and heavy-weapon drop tasks are refused for the \
                 player, and a gun in a hand in a seat fires. Off, the game puts the wielded weapon \
                 away and the table follows.",
        );
        ui.checkbox(&mut c.heavy_on_back, "Heavy weapons dock on the back")
            .on_hover_text(
                "A heavy weapon is taken into a hand through the table, armed there, and docked on \
                 the back like a long gun, never wielded from the heavy slot. Off, it is the \
                 game's: wielded from its slot and dropped when put away.",
            );
        ui.checkbox(&mut c.return_lost, "Bring lost weapons back")
            .on_hover_text(
                "A weapon that fell out of the world, or that was let go of at speed and left \
                 behind, comes back to a free socket, else the feet.",
            );
        ui.add_enabled_ui(c.return_lost, |ui| {
            ui.add(
                egui::Slider::new(&mut c.lost_speed_mps, 2.0..=40.0)
                    .text("Travelling fast from (m/s)"),
            );
            ui.add(egui::Slider::new(&mut c.left_behind_m, 5.0..=100.0).text("Left behind past (m)"));
            ui.add(
                egui::Slider::new(&mut c.lost_depth_m, 1.0..=50.0)
                    .text("Out of the world below the sea by (m)"),
            );
        });
        ui.checkbox(
            &mut c.pin_world,
            "Keep dropped weapons nearby from despawning",
        );
        ui.add_enabled_ui(c.pin_world, |ui| {
            ui.add(egui::Slider::new(&mut c.pin_radius_m, 0.0..=30.0).text("Kept within (m)"));
            ui.add(egui::Slider::new(&mut c.unpin_radius_m, 0.0..=40.0).text("Let go of past (m)"));
            ui.add(egui::Slider::new(&mut c.max_pinned, 0..=16).text("Most kept at once"));
        });
        ui.add(egui::Slider::new(&mut c.forget_m, 10.0..=200.0).text("Left to the game past (m)"));
        ui.checkbox(&mut c.carry, "Carry weapons beyond the slots on the body");
        ui.checkbox(
            &mut c.persist,
            "Keep the carried weapons across saves and loads",
        );
        ui.add_enabled_ui(c.carry, |ui| {
            for socket in Socket::ALL {
                let offset = match socket {
                    Socket::Shoulder(Hand::Right) => &mut c.offsets.back_first,
                    Socket::Shoulder(Hand::Left) => &mut c.offsets.back_second,
                    Socket::BackOuter => &mut c.offsets.back_outer,
                    Socket::Hip(Hand::Left) => &mut c.offsets.hip_left,
                    Socket::Hip(Hand::Right) => &mut c.offsets.hip_right,
                    Socket::Chest => &mut c.offsets.chest,
                };
                super::vec3_drag(
                    ui,
                    &format!("Offset on the {} (m)", socket.label()),
                    offset,
                    0.005,
                );
            }
        });
    });
}

fn task_index(task: Task) -> usize {
    Task::ALL.iter().position(|t| *t == task).unwrap_or(0)
}
