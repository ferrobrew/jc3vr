//! The status page's rows for what the empty hands do in the world: vehicle doors, the characters
//! they take hold of, the moving bodies they climb, and pushing the body up.

use smol_str::{SmolStr, format_smolstr};

use crate::controllers::{
    menu::{
        read_config,
        status::{Health, Line},
    },
    physical_hands::{
        self, climb, doors, physics,
        pickup::PickupStatus,
        vault::{self, VaultStatus},
    },
};

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

pub(super) fn push_up() -> Line {
    let on = read_config(|c| {
        let p = &c.controllers.physical_hands;
        physics::enabled(p) && p.vault.enabled
    });
    push_up_line(on, vault::status())
}

fn push_up_line(on: bool, status: VaultStatus) -> Line {
    if !on {
        return Line::off("");
    }
    let rise = status
        .last_rise_m
        .map_or_else(SmolStr::default, |r| format_smolstr!(", LAST {:.2} M", r));
    match (status.pushes, status.lifting) {
        (_, true) => Line::ok(format_smolstr!(
            "{} PUSHES, LIFTING {:.1} M/S{rise}",
            status.pushes,
            status.lift_mps
        )),
        (0, false) => Line::new(Health::Waiting, "NO PUSH YET"),
        (pushes, false) => Line::ok(format_smolstr!("{pushes} PUSHES{rise}")),
    }
}

fn moving_climbs_line(on: bool, grips: u64, on_moving: bool) -> Line {
    if !on {
        return Line::off("");
    }
    match (grips, on_moving) {
        (_, true) => Line::ok(format_smolstr!("{grips} GRIPS, HOLDING ON")),
        (0, false) => Line::new(Health::Waiting, "NO MOVING GRIP YET"),
        _ => Line::ok(format_smolstr!("{grips} GRIPS")),
    }
}

fn characters_line(on: bool, pickup: &PickupStatus, knockdowns: u64) -> Line {
    if !on {
        return Line::off("");
    }
    let detail = format_smolstr!(
        "{} GRIPS, {} YANKED, {} DOWN, {} DRAGGED, {} PULLED OUT",
        pickup.character_grips,
        pickup.yanks,
        knockdowns,
        pickup.regrips,
        pickup.hijacks
    );
    if pickup.character_grips == 0 {
        Line::new(Health::Waiting, "NO CHARACTER TAKEN YET")
    } else {
        Line::ok(detail)
    }
}

fn doors_line(on: bool, pickup: &PickupStatus, latched: [bool; 2]) -> Line {
    if !on {
        return Line::off("");
    }
    let mut detail = format_smolstr!("{} GRIPS, {} OPENED", pickup.door_grips, pickup.unlatches);
    if let Some(slot) = pickup.last_door {
        detail = format_smolstr!("{detail}, LAST {}", doors::slot_label(slot).to_uppercase());
    }
    if latched.contains(&true) {
        detail = format_smolstr!("{detail}, ON A LATCH");
    }
    if pickup.door_grips == 0 && pickup.unlatches == 0 {
        Line::new(Health::Waiting, "NO DOOR TAKEN YET")
    } else {
        Line::ok(detail)
    }
}

#[cfg(test)]
mod tests {
    use jc3gi::vehicle::vehicle::DoorSlot;

    use super::*;

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
