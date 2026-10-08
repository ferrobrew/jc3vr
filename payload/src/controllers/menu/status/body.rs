//! The status page's rows for the procedural body outside standing play: the torso in a vehicle
//! seat. Each reads the last gate's published status, which the pause stops updating, so a row
//! shows what the body did just before the menu opened.

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
}
