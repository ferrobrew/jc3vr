//! The status page's rows for the procedural body outside standing play: the torso in a vehicle
//! seat, the wingsuit's arms, the body under the parachute, and the torso in the water. Each reads the last gate's published status, which the pause stops
//! updating, so a row shows what the body did just before the menu opened.

use smol_str::format_smolstr;

use crate::controllers::{
    body::procedural::{self, ProceduralStatus, authority::Case, config::ProceduralMode},
    menu::{read_config, status::Line},
};

pub(super) fn seated_torso() -> Line {
    let (enabled, mode) = read_config(|c| {
        (
            c.controllers.body.procedural.seated_torso,
            c.controllers.body.procedural.mode,
        )
    });
    if !enabled || mode != ProceduralMode::Full {
        return Line::off("");
    }
    seated_line(&procedural::published_status())
}

/// The torso in a seat: how much of it is procedural, and how far the neck is from the point under
/// the player's head.
fn seated_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::Seated {
        return Line::waiting("NOT SEATED").with_note(status.case.label());
    }
    torso_on_hips_line(status)
}

/// The torso on the animation's hips: its weight and the neck's error, at full weight and posed on
/// the hips once the hand-over has finished.
fn torso_on_hips_line(status: &ProceduralStatus) -> Line {
    let detail = format_smolstr!(
        "{:.0}%, NECK {:.0} CM",
        status.weight * 100.0,
        status.neck_error_m * 100.0
    );
    if status.on_animated_hips && status.weight >= 1.0 && !status.switch_fading {
        Line::ok(detail)
    } else {
        Line::waiting(detail)
    }
}

pub(super) fn wingsuit_arms() -> Line {
    let (enabled, mode) = read_config(|c| {
        (
            c.controllers.body.procedural.wingsuit_arms,
            c.controllers.body.procedural.mode,
        )
    });
    if !enabled || mode != ProceduralMode::Full {
        return Line::off("");
    }
    wings_line(&procedural::published_status())
}

/// The wingsuit's arms: their weight and the hands they queued targets for.
fn wings_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::Wingsuit {
        return Line::waiting("NOT IN THE WINGSUIT").with_note(status.case.label());
    }
    let hands = match status.wing_queued {
        [true, true] => "BOTH HANDS",
        [true, false] => "LEFT HAND",
        [false, true] => "RIGHT HAND",
        [false, false] => "NO HANDS",
    };
    let detail = format_smolstr!("{:.0}%, {hands}", status.wing_weight * 100.0);
    if status.wing_queued == [true, true] && status.wing_weight >= 1.0 {
        Line::ok(detail)
    } else {
        Line::waiting(detail)
    }
}

pub(super) fn parachute_body() -> Line {
    let (enabled, mode) = read_config(|c| {
        (
            c.controllers.body.procedural.parachute_body,
            c.controllers.body.procedural.mode,
        )
    });
    if !enabled || mode != ProceduralMode::Full {
        return Line::off("");
    }
    parachute_line(&procedural::published_status())
}

/// The body under the parachute: the torso's weight, and how far the legs swing forward for the
/// brake.
fn parachute_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::Parachute {
        return Line::waiting("NOT UNDER THE CANOPY").with_note(status.case.label());
    }
    let detail = format_smolstr!(
        "{:.0}%, FLARE {:.0}%",
        status.weight * 100.0,
        status.flare * 100.0
    );
    if status.on_animated_hips
        && status.weight >= 1.0
        && status.legs_weight >= 1.0
        && !status.switch_fading
    {
        Line::ok(detail)
    } else {
        Line::waiting(detail)
    }
}

pub(super) fn swim_torso() -> Line {
    let (enabled, mode) = read_config(|c| {
        (
            c.controllers.body.procedural.swim_torso,
            c.controllers.body.procedural.mode,
        )
    });
    if !enabled || mode != ProceduralMode::Full {
        return Line::off("");
    }
    let status = procedural::published_status();
    if status.case != Case::Swimming {
        return Line::waiting("NOT SWIMMING").with_note(status.case.label());
    }
    torso_on_hips_line(&status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(case: Case, weight: f32) -> ProceduralStatus {
        ProceduralStatus {
            case,
            weight,
            on_animated_hips: case.anchored() && weight > 0.0,
            neck_error_m: 0.031,
            ..procedural::published_status()
        }
    }

    #[test]
    fn the_seated_torso_reads_its_weight_and_the_necks_error() {
        assert_eq!(
            seated_line(&status(Case::OnFoot, 1.0)).text(),
            "WAITING, NOT SEATED"
        );
        assert_eq!(
            seated_line(&status(Case::Seated, 0.5)).text(),
            "WAITING, 50%, NECK 3 CM"
        );
        assert_eq!(
            seated_line(&status(Case::Seated, 1.0)).text(),
            "OK, 100%, NECK 3 CM"
        );
    }

    #[test]
    fn the_parachute_reads_the_weights_and_the_flare() {
        let hanging = |weight: f32, flare: f32| ProceduralStatus {
            legs_weight: weight,
            flare,
            ..status(Case::Parachute, weight)
        };
        assert_eq!(
            parachute_line(&hanging(1.0, 0.25)).text(),
            "OK, 100%, FLARE 25%"
        );
        assert_eq!(
            parachute_line(&hanging(0.4, 0.0)).text(),
            "WAITING, 40%, FLARE 0%"
        );
        assert_eq!(
            parachute_line(&status(Case::Flight, 0.0)).text(),
            "WAITING, NOT UNDER THE CANOPY"
        );
    }

    #[test]
    fn the_wingsuit_arms_name_the_hands_they_queued() {
        let flying = |weight: f32, queued: [bool; 2]| ProceduralStatus {
            case: Case::Wingsuit,
            wing_weight: weight,
            wing_queued: queued,
            ..procedural::published_status()
        };
        assert_eq!(
            wings_line(&flying(1.0, [true, true])).text(),
            "OK, 100%, BOTH HANDS"
        );
        assert_eq!(
            wings_line(&flying(1.0, [false, true])).text(),
            "WAITING, 100%, RIGHT HAND"
        );
        assert_eq!(
            wings_line(&status(Case::Parachute, 0.0)).text(),
            "WAITING, NOT IN THE WINGSUIT"
        );
    }
}
