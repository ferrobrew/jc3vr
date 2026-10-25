//! Arm-pose steering for the wingsuit. Pure. The parachute steers with its handles instead
//! ([`crate::controllers::vehicles::parachute`]), which share [`combine`].
//!
//! With the arms spread as wings, the difference in the hands' heights banks (the lower hand is the
//! side the turn goes to), and one of three measures pitches ([`ArmPitch`]):
//!
//! - the wrists' tilt (the default): both hands' leading edges, the thumb side of a palm held flat,
//!   tilted down from level dive and tilted up climb, as a wing's angle of attack does. It is read
//!   from the wrist frame's back-of-hand axis in the vertical plane that runs forward from the
//!   player, so raising, lowering, or sweeping an arm does not change it, and it answers as fast as
//!   the wrist turns;
//! - the hands' height: both hands raised or lowered from the shoulders, lowered diving;
//! - the arms' sweep: both hands swept back behind the shoulders dive, and brought forward climb.
//!
//! The bank stays the hands' height difference in every mode. Each hand's tilt is the same motion
//! for both wings, so reading a difference in tilt as a bank would turn the wingsuit whenever one
//! wrist rolls a little more than the other while the player only means to pitch.
//!
//! The result is a stick vector in the move stick's convention (+X right, +Y forward), which the
//! bindings write to the move half-axes the wingsuit task reads, exactly as the stick does.
//!
//! Every measure is taken in the cockpit frame, whose vertical is the player's real vertical: the
//! player stands or sits upright while the character flies prone, so the pose reads the same in
//! any attitude. Forward is the horizontal normal to the line from the left hand to the right,
//! which is where a player with spread arms faces, standing or seated, whichever way they turned
//! since the last recenter. The shoulders come from the body dimensions below and behind the
//! tracked head, which holds seated as well as standing: the shoulders sit as far below the eyes
//! either way.
//!
//! The arms take over once they are spread past the engaging share of the arm span, and let go only
//! once they come in past a narrower share ([`SPREAD_HYSTERESIS`]), so a wing held near the
//! threshold does not flicker in and out.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::{
    body::EYES_FORWARD_OF_SHOULDERS,
    vehicles::config::{ArmPitch, ArmSteeringConfig},
};

/// One hand, as the arm steering reads it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmHand {
    /// The grip's position in the cockpit frame.
    pub position: Vec3,
    /// The wrist frame's orientation in the cockpit frame: -Z toward the fingers, +Y out of the
    /// back of the hand.
    pub wrist: Quat,
}

/// Where the pose is measured from.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmBody {
    /// The head (the eye midpoint) in the cockpit frame.
    pub head: Vec3,
    /// How far the shoulder joints sit below the eyes, metres.
    pub eyes_above_shoulders: f32,
    /// Fingertip to fingertip with the arms spread, metres.
    pub arm_span: f32,
}

/// The arm pose the steering reads.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ArmPose {
    /// The left hand's height above the right's, metres: positive banks right.
    pub bank_m: f32,
    /// The hands' mean height above the shoulders, metres.
    pub height_m: f32,
    /// The wrists' mean tilt, radians: positive with the leading edges down.
    pub tilt: f32,
    /// How far the hands' midpoint is ahead of the shoulders' midpoint, metres.
    pub sweep_m: f32,
    /// The hands' horizontal separation as a share of the arm span.
    pub spread: f32,
}

/// The pitch input the configured measure reads, before the deadzone and the scaling.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum PitchInput {
    /// The hands' mean height above the shoulders, metres.
    Height(f32),
    /// The wrists' mean tilt from the neutral, degrees, positive with the leading edges down.
    TiltDeg(f32),
    /// The hands ahead of the shoulders, metres.
    Sweep(f32),
}

/// Measure the pose of `hands` (`[left, right]`) against `body`.
pub fn arm_pose(hands: [ArmHand; 2], body: &ArmBody) -> ArmPose {
    let [left, right] = hands;
    let across = right.position - left.position;
    let forward = Vec3::Y
        .cross(across)
        .with_y(0.0)
        .try_normalize()
        .unwrap_or(Vec3::NEG_Z);
    let shoulder_y = body.head.y - body.eyes_above_shoulders;
    let chest = body.head.with_y(shoulder_y) - forward * EYES_FORWARD_OF_SHOULDERS;
    let middle = (left.position + right.position) * 0.5;
    ArmPose {
        bank_m: left.position.y - right.position.y,
        height_m: middle.y - shoulder_y,
        tilt: wrists_tilt(left.wrist, right.wrist),
        sweep_m: (middle - chest).dot(forward),
        spread: Vec2::new(across.x, across.z).length() / body.arm_span.max(0.1),
    }
}

/// The wrists' mean tilt, radians, positive with the leading edges down, for the `left` and
/// `right` wrist frames' orientations (-Z toward the fingers, +Y out of the back of the hand).
pub fn wrists_tilt(left: Quat, right: Quat) -> f32 {
    (wrist_tilt(left, 1.0) + wrist_tilt(right, -1.0)) * 0.5
}

/// The pitch input `config`'s measure reads from `pose`.
pub fn pitch_input(pose: &ArmPose, config: &ArmSteeringConfig) -> PitchInput {
    match config.pitch_by {
        ArmPitch::Height => PitchInput::Height(pose.height_m),
        ArmPitch::WristTilt => {
            PitchInput::TiltDeg(pose.tilt.to_degrees() - config.tilt_neutral_deg)
        }
        ArmPitch::Sweep => PitchInput::Sweep(pose.sweep_m),
    }
}

/// The stick vector the pose steers with, or `None` while the arms are not spread. `engaged` is
/// whether the arms steered on the last tick, which narrows the spread they need to keep steering.
pub fn steer(pose: &ArmPose, config: &ArmSteeringConfig, engaged: bool) -> Option<Vec2> {
    if pose.spread.is_nan() || pose.spread < needed_spread(config, engaged) {
        return None;
    }
    let bank = shape(pose.bank_m, config.deadzone_m, config.full_bank_m);
    let pitch = match pitch_input(pose, config) {
        PitchInput::Height(height) => shape(-height, config.deadzone_m, config.full_pitch_m),
        PitchInput::TiltDeg(tilt) => shape(tilt, config.tilt_deadzone_deg, config.full_tilt_deg),
        PitchInput::Sweep(sweep) => shape(-sweep, config.deadzone_m, config.full_sweep_m),
    };
    let pitch = if config.invert_pitch { -pitch } else { pitch };
    Some(Vec2::new(bank, pitch))
}

/// The spread, as a share of the arm span, the arms need to steer: the engaging spread, or the
/// narrower one while they already steer (`engaged`).
pub fn needed_spread(config: &ArmSteeringConfig, engaged: bool) -> f32 {
    if engaged {
        config.min_spread - SPREAD_HYSTERESIS
    } else {
        config.min_spread
    }
}

/// The arms' stick vector this tick: the pose's own (`fresh`), or, while the grapple is aimed
/// (`aiming`), the vector they steered with when the aim began (`held`). The grapple aims along the
/// off-hand, and pointing that hand at a target narrows the spread or lowers it against the other
/// hand, which would stop the steering or bank the suit; holding the last vector keeps the flight
/// line the player had while they aim.
pub fn hold_while_aiming(fresh: Option<Vec2>, held: Option<Vec2>, aiming: bool) -> Option<Vec2> {
    if aiming { held } else { fresh }
}

/// Reserve the pitch motion for braking while retaining the bilateral bank and stick fallback.
pub fn reserve_brake_pitch(arms: Option<Vec2>, reserve: bool) -> Option<Vec2> {
    arms.map(|value| if reserve { value.with_y(0.0) } else { value })
}

/// Combine the stick with the arms: their sum, each axis limited to `-1..=1`.
pub fn combine(stick: Vec2, arms: Option<Vec2>) -> Vec2 {
    (stick + arms.unwrap_or(Vec2::ZERO)).clamp(Vec2::NEG_ONE, Vec2::ONE)
}

/// How much narrower than the engaging spread the arms may come in, as a share of the arm span,
/// before they stop steering.
pub const SPREAD_HYSTERESIS: f32 = 0.1;

/// The tilt of a wrist at `wrist`, radians: its roll about the fingers' axis from the pose with the
/// back of the hand as near straight up as that axis allows, positive with the leading edge down.
/// Measured about the fingers rather than against the world, raising or sweeping the arm turns the
/// reference with it, so only the wrist's roll reads. `lead` is `1` for the left hand and `-1` for
/// the right, whose leading edges lie on opposite sides of the fingers.
fn wrist_tilt(wrist: Quat, lead: f32) -> f32 {
    let fingers = wrist * Vec3::NEG_Z;
    let back = wrist * Vec3::Y;
    let Some(level) = (Vec3::Y - fingers * fingers.y).try_normalize() else {
        return 0.0;
    };
    let tilt = lead * fingers.dot(level.cross(back)).atan2(level.dot(back));
    if tilt.is_finite() { tilt } else { 0.0 }
}

/// A signed offset over a deadzone, reaching one at `full`.
fn shape(value: f32, deadzone: f32, full: f32) -> f32 {
    let magnitude = value.abs();
    if !magnitude.is_finite() || magnitude <= deadzone {
        return 0.0;
    }
    let span = (full - deadzone).max(1e-3);
    value.signum() * ((magnitude - deadzone) / span).min(1.0)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    const CONFIG: ArmSteeringConfig = ArmSteeringConfig::new();

    const HEIGHT: ArmSteeringConfig = ArmSteeringConfig {
        pitch_by: ArmPitch::Height,
        ..CONFIG
    };

    const BODY: ArmBody = ArmBody {
        head: Vec3::new(0.0, 0.21, 0.0),
        eyes_above_shoulders: 0.21,
        arm_span: 1.8,
    };

    /// A wrist with the palm flat and down, the fingers out along the arm on `side` (-1 left, +1
    /// right), its leading edge tilted down by `tilt` radians, all about a player facing `facing`
    /// (the rotation from facing -Z).
    fn wrist(side: f32, tilt: f32, facing: Quat) -> Quat {
        // Fingers (-Z) out along the arm: the right hand's toward +X, the left's toward -X.
        let flat = Quat::from_rotation_y(-side * FRAC_PI_2);
        // Tilting the leading edge (forward) down leans the back of the hand (+Y) forward.
        facing * Quat::from_rotation_x(-tilt) * flat
    }

    fn hand(position: Vec3, side: f32, tilt: f32, facing: Quat) -> ArmHand {
        ArmHand {
            position: facing * position,
            wrist: wrist(side, tilt, facing),
        }
    }

    /// Hands spread `spread` metres apart at heights `left_y` and `right_y`, the wrists flat.
    fn pose(left_y: f32, right_y: f32, spread: f32) -> ArmPose {
        tilted(left_y, right_y, spread, 0.0)
    }

    fn tilted(left_y: f32, right_y: f32, spread: f32, tilt: f32) -> ArmPose {
        arm_pose(
            [
                hand(
                    Vec3::new(-spread * 0.5, left_y, 0.0),
                    -1.0,
                    tilt,
                    Quat::IDENTITY,
                ),
                hand(
                    Vec3::new(spread * 0.5, right_y, 0.0),
                    1.0,
                    tilt,
                    Quat::IDENTITY,
                ),
            ],
            &ArmBody {
                head: Vec3::new(0.0, BODY.eyes_above_shoulders, -EYES_FORWARD_OF_SHOULDERS),
                ..BODY
            },
        )
    }

    #[test]
    fn a_lowered_right_hand_banks_right() {
        let v = steer(&pose(0.2, -0.2, 1.6), &CONFIG, false).unwrap();
        assert!(v.x > 0.8, "{v}");
        assert!(v.y.abs() < 1e-5);
    }

    #[test]
    fn both_hands_lowered_dive_by_height_unless_inverted() {
        let v = steer(&pose(-0.2, -0.2, 1.6), &HEIGHT, false).unwrap();
        assert!(v.y > 0.5 && v.x.abs() < 1e-5, "{v}");
        let inverted = ArmSteeringConfig {
            invert_pitch: true,
            ..HEIGHT
        };
        assert!(steer(&pose(-0.2, -0.2, 1.6), &inverted, false).unwrap().y < 0.0);
        // The default reads the wrists, so lowered flat hands do not pitch.
        assert_eq!(CONFIG.pitch_by, ArmPitch::WristTilt);
        let flat = steer(&pose(-0.2, -0.2, 1.6), &CONFIG, false).unwrap();
        assert_eq!(flat.y, 0.0);
    }

    #[test]
    fn the_leading_edges_tilted_down_dive_and_up_climb() {
        let full = CONFIG.full_tilt_deg.to_radians();
        let dive = steer(&tilted(0.0, 0.0, 1.6, full), &CONFIG, false).unwrap();
        assert!((dive.y - 1.0).abs() < 1e-4, "{dive}");
        assert!(dive.x.abs() < 1e-5, "a level tilt banks nothing: {dive}");
        let climb = steer(&tilted(0.0, 0.0, 1.6, -full), &CONFIG, false).unwrap();
        assert!((climb.y + 1.0).abs() < 1e-4, "{climb}");
        // Halfway between the deadzone and full deflection pitches half.
        let half = (CONFIG.tilt_deadzone_deg + CONFIG.full_tilt_deg) * 0.5;
        let v = steer(&tilted(0.0, 0.0, 1.6, half.to_radians()), &CONFIG, false).unwrap();
        assert!((v.y - 0.5).abs() < 1e-3, "{v}");
        // Inside the deadzone, nothing.
        let small = (CONFIG.tilt_deadzone_deg * 0.5).to_radians();
        let v = steer(&tilted(0.0, 0.0, 1.6, small), &CONFIG, false).unwrap();
        assert_eq!(v.y, 0.0);
        let inverted = ArmSteeringConfig {
            invert_pitch: true,
            ..CONFIG
        };
        assert!(
            steer(&tilted(0.0, 0.0, 1.6, full), &inverted, false)
                .unwrap()
                .y
                < 0.0
        );
    }

    #[test]
    fn the_tilt_ignores_arm_raise_sweep_and_facing() {
        let tilt = 0.3;
        let reference = tilted(0.0, 0.0, 1.6, tilt).tilt;
        assert!((reference - tilt).abs() < 1e-4, "{reference}");
        // A player turned away from the cockpit's forward, with one arm raised and both swept
        // back: the tilt reads the same.
        let facing = Quat::from_rotation_y(2.0);
        let swept =
            |side: f32| Quat::from_rotation_y(side * 0.2) * wrist(side, tilt, Quat::IDENTITY);
        // The right arm raised about the horizontal axis across it.
        let right_swept = swept(1.0);
        let across = Vec3::Y.cross(right_swept * Vec3::NEG_Z).normalize();
        let raised = Quat::from_axis_angle(across, 0.4) * right_swept;
        let left = ArmHand {
            position: facing * Vec3::new(-0.8, 0.0, 0.2),
            wrist: facing * swept(-1.0),
        };
        let right = ArmHand {
            position: facing * Vec3::new(0.8, 0.3, 0.2),
            wrist: facing * raised,
        };
        let measured = arm_pose([left, right], &BODY);
        assert!((measured.tilt - tilt).abs() < 1e-3, "{measured:?}");
    }

    #[test]
    fn aiming_the_grapple_holds_the_arms_where_they_were() {
        let fresh = Some(Vec2::new(-0.8, 0.3));
        let held = Some(Vec2::new(0.2, -0.1));
        assert_eq!(hold_while_aiming(fresh, held, false), fresh);
        assert_eq!(hold_while_aiming(fresh, held, true), held);
        // Arms that were not steering when the aim began do not start while it lasts.
        assert_eq!(hold_while_aiming(fresh, None, true), None);
        assert_eq!(hold_while_aiming(None, held, false), None);
    }

    #[test]
    fn the_arms_swept_back_dive_and_forward_climb() {
        let sweep = ArmSteeringConfig {
            pitch_by: ArmPitch::Sweep,
            ..CONFIG
        };
        let at = |z: f32, facing: Quat| {
            arm_pose(
                [
                    hand(Vec3::new(-0.8, 0.0, z), -1.0, 0.0, facing),
                    hand(Vec3::new(0.8, 0.0, z), 1.0, 0.0, facing),
                ],
                &ArmBody {
                    head: facing * Vec3::new(0.0, 0.21, -EYES_FORWARD_OF_SHOULDERS),
                    ..BODY
                },
            )
        };
        for facing in [Quat::IDENTITY, Quat::from_rotation_y(-1.2)] {
            let level = at(0.0, facing);
            assert!(level.sweep_m.abs() < 1e-4, "{level:?}");
            assert_eq!(steer(&level, &sweep, false).unwrap().y, 0.0);
            let back = steer(&at(sweep.full_sweep_m, facing), &sweep, false).unwrap();
            assert!((back.y - 1.0).abs() < 1e-4, "{back}");
            let forward = steer(&at(-sweep.full_sweep_m, facing), &sweep, false).unwrap();
            assert!((forward.y + 1.0).abs() < 1e-4, "{forward}");
        }
    }

    #[test]
    fn arms_at_the_sides_steer_nothing() {
        assert_eq!(steer(&pose(0.3, -0.3, 0.4), &CONFIG, false), None);
        let nan = ArmPose {
            spread: f32::NAN,
            ..ArmPose::default()
        };
        assert_eq!(steer(&nan, &CONFIG, false), None);
    }

    #[test]
    fn spread_arms_keep_steering_until_they_come_in_past_the_hysteresis() {
        let between = CONFIG.min_spread - SPREAD_HYSTERESIS * 0.5;
        let arm_span = BODY.arm_span;
        let at = |spread: f32| pose(0.0, 0.0, spread * arm_span);
        assert_eq!(steer(&at(between), &CONFIG, false), None);
        assert!(steer(&at(between), &CONFIG, true).is_some());
        let narrow = CONFIG.min_spread - SPREAD_HYSTERESIS * 1.5;
        assert_eq!(steer(&at(narrow), &CONFIG, true), None);
    }

    #[test]
    fn seated_spread_arms_at_shoulder_height_steer_level() {
        // A seated player 1.82 m tall: the eyes 0.21 m above the shoulders, the head at the
        // cockpit's origin, the arm span 1.8 m, and the grips a hand in from the fingertips.
        let (eye_to_shoulder, arm_span, reach) = (0.21, 1.805, 0.1);
        let body = ArmBody {
            head: Vec3::ZERO,
            eyes_above_shoulders: eye_to_shoulder,
            arm_span,
        };
        let shoulder_y = -eye_to_shoulder;
        let half = arm_span * 0.5 - reach;
        let flat =
            |x: f32, z: f32| hand(Vec3::new(x, shoulder_y, z), x.signum(), 0.0, Quat::IDENTITY);
        for config in [CONFIG, HEIGHT] {
            let level = arm_pose([flat(-half, 0.0), flat(half, 0.0)], &body);
            assert!(level.spread > config.min_spread, "{level:?}");
            assert_eq!(steer(&level, &config, false), Some(Vec2::ZERO));
            // Elbows bent, hands out to the sides of a chair: still a wing.
            let bent = arm_pose([flat(-0.5, -0.1), flat(0.5, -0.1)], &body);
            assert!(steer(&bent, &config, false).is_some(), "{bent:?}");
            // Both hands in the lap, a fist apart: not a wing.
            let lap = arm_pose(
                [
                    hand(Vec3::new(-0.15, -0.6, -0.3), -1.0, 0.0, Quat::IDENTITY),
                    hand(Vec3::new(0.15, -0.6, -0.3), 1.0, 0.0, Quat::IDENTITY),
                ],
                &body,
            );
            assert_eq!(steer(&lap, &config, false), None);
        }
    }

    #[test]
    fn small_offsets_are_ignored_and_the_sum_is_limited() {
        assert_eq!(
            steer(&pose(0.01, -0.01, 1.6), &CONFIG, false),
            Some(Vec2::ZERO)
        );
        let sum = combine(Vec2::new(0.8, 0.0), Some(Vec2::new(0.8, -0.2)));
        assert_eq!(sum, Vec2::new(1.0, -0.2));
    }
}
