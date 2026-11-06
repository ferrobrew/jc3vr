//! The status rows for the procedural torso outside standing play: the torso in a vehicle
//! seat, the wingsuit's arms, the torso under the parachute, the torso in the water, the torso on
//! a vehicle's roof, the body mirror, and the render-rate body. Each reads the last published
//! status, which the pause stops updating, so a row shows what the body did just before the menu
//! opened.

use std::time::{Duration, Instant};

use crate::{
    controllers::{
        body::{
            mirror::{self, MirrorStatus},
            procedural::{self, ProceduralStatus, authority::Case, config::ProceduralMode},
        },
        menu::read_config,
        presentation::{self, ElbowReadout, PresentationStatus},
    },
    status::{Line, Row, prose, text},
};

pub(crate) const RENDER_RATE_BODY: Row =
    Row::new("RENDER-RATE BODY", "Render-rate body", render_rate_body);
pub(crate) const SEATED_TORSO: Row = Row::new("SEATED TORSO", "Seated torso", seated_torso);
pub(crate) const WINGSUIT_ARMS: Row = Row::new("WINGSUIT ARMS", "Wingsuit arms", wingsuit_arms);
pub(crate) const PARACHUTE_BODY: Row = Row::new("PARACHUTE BODY", "Parachute body", parachute_body);
pub(crate) const SWIM_TORSO: Row = Row::new("SWIM TORSO", "Swim torso", swim_torso);
pub(crate) const ROOF_TORSO: Row = Row::new("ROOF TORSO", "Roof torso", roof_torso);
pub(crate) const BODY_MIRROR: Row = Row::new("BODY MIRROR", "Body mirror", body_mirror);

fn seated_torso() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) != ProceduralMode::Full {
        return Line::off("");
    }
    seated_line(&procedural::published_status())
}

/// The torso in a seat: how much of it is procedural, and how far the neck is from the point under
/// the player's head.
fn seated_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::Seated {
        return Line::waiting(prose("Not seated")).with_note(status.case.label());
    }
    torso_on_hips_line(status)
}

/// The torso on the animation's hips: its weight and the neck's error, at full weight and posed on
/// the hips once the hand-over has finished.
fn torso_on_hips_line(status: &ProceduralStatus) -> Line {
    let detail = text!(
        format!("{:.0}%", status.weight * 100.0),
        ", neck ",
        format!("{:.0}", status.neck_error_m * 100.0),
        " cm"
    );
    if status.weight >= 1.0 {
        Line::ok(detail)
    } else {
        Line::waiting(detail)
    }
}

fn wingsuit_arms() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) != ProceduralMode::Full {
        return Line::off("");
    }
    wings_line(&procedural::published_status())
}

/// The wingsuit's arms: their weight and the hands they queued targets for.
fn wings_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::Wingsuit {
        return Line::waiting(prose("Not in the wingsuit")).with_note(status.case.label());
    }
    let hands = prose(match status.wing_queued {
        [true, true] => "both hands",
        [true, false] => "left hand",
        [false, true] => "right hand",
        [false, false] => "no hands",
    });
    let detail = text!(format!("{:.0}%", status.wing_weight * 100.0), ", ", hands);
    if status.wing_queued == [true, true] && status.wing_weight >= 1.0 {
        Line::ok(detail)
    } else {
        Line::waiting(detail)
    }
}

fn parachute_body() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) != ProceduralMode::Full {
        return Line::off("");
    }
    parachute_line(&procedural::published_status())
}

/// The torso under the parachute, on the harness's hips.
fn parachute_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::Parachute {
        return Line::waiting(prose("Not under the canopy")).with_note(status.case.label());
    }
    torso_on_hips_line(status)
}

fn swim_torso() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) != ProceduralMode::Full {
        return Line::off("");
    }
    let status = procedural::published_status();
    if status.case != Case::Swimming {
        return Line::waiting(prose("Not swimming")).with_note(status.case.label());
    }
    torso_on_hips_line(&status)
}

fn roof_torso() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) != ProceduralMode::Full {
        return Line::off("");
    }
    roof_line(&procedural::published_status())
}

/// The torso standing in a vehicle's stunt area.
fn roof_line(status: &ProceduralStatus) -> Line {
    if status.case != Case::StuntStanding {
        return Line::waiting(prose("Not on a roof")).with_note(status.case.label());
    }
    torso_on_hips_line(status)
}

fn body_mirror() -> Line {
    if !read_config(|c| c.controllers.overlay.body_mirror) {
        return Line::off(prose("Switch on in debug UI"))
            .with_note("the visualization drawer's Body mirror");
    }
    mirror_line(&mirror::status(), Instant::now())
}

/// The body mirror: the bones it found, and whether it drew recently. The pause stops the pass, so
/// a mirror that drew before the menu opened still reads as drawing.
fn mirror_line(status: &MirrorStatus, now: Instant) -> Line {
    match (status.bones, status.at) {
        (Some(0), _) => Line::failed(prose("No bones found")),
        (Some(bones), Some(at)) => {
            let detail = text!(bones.to_string(), " bones");
            if now.saturating_duration_since(at) < MIRROR_STALE {
                Line::ok(detail)
            } else {
                Line::waiting(detail).with_note("not drawn since the menu opened or the pass ran")
            }
        }
        _ => Line::waiting(prose("Not drawn yet")),
    }
}

/// How long after its last pass the mirror reads as drawing.
const MIRROR_STALE: Duration = Duration::from_secs(30);

fn render_rate_body() -> Line {
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
        return Line::failed(prose("No rig")).with_note(error.to_string());
    }
    if !status.measured {
        return Line::waiting(prose(status.idle.label()));
    }
    Line::ok(text!(
        status.draws.to_string(),
        " draws, ",
        format!("{:.3}", status.mean_cost_us / 1000.0),
        " ms"
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
    use super::*;
    use crate::controllers::presentation::Idle;

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
    fn the_parachute_reads_the_torsos_weight() {
        assert_eq!(
            parachute_line(&status(Case::Parachute, 1.0)).text(),
            "OK, 100%, NECK 3 CM"
        );
        assert_eq!(
            parachute_line(&status(Case::Parachute, 0.4)).text(),
            "WAITING, 40%, NECK 3 CM"
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
