//! The debug panel's placement math, all pure: where a summoned panel goes, when a stored placement
//! counts as behind the player, how a grab carries the panel with the hand, how the thumbstick
//! pushes it along the hand's aim ray, and how the cockpit-frame placement reaches the world.
//!
//! A panel pose is the pose of a viewer squarely in front of the panel: its position is the panel's
//! centre, its -Z points into the panel's front face, its +X is the panel's right, and its +Y the
//! panel's up. This is the rotation [`PanelParams::rot`] takes.

use glam::{Quat, Vec3};

use crate::{
    controllers::{
        frame::Pose,
        hands::pose::{body_of_head, compose_world, inverse, mul},
    },
    hud::{pointer::RenderedCamera, quad::PanelParams},
};

/// The nearest a summoned or grabbed panel is placed, metres.
pub(crate) const MIN_DISTANCE_M: f32 = 0.2;

/// The farthest the thumbstick pushes a grabbed panel along the hand's aim ray, metres.
pub(crate) const MAX_PUSH_DEPTH_M: f32 = 10.0;

/// How fast a full deflection pushes or pulls a grabbed panel, as a fraction of its depth along
/// the ray per second, so the push is as quick to read at arm's length as across a room.
pub(crate) const PUSH_RATE_PER_S: f32 = 1.5;

/// The longest interval one push covers, seconds, so a hitch does not throw the panel.
pub(crate) const MAX_PUSH_DT: f32 = 0.1;

/// The panel pose `distance` metres in front of `head`, along the gaze, facing the head, with no
/// roll: the panel's up is in the vertical plane through the gaze.
pub(crate) fn in_front_of(head: Pose, distance: f32) -> Pose {
    let distance = if distance.is_finite() {
        distance.max(MIN_DISTANCE_M)
    } else {
        MIN_DISTANCE_M
    };
    let forward = head.orientation * Vec3::NEG_Z;
    let heading = heading(head.orientation);
    let yaw = (-heading.x).atan2(-heading.z);
    let pitch = forward
        .y
        .atan2(Vec3::new(forward.x, 0.0, forward.z).length());
    let orientation = (Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch)).normalize();
    Pose {
        position: head.position + orientation * Vec3::NEG_Z * distance,
        orientation,
    }
}

/// Whether a panel centred at `center` lies behind `head`: on the far side of the vertical plane
/// through the head square to its heading. A panel straight above or below the head is not behind.
pub(crate) fn is_behind(head: Pose, center: Vec3) -> bool {
    let to_center = center - head.position;
    flat_direction(to_center).is_some_and(|flat| flat.dot(heading(head.orientation)) < 0.0)
}

/// The panel in the frame of the hand's `aim` pose at the moment of a grab, which the grab keeps.
pub(crate) fn grab_offset(aim: Pose, panel: Pose) -> Pose {
    mul(inverse(aim), panel)
}

/// The panel pose a hand at `aim` holds, with the grab's `offset`.
pub(crate) fn held_pose(aim: Pose, offset: Pose) -> Pose {
    mul(aim, offset)
}

/// The grab's `offset` after the thumbstick pushes (`rate` above zero) or pulls (below zero) the
/// panel for `dt` seconds. The panel moves along the aim ray, the hand frame's -Z, so the point the
/// ray holds stays on it. Its depth along the ray changes in proportion to itself, up to
/// [`PUSH_RATE_PER_S`] at a full deflection, and stops at [`MIN_DISTANCE_M`] and
/// [`MAX_PUSH_DEPTH_M`]; a panel already beyond a bound is never moved further past it.
pub(crate) fn push_along_ray(offset: Pose, rate: f32, dt: f32) -> Pose {
    if !rate.is_finite() || !dt.is_finite() || dt <= 0.0 || rate == 0.0 {
        return offset;
    }
    let depth = -offset.position.z;
    if !depth.is_finite() {
        return offset;
    }
    let step =
        rate.clamp(-1.0, 1.0) * PUSH_RATE_PER_S * depth.max(MIN_DISTANCE_M) * dt.min(MAX_PUSH_DT);
    let pushed = depth + step;
    let pushed = if step > 0.0 {
        pushed.min(depth.max(MAX_PUSH_DEPTH_M))
    } else {
        pushed.max(depth.min(MIN_DISTANCE_M))
    };
    Pose {
        position: Vec3::new(offset.position.x, offset.position.y, -pushed),
        orientation: offset.orientation,
    }
}

/// The world-space panel for a cockpit-frame panel `pose` of `height` metres and `aspect`, placed
/// through the rendered `camera`'s basis: the render-time head, the cockpit head pose that placed
/// it, and the world scale. This is the composition the hands and the head go through, so the
/// panel rides the tracked space with them. The returned parameters put the viewer at the head, so
/// the cursor sized from them keeps a constant apparent size.
pub(crate) fn world_panel(
    camera: &RenderedCamera,
    pose: Pose,
    height: f32,
    aspect: f32,
) -> PanelParams {
    let world = compose_world(
        camera.head.position,
        body_of_head(camera.head.orientation, camera.cockpit_head.orientation),
        camera.cockpit_head.position,
        pose,
        camera.scale,
    );
    let forward = world.orientation * Vec3::NEG_Z;
    let distance = world
        .position
        .distance(camera.head.position)
        .max(f32::EPSILON);
    PanelParams {
        pos: world.position - forward * distance,
        rot: world.orientation,
        aspect,
        distance,
        panel_height: height * camera.scale,
    }
}

/// The horizontal direction `orientation` faces. Looking straight up or down, the gaze has no
/// horizontal part, so the top of the head gives it: it points ahead when looking down and behind
/// when looking up.
fn heading(orientation: Quat) -> Vec3 {
    let forward = orientation * Vec3::NEG_Z;
    let up = orientation * Vec3::Y;
    flat_direction(forward)
        .or_else(|| flat_direction(up * -forward.y.signum()))
        .unwrap_or(Vec3::NEG_Z)
}

/// The horizontal part of `v`, normalized, or `None` when it is too short to give a direction.
fn flat_direction(v: Vec3) -> Option<Vec3> {
    let flat = Vec3::new(v.x, 0.0, v.z);
    (flat.length_squared() > MIN_FLAT_LENGTH_SQUARED).then(|| flat.normalize())
}

/// The squared length below which a horizontal part is rounding noise rather than a direction.
const MIN_FLAT_LENGTH_SQUARED: f32 = 1e-8;

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use glam::EulerRot;

    use super::*;
    use crate::hud::quad::compute_world_corners;

    fn pose(position: Vec3, orientation: Quat) -> Pose {
        Pose {
            position,
            orientation,
        }
    }

    fn assert_near(a: Vec3, b: Vec3) {
        assert!(a.abs_diff_eq(b, 1e-4), "{a} != {b}");
    }

    fn assert_same_rotation(a: Quat, b: Quat) {
        assert!(a.dot(b).abs() > 1.0 - 1e-5, "{a} != {b}");
    }

    #[test]
    fn a_summoned_panel_faces_the_head_along_its_gaze_without_roll() {
        let rotation = Quat::from_euler(EulerRot::YXZ, 0.7, -0.3, 0.5);
        let head = pose(Vec3::new(0.2, 1.6, -0.1), rotation);
        let panel = in_front_of(head, 2.0);
        assert_near(panel.position, head.position + rotation * Vec3::NEG_Z * 2.0);
        // Facing the head: the panel's front looks back along the gaze.
        assert_near(panel.orientation * Vec3::NEG_Z, rotation * Vec3::NEG_Z);
        // No roll: the panel's right is level.
        assert!((panel.orientation * Vec3::X).y.abs() < 1e-5);
        assert!((panel.orientation * Vec3::Y).y > 0.0);
    }

    #[test]
    fn a_summoned_panel_ignores_a_bad_or_tiny_distance() {
        let head = pose(Vec3::ZERO, Quat::IDENTITY);
        for distance in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let panel = in_front_of(head, distance);
            assert_near(panel.position, Vec3::NEG_Z * MIN_DISTANCE_M);
        }
    }

    #[test]
    fn a_summoned_panel_keeps_the_heading_looking_straight_down_or_up() {
        let yaw = Quat::from_rotation_y(0.9);
        let ahead = yaw * Vec3::NEG_Z;
        for pitch in [-FRAC_PI_2, FRAC_PI_2] {
            let head = pose(Vec3::ZERO, yaw * Quat::from_rotation_x(pitch));
            let panel = in_front_of(head, 1.0);
            assert!(panel.position.is_finite());
            assert_near(panel.position, Vec3::Y * pitch.signum());
            // The panel's up leans toward the heading the head had.
            let up = panel.orientation * Vec3::Y;
            assert!(up.dot(ahead) * -pitch.signum() > 0.99, "{pitch}: {up}");
        }
    }

    #[test]
    fn behind_is_past_the_plane_square_to_the_heading() {
        let head = pose(
            Vec3::new(1.0, 1.7, 1.0),
            Quat::from_euler(EulerRot::YXZ, 0.0, 0.6, 0.3),
        );
        let at = |offset: Vec3| is_behind(head, head.position + offset);
        assert!(!at(Vec3::new(0.0, -0.5, -2.0)));
        assert!(!at(Vec3::new(1.5, 0.0, -0.1)));
        assert!(at(Vec3::new(1.5, 0.0, 0.1)));
        assert!(at(Vec3::new(0.0, 0.3, 2.0)));
        assert!(
            !at(Vec3::new(0.0, 2.0, 0.0)),
            "straight above is not behind"
        );
    }

    #[test]
    fn behind_follows_the_heading_when_looking_straight_down() {
        let yaw = Quat::from_rotation_y(FRAC_PI_2);
        let head = pose(Vec3::ZERO, yaw * Quat::from_rotation_x(-FRAC_PI_2));
        // The heading is the yaw's forward, -X.
        assert!(!is_behind(head, Vec3::new(-2.0, 0.0, 0.0)));
        assert!(is_behind(head, Vec3::new(2.0, 0.0, 0.0)));
    }

    #[test]
    fn a_grab_carries_the_panel_rigidly_with_the_hand() {
        let panel = in_front_of(pose(Vec3::new(0.0, 1.6, 0.0), Quat::IDENTITY), 1.5);
        let aim = pose(
            Vec3::new(0.3, 1.2, -0.4),
            Quat::from_euler(EulerRot::YXZ, -0.2, 0.4, 0.1),
        );
        let offset = grab_offset(aim, panel);
        let unmoved = held_pose(aim, offset);
        assert_near(unmoved.position, panel.position);
        assert_same_rotation(unmoved.orientation, panel.orientation);

        // Moving the hand by a rigid motion moves the panel by the same motion.
        let motion = pose(Vec3::new(-0.5, 0.2, 0.7), Quat::from_rotation_y(1.1));
        let moved = held_pose(mul(motion, aim), offset);
        let expected = mul(motion, panel);
        assert_near(moved.position, expected.position);
        assert_same_rotation(moved.orientation, expected.orientation);
    }

    #[test]
    fn the_stick_pushes_and_pulls_along_the_ray() {
        let offset = pose(Vec3::new(0.1, -0.2, -1.0), Quat::from_rotation_z(0.3));
        let pushed = push_along_ray(offset, 1.0, 0.05);
        assert!(pushed.position.z < offset.position.z);
        assert_eq!(pushed.position.truncate(), offset.position.truncate());
        assert_eq!(pushed.orientation, offset.orientation);
        let expected = 1.0 + PUSH_RATE_PER_S * 0.05;
        assert!((-pushed.position.z - expected).abs() < 1e-5);
        let pulled = push_along_ray(offset, -0.5, 0.05);
        assert!(-pulled.position.z < 1.0 && -pulled.position.z > 0.9);
    }

    #[test]
    fn the_push_stops_at_its_bounds() {
        let offset = pose(Vec3::new(0.0, 0.0, -0.25), Quat::IDENTITY);
        let mut pulled = offset;
        for _ in 0..100 {
            pulled = push_along_ray(pulled, -1.0, 0.1);
        }
        assert_eq!(-pulled.position.z, MIN_DISTANCE_M);
        let mut pushed = offset;
        for _ in 0..200 {
            pushed = push_along_ray(pushed, 1.0, 0.1);
        }
        assert_eq!(-pushed.position.z, MAX_PUSH_DEPTH_M);
    }

    #[test]
    fn a_panel_past_a_bound_is_not_moved_further_past_it() {
        // Grabbed side-on, the panel's centre can sit beside or behind the hand.
        let behind = pose(Vec3::new(0.5, 0.0, 0.1), Quat::IDENTITY);
        assert_eq!(push_along_ray(behind, -1.0, 0.1), behind);
        let pushed = push_along_ray(behind, 1.0, 0.1);
        assert!(pushed.position.z < behind.position.z);
        let far = pose(Vec3::new(0.0, 0.0, -20.0), Quat::IDENTITY);
        assert_eq!(push_along_ray(far, 1.0, 0.1), far);
        assert!(push_along_ray(far, -1.0, 0.1).position.z > far.position.z);
    }

    #[test]
    fn the_push_ignores_bad_input_and_caps_a_hitch() {
        let offset = pose(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
        assert_eq!(push_along_ray(offset, f32::NAN, 0.1), offset);
        assert_eq!(push_along_ray(offset, 1.0, f32::NAN), offset);
        assert_eq!(push_along_ray(offset, 1.0, 0.0), offset);
        assert_eq!(
            push_along_ray(offset, 1.0, 5.0),
            push_along_ray(offset, 1.0, MAX_PUSH_DT)
        );
    }

    #[test]
    fn the_world_panel_rides_the_tracked_space() {
        let cockpit_head = pose(Vec3::new(0.1, 1.6, 0.2), Quat::from_rotation_y(0.4));
        let body = Quat::from_euler(EulerRot::YXZ, 1.3, 0.2, -0.1);
        let anchor = Vec3::new(100.0, 20.0, -50.0);
        let scale = 1.5;
        let camera = RenderedCamera {
            head: pose(anchor, body * cockpit_head.orientation),
            cockpit_head,
            scale,
        };
        let panel = pose(
            Vec3::new(0.4, 1.5, -1.8),
            Quat::from_euler(EulerRot::YXZ, 0.1, -0.2, 0.0),
        );
        let params = world_panel(&camera, panel, 0.8, 2.0);
        let corners = compute_world_corners(&params).unwrap();
        let to_world = |p: Vec3| anchor + body * ((p - cockpit_head.position) * scale);
        let (right, up) = (panel.orientation * Vec3::X, panel.orientation * Vec3::Y);
        let expected = [(-0.8, 0.4), (0.8, 0.4), (-0.8, -0.4), (0.8, -0.4)]
            .map(|(dx, dy)| to_world(panel.position + right * dx + up * dy));
        for (corner, expected) in corners.iter().zip(expected) {
            assert_near(corner.truncate(), expected);
        }
        assert!((params.distance - (to_world(panel.position) - anchor).length()).abs() < 1e-3);
    }
}
