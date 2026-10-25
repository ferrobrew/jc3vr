//! The status rows for the VR inventory: the weapons it tracks, the ones lying in the world it
//! keeps from the game's despawn, and the live loadout table written beside it.

use crate::{
    controllers::{
        loadout::{Place, live, slot_label},
        menu::read_config,
        weapons::{
            dual_wield::single::{self, grant::TwinState},
            inventory::{
                self,
                restore::{self, LoadOutcome, Origin},
            },
        },
    },
    status::{Line, Text, prose, text},
};

pub(super) fn tracked() -> Line {
    if !read_config(|c| c.controllers.weapons.inventory.enabled) {
        return Line::off("");
    }
    let s = inventory::status();
    if !s.active {
        return Line::waiting(prose("No player yet"));
    }
    Line::ok(text!(
        s.owned.to_string(),
        " owned, ",
        s.world.len().to_string(),
        " in the world, ",
        s.with_others.to_string(),
        " with others, ",
        s.resets.to_string(),
        " resets"
    ))
}

/// The live loadout table: where its items are, and how many weapons it disagrees with the old
/// handling about.
pub(super) fn loadout() -> Line {
    if !read_config(|c| c.controllers.weapons.inventory.enabled) {
        return Line::off("");
    }
    let s = live::status();
    let Some(snapshot) = &s.snapshot else {
        return Line::waiting(prose("No player yet"));
    };
    let (mut hands, mut sockets, mut world) = (0_usize, 0_usize, 0_usize);
    for item in &snapshot.items {
        match item.place {
            Place::Hand(_) => hands += 1,
            Place::Socket(_) => sockets += 1,
            Place::World(_) => world += 1,
        }
    }
    Line::ok(text!(
        hands.to_string(),
        " in hands, ",
        sockets.to_string(),
        " on sockets, ",
        world.to_string(),
        " in the world, ",
        s.refusals.to_string(),
        " refused"
    ))
}

/// Whether the pistols are two weapons: the slot's pistol, the second pistol of the last pair, and
/// the counts.
pub(super) fn separate_pistols() -> Line {
    if !read_config(|c| c.controllers.weapons.separate_pistols) {
        return Line::off("");
    }
    let s = single::status();
    if !s.active {
        return Line::waiting(prose("Needs track weapons and carry extra weapons"));
    }
    let counts = text!(
        s.singles.to_string(),
        " single, ",
        s.splits.to_string(),
        " pairs split, ",
        (s.picked + s.ones).to_string(),
        " taken as one"
    );
    let line = match (s.slot_single, s.twin) {
        (true, _) => Line::ok(text!("The slot's pistol is single, ", counts)),
        (false, TwinState::Pending { .. }) => {
            Line::waiting(text!("Second pistol coming, ", counts))
        }
        (false, TwinState::Pair { why, .. }) => {
            Line::failed(text!("Pair kept: ", prose(why.label()), ", ", counts))
        }
        (false, _) => Line::ok(text!("No pistol in the slot, ", counts)),
    };
    match s.last {
        Some(last) => line.with_note(last),
        None => line,
    }
}

pub(super) fn pinned() -> Line {
    if !read_config(|c| {
        let inventory = c.controllers.weapons.inventory;
        inventory.enabled && inventory.pin_world
    }) {
        return Line::off("");
    }
    let s = inventory::status();
    Line::ok(text!(
        s.pinned().to_string(),
        " of ",
        s.world.len().to_string(),
        " kept, ",
        s.pins.to_string(),
        " pinned, ",
        s.forgotten.to_string(),
        " left to the game"
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
        return Line::failed(prose("No configuration directory"));
    }
    if let Some(error) = s.last_write_error {
        return Line::failed(prose("Last write failed, see log")).with_note(error);
    }
    if s.writes == 0 {
        return Line::waiting(prose("Not written yet, waiting for a save"));
    }
    Line::ok(text!(
        s.writes.to_string(),
        " writes, ",
        s.last_written.to_string(),
        " carried in the last"
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
        None => prose("No load yet"),
        Some(LoadOutcome::Accepted { carried, unknown }) => text!(
            "Sidecar matched, ",
            carried.to_string(),
            " to restore, ",
            unknown.to_string(),
            " unknown"
        ),
        Some(LoadOutcome::Missing) => prose("No sidecar"),
        Some(LoadOutcome::Unreadable(_)) => prose("Sidecar unreadable"),
        Some(LoadOutcome::Rejected(_)) => prose("Another save's sidecar"),
        Some(LoadOutcome::NoCharacter) => prose("No character"),
    };
    let respawn = s.last_respawn.map_or_else(Text::default, |r| {
        text!(
            "; respawned ",
            r.restored.to_string(),
            " of ",
            r.requested.to_string(),
            " from the ",
            match r.origin {
                Some(Origin::Load) => prose("load"),
                _ => prose("reset"),
            },
            if r.failed > 0 {
                text!(", ", r.failed.to_string(), " failed")
            } else {
                Text::default()
            }
        )
    });
    let note = match &s.last_load {
        Some(LoadOutcome::Unreadable(why) | LoadOutcome::Rejected(why)) => Some(why.clone()),
        _ => None,
    };
    let detail = text!(load, respawn);
    let line = if matches!(s.last_load, Some(LoadOutcome::Unreadable(_))) {
        Line::failed(detail)
    } else {
        Line::ok(detail)
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
    let carried: Vec<Text> = s
        .carried
        .iter()
        .map(|(slot, socket)| text!(prose(slot_label(*slot)), " on ", prose(socket.label())))
        .collect();
    let list = if carried.is_empty() {
        prose("None")
    } else {
        Text::join(carried, ", ")
    };
    let list = if s.stashed {
        text!("Stashed for a mission: ", list)
    } else {
        list
    };
    Line::ok(text!(
        list,
        "; ",
        s.carried_replaced.to_string(),
        " kept from pickups"
    ))
}
