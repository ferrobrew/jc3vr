//! The status page's rows for the fingers: each hand's grip pose and what declared it.

use smol_str::{SmolStr, format_smolstr};

use crate::controllers::{
    fingers::{self, HandFingersStatus, target::GripMode},
    frame::Hand,
    menu::{read_config, status::Line},
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
        return Line::off("FINGER CURLS OFF");
    }
    if status.unresolved {
        return Line::failed("FINGER BONES NOT FOUND");
    }
    if status.source.is_none() {
        return Line::waiting("HAND NOT DRIVEN");
    }
    let Some(grip) = status.grip else {
        return if poses {
            Line::ok("HOLDS NOTHING")
        } else {
            Line::off("GRIP POSES OFF")
        };
    };
    let what = format_smolstr!(
        "{} FROM {}",
        grip.pose.label().to_ascii_uppercase(),
        grip.source.label().to_ascii_uppercase()
    );
    let detail: SmolStr = match grip.mode {
        GripMode::Posed | GripMode::Releasing => {
            let fitted = if grip.fitted > 0 {
                format_smolstr!(", {} FITTED", grip.fitted)
            } else {
                SmolStr::default()
            };
            let releasing = if grip.mode == GripMode::Releasing {
                ", LETTING GO"
            } else {
                ""
            };
            format_smolstr!("{what}, {:.0}%{fitted}{releasing}", grip.share * 100.0)
        }
        GripMode::GameGrip => format_smolstr!("{what}, GAME'S GRIP"),
        GripMode::PosesOff => format_smolstr!("{what}, POSES OFF"),
    };
    Line::ok(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::fingers::{
        curl::CurlSource,
        grip::{GripSource, catalogue::GripPose},
        target::GripReport,
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
