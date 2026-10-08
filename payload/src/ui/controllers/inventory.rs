//! The VR inventory: every weapon it tracks and where each is, the pins, and its settings.

use egui::Ui;

use crate::controllers::weapons::{
    holster::slot_label,
    inventory::{
        InventoryStatus, config::InventoryConfig, restore::RestoreStatus, sockets::SocketId,
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
    let (pinned, world) = s.world();
    ui.label(format!(
        "Active: {} · {} weapons · {pinned} of {world} in the world kept · pins {} · left to the \
         game {} · resets {}",
        super::yes_no(s.active),
        s.entries.len(),
        s.pins,
        s.forgotten,
        s.resets
    ));
    ui.label(format!(
        "Carried: {}{} · drawn {} · put away {} · kept from pickups {} · with others {}",
        s.carried().count(),
        if s.stashed {
            " (stashed for a mission)"
        } else {
            ""
        },
        s.draws,
        s.stows,
        s.carried_replaced,
        s.with_others()
    ));
    for entry in &s.entries {
        ui.label(format!(
            "#{} {} ({:#010x}) · {} · magazine {}{}",
            entry.id.0,
            slot_label(entry.kind.slot),
            entry.kind.tag_hash,
            entry.location.label(),
            entry.magazine,
            entry
                .distance_m
                .map_or(String::new(), |d| format!(" · {d:.1} m away"))
        ));
    }
}

pub(super) fn inventory_settings(ui: &mut Ui, c: &mut InventoryConfig) {
    ui.checkbox(&mut c.enabled, "Track the player's weapons");
    ui.add_enabled_ui(c.enabled, |ui| {
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
            ui.add(egui::Slider::new(&mut c.back_sockets, 0..=3).text("Long guns on the back"));
            ui.add(egui::Slider::new(&mut c.hip_sockets, 0..=2).text("Pistols on the hips"));
            ui.checkbox(&mut c.chest_socket, "A submachine gun on the chest");
            ui.add(egui::Slider::new(&mut c.socket_radius_m, 0.03..=0.3).text("Socket radius (m)"));
            for socket in SocketId::ALL {
                let offset = match socket {
                    SocketId::BackFirst => &mut c.offsets.back_first,
                    SocketId::BackSecond => &mut c.offsets.back_second,
                    SocketId::BackOuter => &mut c.offsets.back_outer,
                    SocketId::HipLeft => &mut c.offsets.hip_left,
                    SocketId::HipRight => &mut c.offsets.hip_right,
                    SocketId::Chest => &mut c.offsets.chest,
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
