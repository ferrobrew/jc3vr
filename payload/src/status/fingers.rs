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
    let curls = read_config(|c| c.controllers.fingers.enabled && c.controllers.enabled);
    grip_line(curls, &fingers::status()[hand.index()])
}

fn grip_line(curls: bool, status: &HandFingersStatus) -> Line {
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
        return Line::ok(prose("Holds nothing"));
    };
    let source = match grip.source {
        GripSource::C4 => prose("C4"),
        source => prose(source.label()),
    };
    let what = text!(prose(grip.pose.label()), " from ", source);
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
    Line::ok(text!(
        what,
        ", ",
        format!("{:.0}%", grip.share * 100.0),
        fitted,
        releasing
    ))
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
        assert_eq!(grip_line(true, &status).text(), "OK, HOLDS NOTHING");
        status.grip = Some(GripReport {
            source: GripSource::Pickup,
            pose: GripPose::Wrap,
            share: 0.8,
            fitted: 4,
            mode: GripMode::Posed,
        });
        assert_eq!(
            grip_line(true, &status).text(),
            "OK, WRAP FROM LOOSE OBJECT, 80%, 4 FITTED"
        );
    }

    #[test]
    fn a_c4_grip_names_its_pose_and_its_source() {
        let status = HandFingersStatus {
            grip: Some(GripReport {
                source: GripSource::C4,
                pose: GripPose::ChargePalm,
                share: 0.5,
                fitted: 0,
                mode: GripMode::Releasing,
            }),
            ..driven()
        };
        let line = grip_line(true, &status);
        assert_eq!(line.text(), "OK, CHARGE PALM FROM C4, 50%, LETTING GO");
        assert_eq!(line.display_detail, "charge palm from C4, 50%, letting go");
    }

    #[test]
    fn the_row_says_why_nothing_is_posed() {
        let status = driven();
        assert_eq!(grip_line(false, &status).text(), "OFF, FINGER CURLS OFF");
        let idle = HandFingersStatus::default();
        assert_eq!(grip_line(true, &idle).text(), "WAITING, HAND NOT DRIVEN");
        let broken = HandFingersStatus {
            unresolved: true,
            ..driven()
        };
        assert_eq!(
            grip_line(true, &broken).text(),
            "FAILED, FINGER BONES NOT FOUND"
        );
    }
}
