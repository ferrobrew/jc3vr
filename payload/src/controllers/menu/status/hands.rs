//! The status page's rows for what the empty hands do in the world: vehicle doors and the
//! characters they take hold of.

use smol_str::format_smolstr;

use crate::controllers::{
    menu::{
        read_config,
        status::{Health, Line},
    },
    physical_hands::{self, doors, pickup::PickupStatus},
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
    let (on, hijack) = read_config(|c| {
        let p = &c.controllers.physical_hands;
        (p.enabled && p.pickup.enabled, p.characters.hijack)
    });
    characters_line(on && hijack, &physical_hands::status().pickup)
}

fn characters_line(on: bool, pickup: &PickupStatus) -> Line {
    if !on {
        return Line::off("");
    }
    let detail = format_smolstr!(
        "{} GRIPS, {} PULLED OUT",
        pickup.character_grips,
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
    fn the_characters_row_counts_grips_and_hijacks() {
        let mut pickup = PickupStatus::default();
        assert_eq!(characters_line(false, &pickup).health, Health::Off);
        assert_eq!(characters_line(true, &pickup).health, Health::Waiting);
        pickup.character_grips = 3;
        pickup.hijacks = 1;
        assert_eq!(
            characters_line(true, &pickup).text(),
            "OK, 3 GRIPS, 1 PULLED OUT"
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
