//! The status rows for the VR inventory: the weapons it tracks, the ones lying in the world it
//! keeps from the game's despawn, and the live loadout table written beside it.

use smol_str::format_smolstr;

use crate::{
    controllers::{
        loadout::{Place, live},
        menu::read_config,
        weapons::{
            dual_wield::single::{self, grant::TwinState},
            holster::slot_label,
            inventory::{
                self,
                restore::{self, LoadOutcome, Origin},
            },
        },
    },
    status::Line,
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
        "{owned} OWNED, {world} IN THE WORLD, {} WITH OTHERS, {} RESETS",
        s.with_others(),
        s.resets
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
        return Line::waiting("NO PLAYER YET");
    };
    let (mut hands, mut sockets, mut world) = (0, 0, 0);
    for item in &snapshot.items {
        match item.place {
            Place::Hand(_) => hands += 1,
            Place::Socket(_) => sockets += 1,
            Place::World(_) => world += 1,
        }
    }
    let detail = format_smolstr!(
        "{hands} IN HANDS, {sockets} ON SOCKETS, {world} IN THE WORLD, {} DIVERGENT, {} REFUSED",
        s.divergence.len(),
        s.refusals
    );
    if s.divergence.is_empty() {
        return Line::ok(detail);
    }
    let note = s
        .divergence
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    Line::failed(detail).with_note(note)
}

/// Whether the pistols are two weapons: the slot's pistol, the second pistol of the last pair, and
/// the counts.
pub(super) fn separate_pistols() -> Line {
    if !read_config(|c| c.controllers.weapons.separate_pistols) {
        return Line::off("");
    }
    let s = single::status();
    if !s.active {
        return Line::waiting("NEEDS TRACK WEAPONS AND CARRY EXTRA WEAPONS");
    }
    let counts = format_smolstr!(
        "{} SINGLE, {} PAIRS SPLIT, {} TAKEN AS ONE",
        s.singles,
        s.splits,
        s.picked + s.ones
    );
    let line = match (s.slot_single, s.twin) {
        (true, _) => Line::ok(format_smolstr!(
            "SLOT'S ON THE {} HIP, {counts}",
            s.slot_home.map_or("right", |h| h.label()).to_uppercase()
        )),
        (false, TwinState::Pending { .. }) => {
            Line::waiting(format_smolstr!("SECOND PISTOL COMING, {counts}"))
        }
        (false, TwinState::Pair { why, .. }) => Line::failed(format_smolstr!(
            "PAIR KEPT: {}, {counts}",
            why.label().to_uppercase()
        )),
        (false, _) => Line::ok(format_smolstr!("NO PISTOL IN THE SLOT, {counts}")),
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
    let list = if s.stashed {
        format!("STASHED FOR A MISSION: {list}")
    } else {
        list
    };
    Line::ok(format_smolstr!(
        "{list}; {} DRAWN, {} PUT AWAY, {} KEPT FROM PICKUPS",
        s.draws,
        s.stows,
        s.carried_replaced
    ))
}
