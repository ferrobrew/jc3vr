//! The status page's rows for the procedural body outside standing play: the torso in a vehicle
//! seat, the wingsuit's arms, the body under the parachute, the torso in the water, the torso on
//! a vehicle's roof, the body mirror, and the render-rate body. Each reads the last published
//! status, which the pause stops updating, so a row shows what the body did just before the menu
//! opened.

use std::time::{Duration, Instant};

use smol_str::format_smolstr;

use crate::controllers::{
    body::{
        mirror::{self, MirrorStatus},
        procedural::{self, ProceduralStatus, authority::Case, config::ProceduralMode},
    },
    menu::{read_config, status::Line},
    presentation::{self, PresentationStatus},
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

pub(super) fn roof_torso() -> Line {
    let (enabled, mode) = read_config(|c| {
        (
            c.controllers.body.procedural.stunt_torso,
            c.controllers.body.procedural.mode,
        )
    });
    if !enabled || mode != ProceduralMode::Full {
        return Line::off("");
    }
    roof_line(&procedural::published_status())
}

/// The torso standing in a vehicle's stunt area.
fn roof_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::StuntStanding {
        return Line::waiting("NOT ON A ROOF").with_note(status.case.label());
    }
    torso_on_hips_line(status)
}

pub(super) fn body_mirror() -> Line {
    if !read_config(|c| c.controllers.overlay.body_mirror) {
        return Line::off("SWITCH ON IN DEBUG UI")
            .with_note("the Controllers tab's overlay section, Body mirror");
    }
    mirror_line(&mirror::status(), Instant::now())
}

/// The body mirror: the bones it found, and whether it drew recently. The pause stops the pass, so
/// a mirror that drew before the menu opened still reads as drawing.
fn mirror_line(status: &MirrorStatus, now: Instant) -> Line {
    match (status.bones, status.at) {
        (Some(0), _) => Line::failed("NO BONES FOUND"),
        (Some(bones), Some(at)) => {
            let detail = format_smolstr!("{bones} BONES");
            if now.saturating_duration_since(at) < MIRROR_STALE {
                Line::ok(detail)
            } else {
                Line::waiting(detail).with_note("not drawn since the menu opened or the pass ran")
            }
        }
        _ => Line::waiting("NOT DRAWN YET"),
    }
}

/// How long after its last pass the mirror reads as drawing.
const MIRROR_STALE: Duration = Duration::from_secs(30);

pub(super) fn render_rate_body() -> Line {
    if !read_config(|c| c.controllers.hands.presentation.enabled) {
        return Line::off("");
    }
    presentation_line(&presentation::status())
}

/// The render-rate body: how many of the player's body draws it re-posed and its smoothed cost
/// per frame, from the last frame it was active in, which the pause menu's frames leave as they
/// were.
fn presentation_line(status: &PresentationStatus) -> Line {
    if let Some(error) = &status.rig_error {
        return Line::failed("NO RIG").with_note(error.to_string());
    }
    if !status.measured {
        return Line::waiting(status.idle.label().to_uppercase());
    }
    Line::ok(format_smolstr!(
        "{} DRAWS, {:.3} MS",
        status.draws,
        status.mean_cost_us / 1000.0
    ))
    .with_note(format!(
        "now {}, last frame {:.3} ms, peak {:.3} ms, {} ticks",
        status.idle.label(),
        status.frame_cost_us / 1000.0,
        status.peak_cost_us / 1000.0,
        status.ticks
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::presentation::Idle;

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
    fn the_roof_torso_reads_its_weight_on_a_roof_only() {
        assert_eq!(
            roof_line(&status(Case::Vehicle, 0.0)).text(),
            "WAITING, NOT ON A ROOF"
        );
        assert_eq!(
            roof_line(&status(Case::StuntStanding, 1.0)).text(),
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
    fn the_mirror_reads_its_bones() {
        let now = Instant::now();
        let drawn = MirrorStatus {
            at: Some(now),
            bones: Some(21),
            primitives: 30,
        };
        assert_eq!(mirror_line(&drawn, now).text(), "OK, 21 BONES");
        assert_eq!(
            mirror_line(
                &MirrorStatus {
                    bones: Some(0),
                    ..drawn
                },
                now
            )
            .text(),
            "FAILED, NO BONES FOUND"
        );
        assert_eq!(
            mirror_line(&MirrorStatus::default(), now).text(),
            "WAITING, NOT DRAWN YET"
        );
    }

    #[test]
    fn the_render_rate_body_reads_its_draws_and_cost_or_why_it_waits() {
        let waiting = PresentationStatus {
            idle: Idle::NotInHeadset,
            ..PresentationStatus::default()
        };
        assert_eq!(
            presentation_line(&waiting).text(),
            "WAITING, NOT IN THE HEADSET"
        );
        let active = PresentationStatus {
            measured: true,
            draws: 6,
            mean_cost_us: 21.0,
            ..PresentationStatus::default()
        };
        assert_eq!(presentation_line(&active).text(), "OK, 6 DRAWS, 0.021 MS");
        let unrigged = PresentationStatus {
            rig_error: Some("presentation rig: the skeleton has no bone named Neck".into()),
            ..active
        };
        assert_eq!(presentation_line(&unrigged).text(), "FAILED, NO RIG");
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
