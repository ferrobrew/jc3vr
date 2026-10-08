//! The status page's rows for the VR inventory: the weapons it tracks, and the ones lying in the
//! world it keeps from the game's despawn.

use smol_str::format_smolstr;

use crate::controllers::{
    menu::{read_config, status::Line},
    weapons::{
        holster::slot_label,
        inventory::{
            self,
            restore::{self, LoadOutcome, Origin},
        },
    },
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

pub(super) fn sidecar() -> Line {
    if !read_config(|c| {
        let inventory = c.controllers.weapons.inventory;
        inventory.enabled && inventory.carry && inventory.persist
    }) {
        return Line::off("");
    }
    let s = restore::status();
    if s.path.is_none() {
        return Line::failed("NO CONFIGURATION DIRECTORY");
    }
    if let Some(error) = s.last_write_error {
        return Line::failed("LAST WRITE FAILED, SEE LOG").with_note(error);
    }
    if s.writes == 0 {
        return Line::waiting("NOT WRITTEN YET, WAITING FOR A SAVE");
    }
    Line::ok(format_smolstr!(
        "{} WRITES, {} CARRIED IN THE LAST",
        s.writes,
        s.last_written
    ))
}

pub(super) fn last_load() -> Line {
    if !read_config(|c| {
        let inventory = c.controllers.weapons.inventory;
        inventory.enabled && inventory.carry && inventory.persist
    }) {
        return Line::off("");
    }
    let s = restore::status();
    let load = match &s.last_load {
        None => "NO LOAD YET".to_owned(),
        Some(LoadOutcome::Accepted { carried, unknown }) => {
            format!("SIDECAR MATCHED, {carried} TO RESTORE, {unknown} UNKNOWN")
        }
        Some(LoadOutcome::Missing) => "NO SIDECAR".to_owned(),
        Some(LoadOutcome::Unreadable(_)) => "SIDECAR UNREADABLE".to_owned(),
        Some(LoadOutcome::Rejected(_)) => "ANOTHER SAVE'S SIDECAR".to_owned(),
        Some(LoadOutcome::NoCharacter) => "NO CHARACTER".to_owned(),
    };
    let respawn = s.last_respawn.map_or(String::new(), |r| {
        format!(
            "; RESPAWNED {} OF {} FROM THE {}{}",
            r.restored,
            r.requested,
            match r.origin {
                Some(Origin::Load) => "LOAD",
                _ => "RESET",
            },
            if r.failed > 0 {
                format!(", {} FAILED", r.failed)
            } else {
                String::new()
            }
        )
    });
    let note = match &s.last_load {
        Some(LoadOutcome::Unreadable(why) | LoadOutcome::Rejected(why)) => Some(why.clone()),
        _ => None,
    };
    let line = if matches!(s.last_load, Some(LoadOutcome::Unreadable(_))) {
        Line::failed(format_smolstr!("{load}{respawn}"))
    } else {
        Line::ok(format_smolstr!("{load}{respawn}"))
    };
    match note {
        Some(note) => line.with_note(note),
        None => line,
    }
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
