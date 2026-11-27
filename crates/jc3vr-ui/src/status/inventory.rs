//! The status rows for the VR inventory: the weapons it tracks, the ones lying in the world it
//! keeps from the game's despawn, and the live loadout table written beside it.

use jc3vr_weapons::controllers::{
    loadout::{Place, live, slot_label},
    weapons::{
        dual_wield::{single, single::grant::TwinState},
        inventory,
        inventory::{
            restore,
            restore::{LoadOutcome, Origin},
        },
    },
};
use smol_str::{SmolStr, format_smolstr};

use crate::status::{Line, Row};

pub(crate) const TRACKED: Row = Row::new("Tracked weapons", tracked);
pub(crate) const CARRIED: Row = Row::new("Carried", carried);
pub(crate) const SEPARATE_PISTOLS: Row = Row::new("Separate pistols", separate_pistols);
pub(crate) const SIDECAR: Row = Row::new("Save sidecar", sidecar);
pub(crate) const LAST_LOAD: Row = Row::new("Last load", last_load);
pub(crate) const PINNED: Row = Row::new("Kept in the world", pinned);
pub(crate) const LOADOUT: Row = Row::new("Loadout table", loadout);

fn tracked() -> Line {
    let s = inventory::status();
    if !s.active {
        return Line::waiting("No player yet");
    }
    Line::ok(format_smolstr!(
        "{} owned, {} in the world, {} with others, {} resets",
        s.owned,
        s.world.len(),
        s.with_others,
        s.resets
    ))
}

/// The live loadout table: where its items are, and how many weapons it disagrees with the old
/// handling about.
fn loadout() -> Line {
    let s = live::status();
    let Some(snapshot) = &s.snapshot else {
        return Line::waiting("No player yet");
    };
    let (mut hands, mut sockets, mut world) = (0_usize, 0_usize, 0_usize);
    for item in &snapshot.items {
        match item.place {
            Place::Hand(_) => hands += 1,
            Place::Socket(_) => sockets += 1,
            Place::World(_) => world += 1,
        }
    }
    Line::ok(format_smolstr!(
        "{} in hands, {} on sockets, {} in the world, {} refused",
        hands,
        sockets,
        world,
        s.refusals
    ))
}

/// Whether the pistols are two weapons: the slot's pistol, the second pistol of the last pair, and
/// the counts.
fn separate_pistols() -> Line {
    let s = single::status();
    if !s.active {
        return Line::waiting("No player yet");
    }
    let counts = format_smolstr!(
        "{} single, {} pairs split, {} taken as one",
        s.singles,
        s.splits,
        (s.picked + s.ones)
    );
    let line = match (s.slot_single, s.twin) {
        (true, _) => Line::ok(format_smolstr!("The slot's pistol is single, {}", counts)),
        (false, TwinState::Pending { .. }) => {
            Line::waiting(format_smolstr!("Second pistol coming, {}", counts))
        }
        (false, TwinState::Pair { why, .. }) => {
            Line::failed(format_smolstr!("Pair kept: {}, {}", why.label(), counts))
        }
        (false, _) => Line::ok(format_smolstr!("No pistol in the slot, {}", counts)),
    };
    match s.last {
        Some(last) => line.with_note(last),
        None => line,
    }
}

fn pinned() -> Line {
    let s = inventory::status();
    Line::ok(format_smolstr!(
        "{} of {} kept, {} pinned, {} left to the game",
        s.pinned(),
        s.world.len(),
        s.pins,
        s.forgotten
    ))
}

fn sidecar() -> Line {
    let s = restore::status();
    if s.path.is_none() {
        return Line::failed("No configuration directory");
    }
    if let Some(error) = s.last_write_error {
        return Line::failed("Last write failed, see log").with_note(error);
    }
    if s.writes == 0 {
        return Line::waiting("Not written yet, waiting for a save");
    }
    Line::ok(format_smolstr!(
        "{} writes, {} carried in the last",
        s.writes,
        s.last_written
    ))
}

fn last_load() -> Line {
    let s = restore::status();
    let load = match &s.last_load {
        None => SmolStr::new_static("No load yet"),
        Some(LoadOutcome::Accepted { carried, unknown }) => format_smolstr!(
            "Sidecar matched, {} to restore, {} unknown",
            carried,
            unknown
        ),
        Some(LoadOutcome::Missing) => SmolStr::new_static("No sidecar"),
        Some(LoadOutcome::Unreadable(_)) => SmolStr::new_static("Sidecar unreadable"),
        Some(LoadOutcome::Rejected(_)) => SmolStr::new_static("Another save's sidecar"),
        Some(LoadOutcome::NoCharacter) => SmolStr::new_static("No character"),
    };
    let respawn = s.last_respawn.map_or_else(SmolStr::default, |r| {
        format_smolstr!(
            "; respawned {} of {} from the {}{}",
            r.restored,
            r.requested,
            match r.origin {
                Some(Origin::Load) => SmolStr::new_static("load"),
                _ => SmolStr::new_static("reset"),
            },
            if r.failed > 0 {
                format_smolstr!(", {} failed", r.failed)
            } else {
                SmolStr::default()
            }
        )
    });
    let note = match &s.last_load {
        Some(LoadOutcome::Unreadable(why) | LoadOutcome::Rejected(why)) => Some(why.clone()),
        _ => None,
    };
    let detail = format_smolstr!("{}{}", load, respawn);
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

fn carried() -> Line {
    let s = inventory::status();
    let carried: Vec<SmolStr> = s
        .carried
        .iter()
        .map(|(slot, socket)| format_smolstr!("{} on {}", slot_label(*slot), socket.label()))
        .collect();
    let list = if carried.is_empty() {
        SmolStr::new_static("None")
    } else {
        SmolStr::from(carried.join(", "))
    };
    let list = if s.stashed {
        format_smolstr!("Stashed for a mission: {}", list)
    } else {
        list
    };
    Line::ok(format_smolstr!(
        "{}; {} kept from pickups",
        list,
        s.carried_replaced
    ))
}
