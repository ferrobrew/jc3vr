//! Rico's arms in the wingsuit, from the player's. Pure.
//!
//! The player stands or sits upright with the arms spread as wings while Rico flies prone, so the
//! player's arms cannot be put where the hands are, as the hand IK does on foot: the flight pose's
//! shoulders are not where an upright body's are. Instead each arm is carried over as the arm
//! steering reads it ([`crate::controllers::vehicles::arm_steering`]): in a frame that keeps the
//! vertical and turns with the heading. The player's hand, measured from the player's estimated
//! shoulder in the frame of the player's torso heading (the cockpit frame's vertical is the
//! player's real vertical), is scaled from the player's arm to Rico's and placed from Rico's
//! shoulder in the frame of Rico's flight heading, with world up. So the spread is the spread, a
//! hand lowered for a bank is lowered toward the ground, a hand swept forward goes toward where
//! Rico flies, and the wrist's tilt for the pitch turns Rico's wrist the same way. The wrist's
//! orientation is carried over by the same turn.

use glam::{Mat4, Quat, Vec3};

use crate::controllers::frame::Pose;

/// The player's arms, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PlayerArms {
    /// The player's torso heading: a turn about the vertical from facing -Z.
    pub torso: Quat,
    /// Each shoulder joint, estimated from the head, left then right.
    pub shoulders: [Vec3; 2],
    /// Each tracked wrist frame (-Z toward the fingers, +Y out of the back of the hand), left then
    /// right; `None` for a hand not tracked.
    pub wrists: [Option<Pose>; 2],
    /// The player's arm, shoulder joint to wrist, metres.
    pub arm_length: f32,
}

/// Rico's arms, in his model space and the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RicoArms {
    /// The root's world transform.
    pub world: Mat4,
    /// Each shoulder joint in model space, left then right, as the animation has it.
    pub shoulders: [Vec3; 2],
    /// Each arm, shoulder joint to wrist, metres, left then right.
    pub arm_lengths: [f32; 2],
    /// Where Rico flies, horizontally: a unit vector in the world.
    pub heading: Vec3,
}

/// A wrist target in Rico's model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WingTarget {
    pub position: Vec3,
    /// The wrist frame's orientation, in the same convention as the player's.
    pub rotation: Quat,
}

/// Each of Rico's wrist targets for the player's arms, left then right; `None` for a hand that is
/// not tracked, or for both when either frame is degenerate.
pub fn wing_targets(player: &PlayerArms, rico: &RicoArms) -> [Option<WingTarget>; 2] {
    let player_heading = flat_turn(player.torso * Vec3::NEG_Z);
    let rico_heading = flat_turn(rico.heading);
    let (_, root, _) = rico.world.to_scale_rotation_translation();
    let to_model = rico.world.inverse();
    let (Some(player_heading), Some(rico_heading)) = (player_heading, rico_heading) else {
        return [None; 2];
    };
    if !(player.arm_length > MIN_ARM_M && player.arm_length.is_finite()) {
        return [None; 2];
    }
    // The turn from the player's heading frame to Rico's: both keep the vertical.
    let carry = (rico_heading * player_heading.inverse()).normalize();
    [0, 1].map(|i| {
        let wrist = player.wrists[i]?;
        let scale = (rico.arm_lengths[i] / player.arm_length).clamp(MIN_SCALE, MAX_SCALE);
        let reach = carry * ((wrist.position - player.shoulders[i]) * scale);
        let shoulder = rico.world.transform_point3(rico.shoulders[i]);
        let target = WingTarget {
            position: to_model.transform_point3(shoulder + reach),
            rotation: (root.inverse() * carry * wrist.orientation).normalize(),
        };
        (target.position.is_finite() && target.rotation.is_finite()).then_some(target)
    })
}

/// Where Rico flies, horizontally, for the root at `world` with the hips and the neck at `hips` and
/// `neck` in model space: the spine's line from the hips to the neck, which a prone flyer holds
/// along the flight, or the root's forward where the spine is near vertical (a dive straight down).
pub fn flight_heading(world: &Mat4, hips: Vec3, neck: Vec3) -> Option<Vec3> {
    let spine = world.transform_vector3(neck - hips).with_y(0.0);
    let forward = world.transform_vector3(Vec3::NEG_Z).with_y(0.0);
    let up = world.transform_vector3(Vec3::Y).with_y(0.0);
    [spine, forward, up]
        .into_iter()
        .find(|v| v.is_finite() && v.length() > MIN_FLAT)
        .and_then(Vec3::try_normalize)
}

/// The turn about +Y from facing -Z to facing `direction` laid flat, or `None` for a direction
/// that is near vertical.
fn flat_turn(direction: Vec3) -> Option<Quat> {
    let flat = direction.with_y(0.0);
    (flat.is_finite() && flat.length() > MIN_FLAT)
        .then(|| Quat::from_rotation_arc(Vec3::NEG_Z, flat.normalize()))
}

/// The shortest player arm the targets are scaled from, metres.
const MIN_ARM_M: f32 = 0.2;

/// The bounds on the scale from the player's arm to Rico's.
const MIN_SCALE: f32 = 0.5;
const MAX_SCALE: f32 = 2.0;

/// How long a direction laid flat must be to have a heading of its own.
const MIN_FLAT: f32 = 0.2;

#[cfg(test)]
mod tests {
    use super::*;

    /// A player standing at the origin facing -Z, the shoulders at 1.45 m and 0.2 m either side.
    fn player(wrists: [Vec3; 2]) -> PlayerArms {
        PlayerArms {
            torso: Quat::IDENTITY,
            shoulders: [Vec3::new(-0.2, 1.45, 0.0), Vec3::new(0.2, 1.45, 0.0)],
            wrists: wrists.map(|position| {
                Some(Pose {
                    position,
                    orientation: Quat::IDENTITY,
                })
            }),
            arm_length: 0.6,
        }
    }

    /// Rico flying north (-Z in the world), prone: the root pitched forward a quarter turn at
    /// 100 m, the shoulders where a prone flyer's are in model space.
    fn rico() -> RicoArms {
        let world = Mat4::from_rotation_translation(
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            Vec3::new(10.0, 100.0, 5.0),
        );
        RicoArms {
            world,
            shoulders: [Vec3::new(-0.2, 1.4, 0.0), Vec3::new(0.2, 1.4, 0.0)],
            arm_lengths: [0.6, 0.6],
            heading: Vec3::NEG_Z,
        }
    }

    fn world_targets(player: &PlayerArms, rico: &RicoArms) -> [Vec3; 2] {
        wing_targets(player, rico).map(|t| rico.world.transform_point3(t.unwrap().position))
    }

    #[test]
    fn spread_arms_spread_rico_level_from_his_shoulders() {
        let rico = rico();
        let player = player([Vec3::new(-0.8, 1.45, 0.0), Vec3::new(0.8, 1.45, 0.0)]);
        let [left, right] = world_targets(&player, &rico);
        let shoulders = rico.shoulders.map(|s| rico.world.transform_point3(s));
        assert!((left - shoulders[0]).abs_diff_eq(Vec3::new(-0.6, 0.0, 0.0), 1e-4));
        assert!((right - shoulders[1]).abs_diff_eq(Vec3::new(0.6, 0.0, 0.0), 1e-4));
    }

    #[test]
    fn a_lowered_hand_lowers_ricos_toward_the_ground_and_a_swept_hand_goes_ahead() {
        let rico = rico();
        let level = world_targets(
            &player([Vec3::new(-0.8, 1.45, 0.0), Vec3::new(0.8, 1.45, 0.0)]),
            &rico,
        );
        let banked = world_targets(
            &player([Vec3::new(-0.75, 1.2, 0.0), Vec3::new(0.8, 1.45, -0.3)]),
            &rico,
        );
        assert!(banked[0].y < level[0].y - 0.2, "{banked:?}");
        // Forward for the player is where Rico flies.
        assert!(banked[1].z < level[1].z - 0.25, "{banked:?}");
    }

    #[test]
    fn the_player_turned_away_maps_by_the_torso_not_the_room() {
        let rico = rico();
        let mut turned = player([Vec3::ZERO; 2]);
        let turn = Quat::from_rotation_y(1.0);
        turned.torso = turn;
        turned.shoulders = turned.shoulders.map(|s| turn * s);
        turned.wrists = [Vec3::new(-0.8, 1.45, 0.0), Vec3::new(0.8, 1.45, 0.0)].map(|p| {
            Some(Pose {
                position: turn * p,
                orientation: turn,
            })
        });
        let straight = player([Vec3::new(-0.8, 1.45, 0.0), Vec3::new(0.8, 1.45, 0.0)]);
        let a = wing_targets(&turned, &rico);
        let b = wing_targets(&straight, &rico);
        for i in 0..2 {
            let (a, b) = (a[i].unwrap(), b[i].unwrap());
            assert!(a.position.abs_diff_eq(b.position, 1e-4));
            assert!(a.rotation.angle_between(b.rotation) < 1e-3);
        }
    }

    #[test]
    fn a_tilted_wrist_tilts_ricos_the_same_way_in_the_world() {
        let rico = rico();
        let mut tilted = player([Vec3::new(-0.8, 1.45, 0.0), Vec3::new(0.8, 1.45, 0.0)]);
        let tilt = Quat::from_rotation_x(0.3);
        tilted.wrists[0] = tilted.wrists[0].map(|w| Pose {
            orientation: tilt,
            ..w
        });
        let target = wing_targets(&tilted, &rico)[0].unwrap();
        let (_, root, _) = rico.world.to_scale_rotation_translation();
        let world = root * target.rotation;
        assert!(world.angle_between(tilt) < 1e-4);
    }

    #[test]
    fn the_arm_is_scaled_to_ricos_and_an_untracked_hand_has_no_target() {
        let mut rico = rico();
        rico.arm_lengths = [0.9, 0.9];
        let mut p = player([Vec3::new(-0.8, 1.45, 0.0), Vec3::new(0.8, 1.45, 0.0)]);
        p.wrists[1] = None;
        let targets = wing_targets(&p, &rico);
        let left = rico.world.transform_point3(targets[0].unwrap().position);
        let shoulder = rico.world.transform_point3(rico.shoulders[0]);
        assert!((left.distance(shoulder) - 0.9).abs() < 1e-4);
        assert!(targets[1].is_none());
    }

    #[test]
    fn the_heading_follows_the_spine_and_falls_back_in_a_dive() {
        let rico = rico();
        // Prone, the model's +Y (the spine) points along the flight.
        let heading = flight_heading(
            &rico.world,
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 1.5, 0.0),
        )
        .unwrap();
        assert!(heading.abs_diff_eq(Vec3::NEG_Z, 1e-4), "{heading}");
        // Head down, the spine is vertical: the root's forward lies flat instead.
        let dive = Mat4::from_rotation_x(std::f32::consts::PI);
        let heading =
            flight_heading(&dive, Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.5, 0.0)).unwrap();
        assert!(heading.abs_diff_eq(Vec3::Z, 1e-4), "{heading}");
    }
}
