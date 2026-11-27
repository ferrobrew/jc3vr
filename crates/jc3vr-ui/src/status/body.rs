//! The status rows for the procedural torso outside standing play: the torso in a vehicle
//! seat, the wingsuit's arms, the torso under the parachute, the torso in the water, the torso on
//! a vehicle's roof, the body mirror, and the render-rate body. Each reads the last published
//! status, which the pause stops updating, so a row shows what the body did just before the menu
//! opened.

use std::time::Duration;

use jc3vr_body::controllers::{
    body::{
        mirror,
        mirror::MirrorStatus,
        procedural,
        procedural::{ProceduralStatus, authority::Case},
    },
    presentation,
    presentation::{ElbowReadout, PresentationStatus},
};
use jc3vr_core::published::Stamp;
use jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG;
use smol_str::format_smolstr;

use crate::{
    status::{Line, Row},
    tuning::UI,
};

pub(crate) const RENDER_RATE_BODY: Row = Row::new("Render-rate body", render_rate_body);
pub(crate) const SEATED_TORSO: Row = Row::new("Seated torso", seated_torso);
pub(crate) const WINGSUIT_ARMS: Row = Row::new("Wingsuit arms", wingsuit_arms);
pub(crate) const PARACHUTE_BODY: Row = Row::new("Parachute body", parachute_body);
pub(crate) const SWIM_TORSO: Row = Row::new("Swim torso", swim_torso);
pub(crate) const ROOF_TORSO: Row = Row::new("Roof torso", roof_torso);
pub(crate) const BODY_MIRROR: Row = Row::new("Body mirror", body_mirror);

fn seated_torso() -> Line {
    seated_line(&procedural::published_status())
}

/// The torso in a seat: how much of it is procedural, and how far the neck is from the point under
/// the player's head.
fn seated_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::Seated {
        return Line::waiting("Not seated").with_note(status.case.label());
    }
    torso_on_hips_line(status)
}

/// The torso on the animation's hips: its weight and the neck's error, at full weight and posed on
/// the hips once the hand-over has finished.
fn torso_on_hips_line(status: &ProceduralStatus) -> Line {
    let detail = format_smolstr!(
        "{:.0}%, neck {:.0} cm",
        status.weight * 100.0,
        status.neck_error_m * 100.0
    );
    if status.weight >= 1.0 {
        Line::ok(detail)
    } else {
        Line::waiting(detail)
    }
}

fn wingsuit_arms() -> Line {
    wings_line(&procedural::published_status())
}

/// The wingsuit's arms: their weight and the hands they queued targets for.
fn wings_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::Wingsuit {
        return Line::waiting("Not in the wingsuit").with_note(status.case.label());
    }
    let hands = format_smolstr!(
        "{}",
        match status.wing_queued {
            [true, true] => "both hands",
            [true, false] => "left hand",
            [false, true] => "right hand",
            [false, false] => "no hands",
        }
    );
    let detail = format_smolstr!("{:.0}%, {}", status.wing_weight * 100.0, hands);
    if status.wing_queued == [true, true] && status.wing_weight >= 1.0 {
        Line::ok(detail)
    } else {
        Line::waiting(detail)
    }
}

fn parachute_body() -> Line {
    parachute_line(&procedural::published_status())
}

/// The torso under the parachute, on the harness's hips.
fn parachute_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::Parachute {
        return Line::waiting("Not under the canopy").with_note(status.case.label());
    }
    torso_on_hips_line(status)
}

fn swim_torso() -> Line {
    let status = procedural::published_status();
    if status.case != Case::Swimming {
        return Line::waiting("Not swimming").with_note(status.case.label());
    }
    torso_on_hips_line(&status)
}

fn roof_torso() -> Line {
    roof_line(&procedural::published_status())
}

/// The torso standing in a vehicle's stunt area, on the on-foot animation's hips.
fn roof_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::StuntStanding {
        return Line::waiting("Not on a roof").with_note(status.case.label());
    }
    torso_on_hips_line(status)
}

fn body_mirror() -> Line {
    if !CONTROLLERS_CONFIG.query(|c| c.overlay.body_mirror) {
        return Line::off("Switch on in debug UI")
            .with_note("the visualization drawer's Body mirror");
    }
    mirror_line(&mirror::status(), Stamp::now())
}

/// The body mirror: the bones it found, and whether it drew recently. The pause stops the pass, so
/// a mirror that drew before the menu opened still reads as drawing.
fn mirror_line(status: &MirrorStatus, now: Stamp) -> Line {
    match (status.bones, status.at) {
        (Some(0), _) => Line::failed("No bones found"),
        (Some(bones), Some(at)) => {
            let detail = format_smolstr!("{} bones", bones);
            if now.saturating_duration_since(at)
                < Duration::from_secs_f32(UI.get().readouts.mirror_stale_s)
            {
                Line::ok(detail)
            } else {
                Line::waiting(detail).with_note("not drawn since the menu opened or the pass ran")
            }
        }
        _ => Line::waiting("Not drawn yet"),
    }
}

fn render_rate_body() -> Line {
    presentation_line(&presentation::status())
}

/// The render-rate body: how many of the player's body draws it re-posed and its smoothed cost
/// per frame, from the last frame it was active in, which the pause menu's frames leave as they
/// were.
fn presentation_line(status: &PresentationStatus) -> Line {
    if let Some(error) = &status.rig_error {
        return Line::failed("No rig").with_note(error.to_string());
    }
    if !status.measured {
        return Line::waiting(format_smolstr!("{}", status.idle.label()));
    }
    Line::ok(format_smolstr!(
        "{} draws, {:.3} ms",
        status.draws,
        status.mean_cost_us / 1000.0
    ))
    .with_note(format!(
        "now {}, last frame {:.3} ms, peak {:.3} ms, {} ticks; elbow pole {}",
        status.idle.label(),
        status.frame_cost_us / 1000.0,
        status.peak_cost_us / 1000.0,
        status.ticks,
        elbow_note(&status.elbows)
    ))
}

/// Each arm's elbow pole: how much the arm bent toward it, and how far it turned the elbow from
/// HumanIK's, or that the arm bent toward HumanIK's elbow.
fn elbow_note(elbows: &[Option<ElbowReadout>; 2]) -> String {
    let arm = |name: &str, elbow: Option<ElbowReadout>| match elbow {
        None => format!("{name} off"),
        Some(ElbowReadout {
            weight,
            swivel_deg: Some(swivel),
        }) => format!("{name} {:.0}%, {swivel:.0}° from HumanIK", weight * 100.0),
        Some(ElbowReadout { weight, .. }) => format!("{name} {:.0}%", weight * 100.0),
    };
    format!("{}, {}", arm("left", elbows[0]), arm("right", elbows[1]))
}

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::presentation::Idle;

    use super::*;

    fn status(case: Case, weight: f32) -> ProceduralStatus {
        ProceduralStatus {
            case,
            weight,
            neck_error_m: 0.031,
            ..procedural::published_status()
        }
    }

    #[test]
    fn the_seated_torso_reads_its_weight_and_the_necks_error() {
        assert_eq!(
            seated_line(&status(Case::OnFoot, 1.0)).text(),
            "Waiting, Not seated"
        );
        assert_eq!(
            seated_line(&status(Case::Seated, 0.5)).text(),
            "Waiting, 50%, neck 3 cm"
        );
        assert_eq!(
            seated_line(&status(Case::Seated, 1.0)).text(),
            "Ok, 100%, neck 3 cm"
        );
    }

    #[test]
    fn the_parachute_reads_the_torsos_weight() {
        assert_eq!(
            parachute_line(&status(Case::Parachute, 1.0)).text(),
            "Ok, 100%, neck 3 cm"
        );
        assert_eq!(
            parachute_line(&status(Case::Parachute, 0.4)).text(),
            "Waiting, 40%, neck 3 cm"
        );
        assert_eq!(
            parachute_line(&status(Case::Flight, 0.0)).text(),
            "Waiting, Not under the canopy"
        );
    }

    #[test]
    fn the_roof_torso_reads_its_weight_on_a_roof_only() {
        assert_eq!(
            roof_line(&status(Case::Vehicle, 0.0)).text(),
            "Waiting, Not on a roof"
        );
        assert_eq!(
            roof_line(&status(Case::StuntStanding, 1.0)).text(),
            "Ok, 100%, neck 3 cm"
        );
    }

    #[test]
    fn the_mirror_reads_its_bones() {
        let now = Stamp::now();
        let drawn = MirrorStatus {
            at: Some(now),
            bones: Some(21),
            primitives: 30,
        };
        assert_eq!(mirror_line(&drawn, now).text(), "Ok, 21 bones");
        assert_eq!(
            mirror_line(
                &MirrorStatus {
                    bones: Some(0),
                    ..drawn
                },
                now
            )
            .text(),
            "Failed, No bones found"
        );
        assert_eq!(
            mirror_line(&MirrorStatus::default(), now).text(),
            "Waiting, Not drawn yet"
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
            "Waiting, not in the headset"
        );
        let active = PresentationStatus {
            measured: true,
            draws: 6,
            mean_cost_us: 21.0,
            ..PresentationStatus::default()
        };
        assert_eq!(presentation_line(&active).text(), "Ok, 6 draws, 0.021 ms");
        let unrigged = PresentationStatus {
            rig_error: Some("presentation rig: the skeleton has no bone named Neck".into()),
            ..active
        };
        assert_eq!(presentation_line(&unrigged).text(), "Failed, No rig");
    }

    #[test]
    fn the_elbow_note_names_each_arm_s_pole() {
        let elbows = [
            None,
            Some(ElbowReadout {
                weight: 1.0,
                swivel_deg: Some(12.4),
            }),
        ];
        assert_eq!(
            elbow_note(&elbows),
            "left off, right 100%, 12° from HumanIK"
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
            "Ok, 100%, both hands"
        );
        assert_eq!(
            wings_line(&flying(1.0, [false, true])).text(),
            "Waiting, 100%, right hand"
        );
        assert_eq!(
            wings_line(&status(Case::Parachute, 0.0)).text(),
            "Waiting, Not in the wingsuit"
        );
    }
}
