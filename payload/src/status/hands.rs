//! The status rows for what the empty hands do in the world: whether the palms act at all, vehicle
//! doors, the characters they take hold of, the moving bodies they climb, the stunt positions they hold, pushing the body
//! up, and articulated objects.

use crate::{
    controllers::{
        menu::read_config,
        physical_hands::{
            self, Idle, climb, doors,
            grasp::TargetKind,
            physics,
            pickup::PickupStatus,
            stunt::{self, StuntStatus, solve::StuntKind},
            vault::{self, VaultStatus},
        },
    },
    status::{Health, Line, Text, prose, text},
};

pub(super) fn palms() -> Line {
    let on = read_config(|c| c.controllers.physical_hands.enabled);
    let status = physical_hands::status();
    palms_line(on, status.idle, status.casts, status.off_thread)
}

pub(super) fn doors() -> Line {
    let on = read_config(|c| {
        let p = &c.controllers.physical_hands;
        p.enabled && p.pickup.enabled && p.doors.enabled
    });
    let status = physical_hands::status();
    let latched = status.hands.map(|h| h.latched);
    doors_line(on, &status.pickup, latched)
}

pub(super) fn characters() -> Line {
    let on = read_config(|c| {
        let p = &c.controllers.physical_hands;
        p.enabled && p.pickup.enabled && (p.characters.grab || p.characters.hijack)
    });
    characters_line(
        on,
        &physical_hands::status().pickup,
        physics::status().knockdowns,
    )
}

pub(super) fn moving_climbs() -> Line {
    let on = read_config(|c| {
        let p = &c.controllers.physical_hands;
        climb::enabled(p) && p.climb.moving
    });
    let status = climb::status();
    let on_moving = status
        .hands
        .iter()
        .any(|h| h.grip.is_some_and(|g| g.moving));
    moving_climbs_line(on, status.moving_grips, on_moving)
}

pub(super) fn stunt_positions() -> Line {
    let on = read_config(|c| stunt::enabled(&c.controllers.physical_hands));
    stunt_line(on, &stunt::status())
}

pub(super) fn push_up() -> Line {
    let on = read_config(|c| {
        let p = &c.controllers.physical_hands;
        physics::enabled(p) && p.vault.enabled
    });
    push_up_line(on, vault::status())
}

pub(super) fn articulated() -> Line {
    let on = read_config(|c| {
        let p = &c.controllers.physical_hands;
        p.enabled && p.pickup.enabled && p.pickup.articulated
    });
    let status = physical_hands::status();
    let reaching = status.hands.iter().any(|h| {
        matches!(
            h.reach.map(|r| r.kind),
            Some(TargetKind::Articulated { .. })
        )
    });
    articulated_line(on, status.pickup.articulated_grips, reaching)
}

fn articulated_line(on: bool, grips: u64, reaching: bool) -> Line {
    if !on {
        return Line::off("");
    }
    match (grips, reaching) {
        (_, true) => Line::ok(text!(grips.to_string(), " grips, one in reach")),
        (0, false) => Line::new(Health::Waiting, prose("None taken yet")),
        _ => Line::ok(text!(grips.to_string(), " grips")),
    }
}

fn push_up_line(on: bool, status: VaultStatus) -> Line {
    if !on {
        return Line::off("");
    }
    let rise = status
        .last_rise_m
        .map_or_else(Text::default, |r| text!(", last ", format!("{r:.2}"), " m"));
    let line = match (status.pushes, status.lifting) {
        (_, true) => Line::ok(text!(
            status.pushes.to_string(),
            " pushes, lifting ",
            format!("{:.1}", status.lift_mps),
            " m/s",
            rise
        )),
        (0, false) => Line::new(Health::Waiting, prose("No push yet")),
        (pushes, false) => Line::ok(text!(pushes.to_string(), " pushes", rise)),
    };
    line.with_note(format!(
        "{} presses at the feet ignored, {} refused off the ground or moving down",
        status.at_the_feet, status.refusals
    ))
}

fn moving_climbs_line(on: bool, grips: u64, on_moving: bool) -> Line {
    if !on {
        return Line::off("");
    }
    match (grips, on_moving) {
        (_, true) => Line::ok(text!(grips.to_string(), " grips, holding on")),
        (0, false) => Line::new(Health::Waiting, prose("No moving grip yet")),
        _ => Line::ok(text!(grips.to_string(), " grips")),
    }
}

/// The stunt positions: the one the game holds the player in, whether the grips hold the hang,
/// and the entries and let-gos counted.
fn stunt_line(on: bool, status: &StuntStatus) -> Line {
    if !on {
        return Line::off("");
    }
    let counts = text!(
        status.hang_entries.to_string(),
        " hangs, ",
        status.roof_entries.to_string(),
        " roofs, ",
        status.releases.to_string(),
        " let go"
    );
    let line = match status.position {
        Some(StuntKind::Hanging) if status.holding.iter().any(|&h| h) => {
            Line::ok(text!("Hanging, holding on, ", counts))
        }
        Some(StuntKind::Hanging) if status.armed => {
            Line::ok(text!("Hanging, letting go, ", counts))
        }
        Some(StuntKind::Hanging) => Line::waiting(text!("Hanging, take hold, ", counts)),
        Some(StuntKind::Standing) => Line::ok(text!("On a roof, ", counts)),
        None if status.hang_entries + status.roof_entries + status.releases == 0 => {
            Line::waiting(prose("No stunt position yet"))
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

fn characters_line(on: bool, pickup: &PickupStatus, knockdowns: u64) -> Line {
    if !on {
        return Line::off("");
    }
    let detail = text!(
        pickup.character_grips.to_string(),
        " grips, ",
        pickup.yanks.to_string(),
        " yanked, ",
        knockdowns.to_string(),
        " down, ",
        pickup.regrips.to_string(),
        " dragged, ",
        pickup.hijacks.to_string(),
        " pulled out"
    );
    if pickup.character_grips == 0 {
        Line::new(Health::Waiting, prose("No character taken yet"))
    } else {
        Line::ok(detail)
    }
}

/// The palms' row: everything the empty hands find in the world (doors, pickups, climbs) comes from
/// the palms' tick, so a tick that stands down silently takes all of it away.
fn palms_line(on: bool, idle: Option<Idle>, casts: u32, off_thread: u64) -> Line {
    if !on {
        return Line::off("");
    }
    let off_thread_note =
        (off_thread > 0).then(|| text!(off_thread.to_string(), " ticks ran off the main thread"));
    match (idle, off_thread_note) {
        (Some(Idle::OffThread), _) => Line::failed(text!(
            "Idle, off the main thread (",
            off_thread.to_string(),
            " ticks)"
        )),
        (_, Some(note)) => Line::failed(note),
        (Some(Idle::Disabled), None) => Line::off(prose("Not under the controller scheme")),
        (Some(Idle::Mode), None) => Line::waiting(prose("Not on foot or falling")),
        (Some(Idle::NoHands), None) => Line::waiting(prose("No tracked hands")),
        (None, None) => Line::ok(text!("Acting, ", casts.to_string(), " casts")),
    }
}

fn doors_line(on: bool, pickup: &PickupStatus, latched: [bool; 2]) -> Line {
    if !on {
        return Line::off("");
    }
    let mut detail = text!(
        pickup.door_grips.to_string(),
        " grips, ",
        pickup.unlatches.to_string(),
        " opened"
    );
    if let Some(slot) = pickup.last_door {
        detail.push(text!(", last ", prose(doors::slot_label(slot))));
    }
    if latched.contains(&true) {
        detail.push(prose(", on a latch"));
    }
    if pickup.door_grips == 0 && pickup.unlatches == 0 {
        Line::new(Health::Waiting, prose("No door taken yet"))
    } else {
        Line::ok(detail)
    }
}

#[cfg(test)]
mod tests {
    use jc3gi::vehicle::vehicle::DoorSlot;

    use super::*;

    #[test]
    fn the_palms_row_fails_when_a_tick_ran_off_the_main_thread() {
        assert_eq!(palms_line(false, None, 4, 0).health, Health::Off);
        let acting = palms_line(true, None, 4, 0);
        assert_eq!(acting.text(), "OK, ACTING, 4 CASTS");
        assert_eq!(acting.display_detail, "Acting, 4 casts");
        assert_eq!(
            palms_line(true, Some(Idle::Mode), 0, 0).health,
            Health::Waiting
        );
        assert_eq!(
            palms_line(true, Some(Idle::OffThread), 0, 12).text(),
            "FAILED, IDLE, OFF THE MAIN THREAD (12 TICKS)"
        );
        // Acting now, but a tick ran off the main thread since the payload loaded.
        assert_eq!(
            palms_line(true, None, 4, 3).text(),
            "FAILED, 3 TICKS RAN OFF THE MAIN THREAD"
        );
    }

    #[test]
    fn the_stunt_row_says_what_holds_the_player() {
        let mut status = StuntStatus::default();
        assert_eq!(stunt_line(false, &status).health, Health::Off);
        assert_eq!(stunt_line(true, &status).health, Health::Waiting);
        status.position = Some(StuntKind::Hanging);
        status.hang_entries = 1;
        assert_eq!(
            stunt_line(true, &status).text(),
            "WAITING, HANGING, TAKE HOLD, 1 HANGS, 0 ROOFS, 0 LET GO"
        );
        status.holding = [false, true];
        status.armed = true;
        assert_eq!(
            stunt_line(true, &status).text(),
            "OK, HANGING, HOLDING ON, 1 HANGS, 0 ROOFS, 0 LET GO"
        );
        status.position = Some(StuntKind::Standing);
        status.roof_entries = 2;
        assert_eq!(
            stunt_line(true, &status).text(),
            "OK, ON A ROOF, 1 HANGS, 2 ROOFS, 0 LET GO"
        );
        status.position = None;
        status.releases = 1;
        assert_eq!(
            stunt_line(true, &status).text(),
            "OK, 1 HANGS, 2 ROOFS, 1 LET GO"
        );
    }

    #[test]
    fn the_articulated_row_counts_grips() {
        assert_eq!(articulated_line(false, 3, true).health, Health::Off);
        assert_eq!(articulated_line(true, 0, false).health, Health::Waiting);
        assert_eq!(
            articulated_line(true, 2, true).text(),
            "OK, 2 GRIPS, ONE IN REACH"
        );
        assert_eq!(articulated_line(true, 2, false).text(), "OK, 2 GRIPS");
    }

    #[test]
    fn the_push_up_row_says_when_the_hands_lift() {
        let mut status = VaultStatus::default();
        assert_eq!(push_up_line(false, status).health, Health::Off);
        assert_eq!(push_up_line(true, status).health, Health::Waiting);
        status.pushes = 2;
        status.lifting = true;
        status.lift_mps = 1.25;
        status.last_rise_m = Some(0.4);
        assert_eq!(
            push_up_line(true, status).text(),
            "OK, 2 PUSHES, LIFTING 1.2 M/S, LAST 0.40 M"
        );
        status.lifting = false;
        assert_eq!(
            push_up_line(true, status).text(),
            "OK, 2 PUSHES, LAST 0.40 M"
        );
    }

    #[test]
    fn the_moving_climbs_row_says_when_a_hand_holds_on() {
        assert_eq!(moving_climbs_line(false, 4, true).health, Health::Off);
        assert_eq!(moving_climbs_line(true, 0, false).health, Health::Waiting);
        assert_eq!(
            moving_climbs_line(true, 2, true).text(),
            "OK, 2 GRIPS, HOLDING ON"
        );
        assert_eq!(moving_climbs_line(true, 2, false).text(), "OK, 2 GRIPS");
    }

    #[test]
    fn the_characters_row_counts_grips_and_hijacks() {
        let mut pickup = PickupStatus::default();
        assert_eq!(characters_line(false, &pickup, 0).health, Health::Off);
        assert_eq!(characters_line(true, &pickup, 0).health, Health::Waiting);
        pickup.character_grips = 3;
        pickup.yanks = 2;
        pickup.regrips = 1;
        pickup.hijacks = 1;
        assert_eq!(
            characters_line(true, &pickup, 2).text(),
            "OK, 3 GRIPS, 2 YANKED, 2 DOWN, 1 DRAGGED, 1 PULLED OUT"
        );
    }

    #[test]
    fn the_doors_row_counts_grips_and_openings() {
        let mut pickup = PickupStatus::default();
        assert_eq!(doors_line(false, &pickup, [false; 2]).health, Health::Off);
        assert_eq!(
            doors_line(true, &pickup, [false; 2]).text(),
            "WAITING, NO DOOR TAKEN YET"
        );
        pickup.door_grips = 2;
        pickup.unlatches = 1;
        pickup.last_door = Some(DoorSlot::FRONT_LEFT);
        assert_eq!(
            doors_line(true, &pickup, [false, true]).text(),
            "OK, 2 GRIPS, 1 OPENED, LAST FRONT LEFT, ON A LATCH"
        );
    }
}
