//! The status rows for the fingers: each hand's grip pose and what declared it.

use crate::{
    controllers::{
        fingers::{self, HandFingersStatus, grip::GripSource, target::GripMode},
        frame::Hand,
        menu::read_config,
    },
    status::{Line, Text, prose, text},
};

pub(super) fn left_grip() -> Line {
    grip(Hand::Left)
}

pub(super) fn right_grip() -> Line {
    grip(Hand::Right)
}

fn grip(hand: Hand) -> Line {
    let (curls, poses) = read_config(|c| {
        let f = &c.controllers.fingers;
        (f.enabled && c.controllers.enabled, f.grip.enabled)
    });
    grip_line(curls, poses, &fingers::status()[hand.index()])
}

fn grip_line(curls: bool, poses: bool, status: &HandFingersStatus) -> Line {
    if !curls {
        return Line::off(prose("Finger curls off"));
    }
    if status.unresolved {
        return Line::failed(prose("Finger bones not found"));
    }
    if status.source.is_none() {
        return Line::waiting(prose("Hand not driven"));
    }
    let Some(grip) = status.grip else {
        return if poses {
            Line::ok(prose("Holds nothing"))
        } else {
            Line::off(prose("Grip poses off"))
        };
    };
    let source = match grip.source {
        GripSource::C4 => prose("C4"),
        source => prose(source.label()),
    };
    let what = text!(prose(grip.pose.label()), " from ", source);
    let detail = match grip.mode {
        GripMode::Posed | GripMode::Releasing => {
            let fitted = if grip.fitted > 0 {
                text!(", ", grip.fitted.to_string(), " fitted")
            } else {
                Text::default()
            };
            let releasing = if grip.mode == GripMode::Releasing {
                prose(", letting go")
            } else {
                Text::default()
            };
            text!(
                what,
                ", ",
                format!("{:.0}%", grip.share * 100.0),
                fitted,
                releasing
            )
        }
        GripMode::GameGrip => text!(what, ", game's grip"),
        GripMode::PosesOff => text!(what, ", poses off"),
    };
    Line::ok(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::fingers::{
        curl::CurlSource, grip::catalogue::GripPose, target::GripReport,
    };

    fn driven() -> HandFingersStatus {
        HandFingersStatus {
            source: Some(CurlSource::Joints),
            ..HandFingersStatus::default()
        }
    }

    #[test]
    fn the_row_names_the_pose_and_its_source() {
        let mut status = driven();
        assert_eq!(grip_line(true, true, &status).text(), "OK, HOLDS NOTHING");
        status.grip = Some(GripReport {
            source: GripSource::Pickup,
            pose: GripPose::Wrap,
            share: 0.8,
            fitted: 4,
            mode: GripMode::Posed,
        });
        assert_eq!(
            grip_line(true, true, &status).text(),
            "OK, WRAP FROM LOOSE OBJECT, 80%, 4 FITTED"
        );
    }

    #[test]
    fn the_games_grip_keeps_c4_and_the_legacy_spelling() {
        let status = HandFingersStatus {
            grip: Some(GripReport {
                source: GripSource::C4,
                pose: GripPose::ChargePinch,
                share: 0.0,
                fitted: 0,
                mode: GripMode::GameGrip,
            }),
            ..driven()
        };
        let line = grip_line(true, true, &status);
        assert_eq!(line.text(), "OK, CHARGE PINCH FROM C4, GAME'S GRIP");
        assert_eq!(line.display_detail, "charge pinch from C4, game's grip");
    }

    #[test]
    fn the_row_says_why_nothing_is_posed() {
        let status = driven();
        assert_eq!(
            grip_line(false, true, &status).text(),
            "OFF, FINGER CURLS OFF"
        );
        assert_eq!(
            grip_line(true, false, &status).text(),
            "OFF, GRIP POSES OFF"
        );
        let idle = HandFingersStatus::default();
        assert_eq!(
            grip_line(true, true, &idle).text(),
            "WAITING, HAND NOT DRIVEN"
        );
        let broken = HandFingersStatus {
            unresolved: true,
            ..driven()
        };
        assert_eq!(
            grip_line(true, true, &broken).text(),
            "FAILED, FINGER BONES NOT FOUND"
        );
    }
}
