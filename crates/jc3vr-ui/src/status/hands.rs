//! The status rows for what the empty hands do in the world: whether the palms act, vehicle
//! doors, the characters they take hold of, the moving bodies they climb, the stunt positions they hold, pushing the body
//! up, and articulated objects.

use jc3vr_body::controllers::grab::world_reach::TargetKind;
use jc3vr_physical::controllers::{
    physical_hands,
    physical_hands::{
        Idle, climb, doors, physics,
        pickup::PickupStatus,
        stunt,
        stunt::{StuntStatus, solve::StuntKind},
        vault,
        vault::VaultStatus,
    },
};
use smol_str::{SmolStr, format_smolstr};

use crate::status::{Health, Line, Row};

pub(crate) const PALMS: Row = Row::new("Palms", palms);
pub(crate) const DOORS: Row = Row::new("Vehicle doors", doors);
pub(crate) const CHARACTERS: Row = Row::new("Characters", characters);
pub(crate) const MOVING_CLIMBS: Row = Row::new("Moving climbs", moving_climbs);
pub(crate) const STUNT_POSITIONS: Row = Row::new("Stunt positions", stunt_positions);
pub(crate) const PUSH_UP: Row = Row::new("Push up", push_up);
pub(crate) const ARTICULATED: Row = Row::new("Hinged things", articulated);

fn palms() -> Line {
    let status = physical_hands::status();
    palms_line(status.idle, status.casts, status.off_thread)
}

fn doors() -> Line {
    let status = physical_hands::status();
    let latched = status.hands.map(|h| h.latched);
    doors_line(&status.pickup, latched)
}

fn characters() -> Line {
    characters_line(
        &physical_hands::status().pickup,
        physics::status().knockdowns,
    )
}

fn moving_climbs() -> Line {
    let status = climb::status();
    let on_moving = status
        .hands
        .iter()
        .any(|h| h.grip.is_some_and(|g| g.moving));
    moving_climbs_line(status.moving_grips, on_moving)
}

fn stunt_positions() -> Line {
    stunt_line(&stunt::status())
}

fn push_up() -> Line {
    push_up_line(vault::status())
}

fn articulated() -> Line {
    let status = physical_hands::status();
    let reaching = status.hands.iter().any(|h| {
        matches!(
            h.reach.map(|r| r.kind),
            Some(TargetKind::Articulated { .. })
        )
    });
    articulated_line(status.pickup.articulated_grips, reaching)
}

fn articulated_line(grips: u64, reaching: bool) -> Line {
    match (grips, reaching) {
        (_, true) => Line::ok(format_smolstr!("{} grips, one in reach", grips)),
        (0, false) => Line::waiting("None taken yet"),
        _ => Line::ok(format_smolstr!("{} grips", grips)),
    }
}

fn push_up_line(status: VaultStatus) -> Line {
    let rise = status
        .last_rise_m
        .map_or_else(SmolStr::default, |r| format_smolstr!(", last {r:.2} m"));
    let line = match (status.pushes, status.lifting) {
        (_, true) => Line::ok(format_smolstr!(
            "{} pushes, lifting {:.1} m/s{}",
            status.pushes,
            status.lift_mps,
            rise
        )),
        (0, false) => Line::waiting("No push yet"),
        (pushes, false) => Line::ok(format_smolstr!("{} pushes{}", pushes, rise)),
    };
    line.with_note(format!(
        "{} presses at the feet ignored, {} refused off the ground or moving down",
        status.at_the_feet, status.refusals
    ))
}

fn moving_climbs_line(grips: u64, on_moving: bool) -> Line {
    match (grips, on_moving) {
        (_, true) => Line::ok(format_smolstr!("{} grips, holding on", grips)),
        (0, false) => Line::waiting("No moving grip yet"),
        _ => Line::ok(format_smolstr!("{} grips", grips)),
    }
}

/// The stunt positions: the one the game holds the player in, whether the grips hold the hang,
/// and the entries and let-gos counted.
fn stunt_line(status: &StuntStatus) -> Line {
    let counts = format_smolstr!(
        "{} hangs, {} roofs, {} let go",
        status.hang_entries,
        status.roof_entries,
        status.releases
    );
    let line = match status.position {
        Some(StuntKind::Hanging) if status.holding.iter().any(|&h| h) => {
            Line::ok(format_smolstr!("Hanging, holding on, {}", counts))
        }
        Some(StuntKind::Hanging) if status.armed => {
            Line::ok(format_smolstr!("Hanging, letting go, {}", counts))
        }
        Some(StuntKind::Hanging) => {
            Line::waiting(format_smolstr!("Hanging, take hold, {}", counts))
        }
        Some(StuntKind::Standing) => Line::ok(format_smolstr!("On a roof, {}", counts)),
        None if status.hang_entries + status.roof_entries + status.releases == 0 => {
            Line::waiting("No stunt position yet")
        }
        None => Line::ok(counts),
    };
    match (&status.last_error, status.refusals) {
        (Some(error), refusals) if refusals > 0 => {
            line.with_note(format!("{refusals} refused, last: {error}"))
        }
        _ => line,
    }
}

fn characters_line(pickup: &PickupStatus, knockdowns: u64) -> Line {
    let detail = format_smolstr!(
        "{} grips, {} thrown, {} down, {} dragged, {} pulled out",
        pickup.character_grips,
        pickup.yanks,
        knockdowns,
        pickup.regrips,
        pickup.hijacks
    );
    if pickup.character_grips == 0 {
        Line::new(
            Health::Waiting,
            SmolStr::new_static("No character taken yet"),
        )
    } else {
        Line::ok(detail)
    }
}

/// The palms' row: everything the empty hands find in the world (doors, pickups, climbs) comes from
/// the palms' tick, so a tick that stands down silently takes all of it away.
fn palms_line(idle: Option<Idle>, casts: u32, off_thread: u64) -> Line {
    let off_thread_note =
        (off_thread > 0).then(|| format_smolstr!("{} ticks ran off the game thread", off_thread));
    match (idle, off_thread_note) {
        (Some(Idle::OffThread), _) => Line::failed(format_smolstr!(
            "Idle, off the game thread ({} ticks)",
            off_thread
        )),
        (_, Some(note)) => Line::failed(note),
        (Some(Idle::Disabled), None) => Line::off("Not under the controller scheme"),
        (Some(Idle::Mode), None) => Line::waiting("Not on foot or falling"),
        (Some(Idle::NoHands), None) => Line::waiting("No tracked hands"),
        (None, None) => Line::ok(format_smolstr!("Acting, {} casts", casts)),
    }
}

fn doors_line(pickup: &PickupStatus, latched: [bool; 2]) -> Line {
    let mut detail = format!("{} grips, {} opened", pickup.door_grips, pickup.unlatches);
    if let Some(slot) = pickup.last_door {
        detail += &format!(", last {}", doors::slot_label(slot));
    }
    if latched.contains(&true) {
        detail += ", on a latch";
    }
    if pickup.door_grips == 0 && pickup.unlatches == 0 {
        Line::waiting("No door taken yet")
    } else {
        Line::ok(detail)
    }
}

#[cfg(test)]
mod tests {
    use jc3gi::vehicle::vehicle::DoorSlot;

    use super::*;

    #[test]
    fn the_palms_row_fails_when_a_tick_ran_off_the_game_thread() {
        let acting = palms_line(None, 4, 0);
        assert_eq!(acting.text(), "Ok, Acting, 4 casts");
        assert_eq!(acting.detail, "Acting, 4 casts");
        assert_eq!(palms_line(Some(Idle::Mode), 0, 0).health, Health::Waiting);
        assert_eq!(
            palms_line(Some(Idle::OffThread), 0, 12).text(),
            "Failed, Idle, off the game thread (12 ticks)"
        );
        // Acting now, but a tick ran off the game thread since the payload loaded.
        assert_eq!(
            palms_line(None, 4, 3).text(),
            "Failed, 3 ticks ran off the game thread"
        );
    }

    #[test]
    fn the_stunt_row_says_what_holds_the_player() {
        let mut status = StuntStatus::default();
        assert_eq!(stunt_line(&status).health, Health::Waiting);
        status.position = Some(StuntKind::Hanging);
        status.hang_entries = 1;
        assert_eq!(
            stunt_line(&status).text(),
            "Waiting, Hanging, take hold, 1 hangs, 0 roofs, 0 let go"
        );
        status.holding = [false, true];
        status.armed = true;
        assert_eq!(
            stunt_line(&status).text(),
            "Ok, Hanging, holding on, 1 hangs, 0 roofs, 0 let go"
        );
        status.position = Some(StuntKind::Standing);
        status.roof_entries = 2;
        assert_eq!(
            stunt_line(&status).text(),
            "Ok, On a roof, 1 hangs, 2 roofs, 0 let go"
        );
        status.position = None;
        status.releases = 1;
        assert_eq!(stunt_line(&status).text(), "Ok, 1 hangs, 2 roofs, 1 let go");
    }

    #[test]
    fn the_articulated_row_counts_grips() {
        assert_eq!(articulated_line(0, false).health, Health::Waiting);
        assert_eq!(
            articulated_line(2, true).text(),
            "Ok, 2 grips, one in reach"
        );
        assert_eq!(articulated_line(2, false).text(), "Ok, 2 grips");
    }

    #[test]
    fn the_push_up_row_says_when_the_hands_lift() {
        let mut status = VaultStatus::default();
        assert_eq!(push_up_line(status).health, Health::Waiting);
        status.pushes = 2;
        status.lifting = true;
        status.lift_mps = 1.25;
        status.last_rise_m = Some(0.4);
        assert_eq!(
            push_up_line(status).text(),
            "Ok, 2 pushes, lifting 1.2 m/s, last 0.40 m"
        );
        status.lifting = false;
        assert_eq!(push_up_line(status).text(), "Ok, 2 pushes, last 0.40 m");
    }

    #[test]
    fn the_moving_climbs_row_says_when_a_hand_holds_on() {
        assert_eq!(moving_climbs_line(0, false).health, Health::Waiting);
        assert_eq!(
            moving_climbs_line(2, true).text(),
            "Ok, 2 grips, holding on"
        );
        assert_eq!(moving_climbs_line(2, false).text(), "Ok, 2 grips");
    }

    #[test]
    fn the_characters_row_counts_grips_and_hijacks() {
        let mut pickup = PickupStatus::default();
        assert_eq!(characters_line(&pickup, 0).health, Health::Waiting);
        pickup.character_grips = 3;
        pickup.yanks = 2;
        pickup.regrips = 1;
        pickup.hijacks = 1;
        assert_eq!(
            characters_line(&pickup, 2).text(),
            "Ok, 3 grips, 2 thrown, 2 down, 1 dragged, 1 pulled out"
        );
    }

    #[test]
    fn the_doors_row_counts_grips_and_openings() {
        let mut pickup = PickupStatus::default();
        assert_eq!(
            doors_line(&pickup, [false; 2]).text(),
            "Waiting, No door taken yet"
        );
        pickup.door_grips = 2;
        pickup.unlatches = 1;
        pickup.last_door = Some(DoorSlot::FRONT_LEFT);
        assert_eq!(
            doors_line(&pickup, [false, true]).text(),
            "Ok, 2 grips, 1 opened, last front left, on a latch"
        );
    }
}
