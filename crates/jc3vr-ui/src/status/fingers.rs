//! The status rows for the fingers: each hand's grip pose and what declared it.

use jc3vr_body::controllers::{
    fingers,
    fingers::{HandFingersStatus, target::GripMode},
};
use jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG;
use jc3vr_input::frame::Hand;
use smol_str::{SmolStr, format_smolstr};

use crate::status::{Line, Row};

pub(crate) const LEFT_GRIP: Row = Row::new("Left grip pose", left_grip);
pub(crate) const RIGHT_GRIP: Row = Row::new("Right grip pose", right_grip);

fn left_grip() -> Line {
    grip(Hand::Left)
}

fn right_grip() -> Line {
    grip(Hand::Right)
}

fn grip(hand: Hand) -> Line {
    let curls = CONTROLLERS_CONFIG.query(|c| c.enabled);
    grip_line(curls, &fingers::status()[hand.index()])
}

fn grip_line(curls: bool, status: &HandFingersStatus) -> Line {
    if !curls {
        return Line::off("Controllers off");
    }
    if status.unresolved {
        return Line::failed("Finger bones not found");
    }
    if status.source.is_none() {
        return Line::waiting("Hand not driven");
    }
    let Some(grip) = status.grip else {
        return Line::ok("Holds nothing");
    };
    let what = format_smolstr!("{} from {}", grip.pose.label(), grip.source.label());
    let fitted = if grip.fitted > 0 {
        format_smolstr!(", {} fitted", grip.fitted)
    } else {
        SmolStr::default()
    };
    let releasing = if grip.mode == GripMode::Releasing {
        SmolStr::new_static(", letting go")
    } else {
        SmolStr::default()
    };
    Line::ok(format_smolstr!(
        "{}, {:.0}%{}{}",
        what,
        grip.share * 100.0,
        fitted,
        releasing
    ))
}

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::fingers::{
        curl::CurlSource, grip::catalogue::GripPose, target::GripReport,
    };
    use jc3vr_physical::controllers::physical_hands::grip_sources as physical_grips;
    use jc3vr_weapons::controllers::weapons::grip_sources as weapon_grips;

    use super::*;

    fn driven() -> HandFingersStatus {
        HandFingersStatus {
            source: Some(CurlSource::Joints),
            ..HandFingersStatus::default()
        }
    }

    #[test]
    fn the_row_names_the_pose_and_its_source() {
        let mut status = driven();
        assert_eq!(grip_line(true, &status).text(), "Ok, Holds nothing");
        status.grip = Some(GripReport {
            source: physical_grips::PICKUP,
            pose: GripPose::Wrap,
            share: 0.8,
            fitted: 4,
            mode: GripMode::Posed,
        });
        assert_eq!(
            grip_line(true, &status).text(),
            "Ok, wrap from loose object, 80%, 4 fitted"
        );
    }

    #[test]
    fn a_c4_grip_names_its_pose_and_its_source() {
        let status = HandFingersStatus {
            grip: Some(GripReport {
                source: weapon_grips::C4,
                pose: GripPose::ChargePalm,
                share: 0.5,
                fitted: 0,
                mode: GripMode::Releasing,
            }),
            ..driven()
        };
        let line = grip_line(true, &status);
        assert_eq!(line.text(), "Ok, charge palm from C4, 50%, letting go");
        assert_eq!(line.detail, "charge palm from C4, 50%, letting go");
    }

    #[test]
    fn the_row_says_why_nothing_is_posed() {
        let status = driven();
        assert_eq!(grip_line(false, &status).text(), "Off, Controllers off");
        let idle = HandFingersStatus::default();
        assert_eq!(grip_line(true, &idle).text(), "Waiting, Hand not driven");
        let broken = HandFingersStatus {
            unresolved: true,
            ..driven()
        };
        assert_eq!(
            grip_line(true, &broken).text(),
            "Failed, Finger bones not found"
        );
    }
}
