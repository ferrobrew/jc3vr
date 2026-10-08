//! The status page's rows for the VR inventory: the weapons it tracks, and the ones lying in the
//! world it keeps from the game's despawn.

use smol_str::format_smolstr;

use crate::controllers::{
    menu::{read_config, status::Line},
    weapons::{holster::slot_label, inventory},
};

pub(super) fn tracked() -> Line {
    if !read_config(|c| c.controllers.weapons.inventory.enabled) {
        return Line::off("");
    }
    let s = inventory::status();
    if !s.active {
        return Line::waiting("NO PLAYER YET");
    }
    let owned = s.entries.iter().filter(|e| e.location.owned()).count();
    let (_, world) = s.world();
    Line::ok(format_smolstr!(
        "{owned} OWNED, {world} IN THE WORLD, {} RESETS",
        s.resets
    ))
}

pub(super) fn pinned() -> Line {
    if !read_config(|c| {
        let inventory = c.controllers.weapons.inventory;
        inventory.enabled && inventory.pin_world
    }) {
        return Line::off("");
    }
    let s = inventory::status();
    let (pinned, world) = s.world();
    Line::ok(format_smolstr!(
        "{pinned} OF {world} KEPT, {} PINNED, {} LEFT TO THE GAME",
        s.pins,
        s.forgotten
    ))
}

pub(super) fn carried() -> Line {
    if !read_config(|c| {
        let inventory = c.controllers.weapons.inventory;
        inventory.enabled && inventory.carry
    }) {
        return Line::off("");
    }
    let s = inventory::status();
    let carried: Vec<String> = s
        .carried()
        .map(|(slot, socket)| {
            format!(
                "{} ON {}",
                slot_label(slot).to_uppercase(),
                socket.label().to_uppercase()
            )
        })
        .collect();
    let list = if carried.is_empty() {
        "NONE".to_owned()
    } else {
        carried.join(", ")
    };
    Line::ok(format_smolstr!(
        "{list}; {} DRAWN, {} PUT AWAY, {} KEPT FROM PICKUPS",
        s.draws,
        s.stows,
        s.carried_replaced
    ))
}
