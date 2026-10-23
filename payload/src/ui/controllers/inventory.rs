//! The VR inventory: the carried weapons, the weapons lying in the world and their pins, and its
//! settings.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    loadout::{Socket, slot_label},
    weapons::{
        dual_wield::single,
        inventory::{InventoryStatus, config::InventoryConfig, restore::RestoreStatus},
    },
};

pub(super) fn inventory_readout(ui: &mut Ui, s: &InventoryStatus, r: &RestoreStatus) {
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
