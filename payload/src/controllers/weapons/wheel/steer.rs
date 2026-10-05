//! The weapon wheel's steering geometry: how far and which way the steering hand has gone from
//! where the wheel opened, and which arm of the cross that picks. Pure.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::{
    frame::Pose,
    weapons::wheel::{config::SteerMode, select::WheelSlot},
};

/// The steering hand's frame when the wheel opened: where it pointed, where it was, and the
/// directions the cross's arms lie in.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SteerOrigin {
    /// The controller's aim direction at the open.
    forward: Vec3,
    /// The aim's own right and up, perpendicular to [`forward`](Self::forward): the turn toward
    /// them is the pointed deflection.
    aim_right: Vec3,
    aim_up: Vec3,
    /// The controller's grip position at the open.
    grip: Vec3,
    /// The head's level right at the open: the moved deflection's horizontal axis. Its vertical
    /// axis is the cockpit frame's up.
    head_right: Vec3,
}

impl SteerOrigin {
    /// The origin for a hand whose aim pose is `aim` and grip position `grip`, under a head of
    /// orientation `head`, all in the cockpit frame (+Y up, -Z forward). `None` for a non-finite
    /// pose.
    pub fn new(head: Quat, aim: Pose, grip: Vec3) -> Option<Self> {
        let forward = (aim.orientation * Vec3::NEG_Z).try_normalize()?;
        let head_right = level(head * Vec3::X)
            .or_else(|| level(aim.orientation * Vec3::X))
            .unwrap_or(Vec3::X);
        // The aim's level right; straight up or down, the head's.
        let aim_right = forward
            .cross(Vec3::Y)
            .try_normalize()
            .or_else(|| forward.cross(head_right.cross(forward)).try_normalize())?;
        let aim_up = aim_right.cross(forward).try_normalize()?;
        grip.is_finite().then_some(Self {
            forward,
            aim_right,
            aim_up,
            grip,
            head_right,
        })
    }

    /// The hand's deflection from the origin, +X toward the cross's right arm and +Y toward its
    /// up arm: the aim's turn in degrees under [`SteerMode::Point`], the grip's offset in metres
    /// under [`SteerMode::Offset`]. Zero for a non-finite pose.
    pub fn deflection(&self, mode: SteerMode, aim: Pose, grip: Vec3) -> Vec2 {
        let deflection = match mode {
            SteerMode::Point => {
                let f = aim.orientation * Vec3::NEG_Z;
                let ahead = f.dot(self.forward);
                Vec2::new(
                    f.dot(self.aim_right).atan2(ahead).to_degrees(),
                    f.dot(self.aim_up).atan2(ahead).to_degrees(),
                )
            }
            SteerMode::Offset => {
                let d = grip - self.grip;
                Vec2::new(d.dot(self.head_right), d.y)
            }
        };
        if deflection.is_finite() {
            deflection
        } else {
            Vec2::ZERO
        }
    }
}

/// Where the hand points on the wheel, as the steering keeps it across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Aimed {
    /// Within the centre: no arm.
    Centre,
    /// Toward an arm.
    Arm(WheelSlot),
}

/// Step the aimed arm for `deflection`. Leaving the centre takes `leave`; coming back takes
/// `back`, nearer the centre, so that a hand resting near the edge does not flicker. An arm stays
/// chosen until the deflection is `hysteresis_deg` past the line to the next.
pub fn aim(previous: Aimed, deflection: Vec2, leave: f32, back: f32, hysteresis_deg: f32) -> Aimed {
    let radius = deflection.length();
    if !radius.is_finite() {
        return previous;
    }
    let angle = clockwise_from_up(deflection);
    match previous {
        Aimed::Centre if radius >= leave => Aimed::Arm(sector(angle, None, hysteresis_deg)),
        Aimed::Centre => Aimed::Centre,
        Aimed::Arm(_) if radius < back.min(leave) => Aimed::Centre,
        Aimed::Arm(arm) => Aimed::Arm(sector(angle, Some(arm), hysteresis_deg)),
    }
}

/// The arm whose quarter of the wheel holds `angle` (degrees clockwise from up), keeping
/// `previous` until `angle` is more than `hysteresis_deg` outside its quarter.
pub fn sector(angle: f32, previous: Option<WheelSlot>, hysteresis_deg: f32) -> WheelSlot {
    let hysteresis = hysteresis_deg.clamp(0.0, 44.0);
    if let Some(arm) = previous
        && angular_distance(angle, arm.centre_deg()) <= 45.0 + hysteresis
    {
        return arm;
    }
    WheelSlot::ALL
        .into_iter()
        .min_by(|a, b| {
            angular_distance(angle, a.centre_deg())
                .total_cmp(&angular_distance(angle, b.centre_deg()))
        })
        .unwrap_or(WheelSlot::DualWield)
}

/// `v` levelled onto the horizontal plane and normalised, or `None` when it is vertical.
fn level(v: Vec3) -> Option<Vec3> {
    Vec3::new(v.x, 0.0, v.z).try_normalize()
}

/// The direction of `v` in degrees clockwise from +Y, in `0..360`.
fn clockwise_from_up(v: Vec2) -> f32 {
    v.x.atan2(v.y).to_degrees().rem_euclid(360.0)
}

/// The unsigned angle between two directions in degrees, in `0..=180`.
fn angular_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    fn pose(orientation: Quat) -> Pose {
        Pose {
            position: Vec3::ZERO,
            orientation,
        }
    }

    #[test]
    fn each_quarter_picks_its_arm() {
        for (angle, arm) in [
            (0.0, WheelSlot::DualWield),
            (90.0, WheelSlot::TwoHanded),
            (180.0, WheelSlot::Explosives),
            (270.0, WheelSlot::Special),
            (44.0, WheelSlot::DualWield),
            (46.0, WheelSlot::TwoHanded),
            (350.0, WheelSlot::DualWield),
        ] {
            assert_eq!(sector(angle, None, 10.0), arm, "{angle}");
        }
    }

    #[test]
    fn an_arm_holds_until_past_the_hysteresis() {
        let up = Some(WheelSlot::DualWield);
        assert_eq!(sector(50.0, up, 10.0), WheelSlot::DualWield);
        assert_eq!(sector(54.9, up, 10.0), WheelSlot::DualWield);
        assert_eq!(sector(56.0, up, 10.0), WheelSlot::TwoHanded);
        assert_eq!(sector(306.0, up, 10.0), WheelSlot::DualWield);
        assert_eq!(sector(304.0, up, 10.0), WheelSlot::Special);
    }

    #[test]
    fn the_centre_is_left_and_rejoined_at_different_radii() {
        let right = Vec2::new(1.0, 0.0);
        assert_eq!(
            aim(Aimed::Centre, right * 10.0, 12.0, 7.0, 10.0),
            Aimed::Centre
        );
        let arm = aim(Aimed::Centre, right * 12.0, 12.0, 7.0, 10.0);
        assert_eq!(arm, Aimed::Arm(WheelSlot::TwoHanded));
        assert_eq!(aim(arm, right * 8.0, 12.0, 7.0, 10.0), arm);
        assert_eq!(aim(arm, right * 6.0, 12.0, 7.0, 10.0), Aimed::Centre);
    }

    #[test]
    fn a_non_finite_deflection_keeps_the_aim() {
        let arm = Aimed::Arm(WheelSlot::Explosives);
        assert_eq!(aim(arm, Vec2::NAN, 12.0, 7.0, 10.0), arm);
    }

    #[test]
    fn pointing_turns_read_in_the_aims_frame() {
        let origin = SteerOrigin::new(Quat::IDENTITY, pose(Quat::IDENTITY), Vec3::ZERO).unwrap();
        // A turn of the aim to the right (a negative yaw about +Y) and up (a positive pitch).
        let right = Quat::from_rotation_y(-20f32.to_radians());
        let d = origin.deflection(SteerMode::Point, pose(right), Vec3::ZERO);
        assert!((d.x - 20.0).abs() < 1e-3 && d.y.abs() < 1e-3, "{d:?}");
        let up = Quat::from_rotation_x(15f32.to_radians());
        let d = origin.deflection(SteerMode::Point, pose(up), Vec3::ZERO);
        assert!(d.x.abs() < 1e-3 && (d.y - 15.0).abs() < 1e-3, "{d:?}");
    }

    #[test]
    fn pointing_works_from_an_aim_pointing_down() {
        // The controller pointed straight down: the head's right stands in for the aim's.
        let down = Quat::from_rotation_x(-FRAC_PI_2);
        let origin = SteerOrigin::new(Quat::IDENTITY, pose(down), Vec3::ZERO).unwrap();
        let turned = Quat::from_rotation_z(-10f32.to_radians()) * down;
        let d = origin.deflection(SteerMode::Point, pose(turned), Vec3::ZERO);
        assert!(d.is_finite() && d.length() > 5.0, "{d:?}");
    }

    #[test]
    fn moving_reads_in_the_heads_level_frame() {
        // The head turned to face +X: its right is +Z.
        let head = Quat::from_rotation_y(-FRAC_PI_2);
        let origin =
            SteerOrigin::new(head, pose(Quat::IDENTITY), Vec3::new(0.1, 1.0, 0.0)).unwrap();
        let d = origin.deflection(
            SteerMode::Offset,
            pose(Quat::IDENTITY),
            Vec3::new(0.1, 1.03, 0.05),
        );
        assert!(
            (d.x - 0.05).abs() < 1e-5 && (d.y - 0.03).abs() < 1e-5,
            "{d:?}"
        );
    }

    #[test]
    fn a_non_finite_pose_has_no_origin() {
        assert!(SteerOrigin::new(Quat::IDENTITY, pose(Quat::IDENTITY), Vec3::NAN).is_none());
    }
}
