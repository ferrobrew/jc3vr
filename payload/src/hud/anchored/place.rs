//! Where the hand- and wrist-anchored readouts go, and when they show. Pure.

use body_rig::smooth::half_life_factor;
use glam::{Mat3, Quat, Vec2, Vec3};

use crate::controllers::{
    frame::{Hand, Pose},
    summary::glance::PanelPose,
};

/// An ammunition counter's centre for a quad `height` metres tall: straight up in the world from the
/// gun hand's `grip` until the quad's lower edge is `clearance` metres above it, then `pull`
/// metres toward the `eye`. The hand's tilt does not move it, so the quad stays clear of the hand
/// and the gun however the gun is held.
pub(crate) fn ammo_anchor(grip: Vec3, eye: Vec3, clearance: f32, height: f32, pull: f32) -> Vec3 {
    pull_toward(grip + WORLD_UP * (clearance + height * 0.5), eye, pull)
}

/// Whether the gun is raised to the eye to sight along it: the gun hand's `grip` within
/// [`SIGHT_REACH_M`] of the `eye`, and the hand's aim `aim_forward` within [`SIGHT_CONE_DEG`] of
/// the `gaze`. A counter above the hand would then sit in the sight picture.
pub(crate) fn sighting(grip: Vec3, aim_forward: Vec3, eye: Vec3, gaze: Vec3) -> bool {
    let (Some(aim), Some(gaze)) = (aim_forward.try_normalize(), gaze.try_normalize()) else {
        return false;
    };
    grip.distance(eye) <= SIGHT_REACH_M && aim.dot(gaze) >= SIGHT_CONE_DEG.to_radians().cos()
}

/// How near the eye the gun hand's grip is when the gun is raised to sight along it, metres.
pub(crate) const SIGHT_REACH_M: f32 = 0.4;

/// How far off the gaze the gun may point and still be sighted along, degrees.
pub(crate) const SIGHT_CONE_DEG: f32 = 15.0;

/// The tether readout's centre for a quad `height` metres tall: `back` metres along the forearm
/// from the `wrist` (whose +Z points toward the elbow), then straight up in the world until the
/// quad's lower edge is `clearance` metres above the forearm.
pub(crate) fn forearm_anchor(wrist: Pose, back: f32, clearance: f32, height: f32) -> Vec3 {
    wrist.position
        + wrist.orientation * Vec3::new(0.0, 0.0, back)
        + WORLD_UP * (clearance + height * 0.5)
}

/// Ease `current` toward `target` with a critically damped exponential of `halflife` seconds over
/// `dt`. `None`, or a jump farther than `snap` metres (a hand change, a teleport), snaps.
pub(crate) fn follow(
    current: Option<Vec3>,
    target: Vec3,
    dt: f32,
    halflife: f32,
    snap: f32,
) -> Vec3 {
    let Some(current) = current.filter(|c| c.is_finite() && c.distance(target) <= snap) else {
        return target;
    };
    current.lerp(target, half_life_factor(dt, halflife))
}

/// The corners (top left, top right, bottom left, bottom right) of a `width` by `height` quad
/// centred on `center`, facing `head`, with its up following the head's up so it rolls with the
/// view. `None` when the head is at the centre.
pub(crate) fn facing_corners(
    center: Vec3,
    head: Vec3,
    head_up: Vec3,
    width: f32,
    height: f32,
) -> Option<[Vec3; 4]> {
    let back = (head - center).try_normalize()?;
    let right = head_up.cross(back).try_normalize()?;
    let up = back.cross(right);
    let (r, u) = (right * (width * 0.5), up * (height * 0.5));
    Some([
        center - r + u,
        center + r + u,
        center - r - u,
        center + r - u,
    ])
}

/// The corners of a `width` by `height` quad centred on `center`, facing `eye` and upright: its top
/// edge stays level in the world, so it reads like a sign held up rather than rolling with the
/// head. Seen from nearly straight above or below, where the world's up leaves no level to keep,
/// its up blends to `head_up`. `None` when the eye is at the centre.
pub(crate) fn upright_corners(
    center: Vec3,
    eye: Vec3,
    head_up: Vec3,
    width: f32,
    height: f32,
) -> Option<[Vec3; 4]> {
    let back = (eye - center).try_normalize()?;
    let (level, vertical) = UPRIGHT_BLEND;
    let steep = ((back.y.abs() - level) / (vertical - level)).clamp(0.0, 1.0);
    facing_corners(center, eye, WORLD_UP.lerp(head_up, steep), width, height)
}

/// The wrist readout's pose on the inside of `hand`'s wrist: `offset` in the grip frame with +X
/// out of the palm of either hand (OpenXR's grip +X points out of the left palm and into the right
/// one, so X is mirrored for the right hand), facing out of the palm, its text running across the
/// forearm and its top toward the fingers, as read with the palm turned up.
pub(crate) fn inner_wrist_pose(grip: Pose, hand: Hand, offset: Vec3) -> PanelPose {
    PanelPose {
        center: grip.position + grip.orientation * hand.mirror_from_left(offset),
        rotation: (grip.orientation * inner_face(hand)).normalize(),
    }
}

/// The corners of quads stacked top to bottom on `panel`, each as wide and as tall as its entry in
/// `sizes`, centred across the panel, `gap` apart, the stack centred on the panel.
pub(crate) fn stacked_corners(panel: &PanelPose, sizes: &[Vec2], gap: f32) -> Vec<[Vec3; 4]> {
    let total =
        sizes.iter().map(|size| size.y).sum::<f32>() + gap * sizes.len().saturating_sub(1) as f32;
    let up = panel.rotation * Vec3::Y;
    let mut top = total * 0.5;
    sizes
        .iter()
        .map(|&size| {
            let center = panel.center + up * (top - size.y * 0.5);
            top -= size.y + gap;
            PanelPose {
                center,
                rotation: panel.rotation,
            }
            .corners(size.x, size.y)
        })
        .collect()
}

/// What makes the wrist readout show on its own, without a glance.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Attention {
    /// The health fraction, when known.
    pub health: Option<f32>,
    /// Seconds since health last dropped, when it has.
    pub since_damage_s: Option<f32>,
    /// The heat level (zero without heat).
    pub heat_level: i32,
}

/// Whether the player's state wants the wrist readout shown: low health, recent damage, or heat.
pub(crate) fn wants_attention(state: &Attention, low_health: f32, after_damage_s: f32) -> bool {
    let low = state.health.is_some_and(|h| h <= low_health);
    let hurt = state.since_damage_s.is_some_and(|s| s <= after_damage_s);
    low || hurt || state.heat_level > 0
}

/// `point` moved `pull` metres toward `eye`, never past it.
pub(crate) fn pull_toward(point: Vec3, eye: Vec3, pull: f32) -> Vec3 {
    let to_eye = eye - point;
    let distance = to_eye.length();
    if distance <= f32::EPSILON {
        return point;
    }
    point + to_eye * (pull.max(0.0).min(distance) / distance)
}

/// The opacity of a readout `closed_s` seconds into a `linger_s` fade-out.
pub(crate) fn fade_out(closed_s: f32, linger_s: f32) -> f32 {
    if linger_s <= 0.0 {
        return 0.0;
    }
    (1.0 - closed_s / linger_s).clamp(0.0, 1.0)
}

/// The world's up.
const WORLD_UP: Vec3 = Vec3::Y;

/// The range of the vertical component of the direction to the eye over which an upright quad's up
/// blends from the world's up to the head's: from about 18 degrees off vertical to about 6.
const UPRIGHT_BLEND: (f32, f32) = (0.95, 0.995);

/// The inner-wrist readout's rotation in the grip frame: +X along its text, +Y up its page, +Z out
/// of its face. With the palm turned up toward the face, out of the palm is grip +X for the left
/// hand and -X for the right; the text runs left to right across the forearm and the page's top
/// points toward the fingers (grip -Z).
fn inner_face(hand: Hand) -> Quat {
    let (right, normal) = match hand {
        Hand::Left => (Vec3::NEG_Y, Vec3::X),
        Hand::Right => (Vec3::Y, Vec3::NEG_X),
    };
    Quat::from_mat3(&Mat3::from_cols(right, Vec3::NEG_Z, normal))
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    #[test]
    fn a_gun_raised_along_the_gaze_is_sighted() {
        let eye = Vec3::new(0.0, 1.7, 0.0);
        let gaze = Vec3::NEG_Z;
        let raised = eye + Vec3::new(0.05, -0.08, -0.25);
        assert!(sighting(raised, Vec3::NEG_Z, eye, gaze));
        // Held at the hip, or pointed off to the side, it is not.
        let hip = eye + Vec3::new(0.2, -0.7, -0.3);
        assert!(!sighting(hip, Vec3::NEG_Z, eye, gaze));
        let aside = Vec3::new(1.0, 0.0, -1.0);
        assert!(!sighting(raised, aside, eye, gaze));
        assert!(!sighting(raised, Vec3::ZERO, eye, gaze));
    }

    /// The readout rises straight up from the grip until its lower edge clears it, then leans
    /// toward the eye.
    #[test]
    fn the_ammo_anchor_sits_above_the_grip() {
        let grip = Vec3::new(0.2, 1.2, -0.4);
        let eye = Vec3::new(0.0, 1.7, 0.0);
        let placed = ammo_anchor(grip, eye, 0.1, 0.08, 0.0);
        assert!(placed.abs_diff_eq(grip + Vec3::Y * 0.14, 1e-6), "{placed}");
        let pulled = ammo_anchor(grip, eye, 0.1, 0.08, 0.05);
        assert!((pulled.distance(placed) - 0.05).abs() < 1e-5);
        assert!(pulled.distance(eye) < placed.distance(eye));
    }

    /// The tether readout follows the forearm back from the wrist, and rises straight up however
    /// the forearm is turned.
    #[test]
    fn the_forearm_anchor_sits_above_the_forearm() {
        let wrist = Pose {
            position: Vec3::new(-0.2, 1.2, -0.3),
            orientation: Quat::from_rotation_z(FRAC_PI_2) * Quat::from_rotation_y(FRAC_PI_2),
        };
        let placed = forearm_anchor(wrist, 0.12, 0.06, 0.04);
        let expected = wrist.position + wrist.orientation * Vec3::Z * 0.12 + Vec3::Y * 0.08;
        assert!(placed.abs_diff_eq(expected, 1e-6), "{placed} {expected}");
    }

    /// An upright quad keeps its top edge level while the head rolls and faces the eye, and takes
    /// the head's up when seen from straight above.
    #[test]
    fn upright_corners_stay_level() {
        let center = Vec3::new(0.0, 0.0, -0.5);
        let rolled = Quat::from_rotation_z(0.4) * Vec3::Y;
        let [tl, tr, bl, br] = upright_corners(center, Vec3::ZERO, rolled, 0.1, 0.05).unwrap();
        assert!((tl.y - tr.y).abs() < 1e-6, "the top edge is level");
        assert!(tl.y > bl.y, "top is up");
        let normal = (tr - tl).cross(tl - bl).normalize();
        assert!(normal.abs_diff_eq(Vec3::Z, 1e-5), "{normal}");
        assert!(((tl + br) * 0.5).abs_diff_eq(center, 1e-6));
        let below = Vec3::new(0.0, -0.5, 0.0);
        let [tl, tr, bl, _] = upright_corners(below, Vec3::ZERO, Vec3::NEG_Z, 0.1, 0.05).unwrap();
        assert!(tr.x > tl.x, "right is to the right");
        assert!(tl.z < bl.z, "the top points along the head's up");
        assert_eq!(
            upright_corners(Vec3::ZERO, Vec3::ZERO, Vec3::Y, 1.0, 1.0),
            None
        );
    }

    #[test]
    fn the_follow_eases_and_snaps() {
        let target = Vec3::new(1.0, 0.0, 0.0);
        assert_eq!(follow(None, target, 0.01, 0.05, 0.5), target);
        // One halflife covers half the way.
        let eased = follow(Some(Vec3::ZERO), Vec3::new(0.2, 0.0, 0.0), 0.05, 0.05, 0.5);
        assert!((eased.x - 0.1).abs() < 1e-5, "{eased}");
        // A jump past the snap distance snaps.
        assert_eq!(follow(Some(Vec3::ZERO), target, 0.01, 0.05, 0.5), target);
        assert_eq!(
            follow(Some(Vec3::ZERO), Vec3::X * 0.1, 0.01, 0.0, 0.5),
            Vec3::X * 0.1
        );
    }

    /// The quad faces the head and keeps its size.
    #[test]
    fn facing_corners_face_the_head() {
        let center = Vec3::new(0.0, 0.0, -0.5);
        let [tl, tr, bl, br] = facing_corners(center, Vec3::ZERO, Vec3::Y, 0.1, 0.05).unwrap();
        assert!((tr - tl).length() - 0.1 < 1e-5);
        assert!(((tl - bl).length() - 0.05).abs() < 1e-5);
        assert!(tr.x > tl.x, "right is to the right");
        assert!(tl.y > bl.y, "top is up");
        let normal = (tr - tl).cross(tl - bl).normalize();
        assert!(normal.abs_diff_eq(Vec3::Z, 1e-5), "{normal}");
        assert!(((tl + br) * 0.5).abs_diff_eq(center, 1e-6));
        assert_eq!(
            facing_corners(Vec3::ZERO, Vec3::ZERO, Vec3::Y, 1.0, 1.0),
            None
        );
    }

    /// Both hands' readouts face out of the palm, with a proper rotation, and the offset is
    /// mirrored so +X is the palm side of either hand.
    #[test]
    fn the_wrist_readout_faces_out_of_the_palm() {
        let offset = Vec3::new(0.045, 0.0, 0.1);
        let left = inner_wrist_pose(Pose::IDENTITY, Hand::Left, offset);
        let right = inner_wrist_pose(Pose::IDENTITY, Hand::Right, offset);
        assert!(left.normal().abs_diff_eq(Vec3::X, 1e-5));
        assert!(right.normal().abs_diff_eq(Vec3::NEG_X, 1e-5));
        assert!(left.center.abs_diff_eq(offset, 1e-6));
        assert!(right.center.abs_diff_eq(Vec3::new(-0.045, 0.0, 0.1), 1e-6));
        for pose in [left, right] {
            let (x, y) = (pose.rotation * Vec3::X, pose.rotation * Vec3::Y);
            assert!(x.cross(y).abs_diff_eq(pose.normal(), 1e-5));
            // The page's top points toward the fingers.
            assert!(y.abs_diff_eq(Vec3::NEG_Z, 1e-5));
        }
    }

    /// With the left palm turned up in front of the face, the readout faces up and reads left to
    /// right.
    #[test]
    fn the_left_wrist_reads_left_to_right_palm_up() {
        // Palm up: grip +X (out of the left palm) points up, -Z (the fingers) points forward.
        let orientation = Quat::from_mat3(&Mat3::from_cols(Vec3::Y, Vec3::NEG_X, Vec3::Z));
        let grip = Pose {
            position: Vec3::ZERO,
            orientation,
        };
        let pose = inner_wrist_pose(grip, Hand::Left, Vec3::ZERO);
        assert!(
            pose.normal().abs_diff_eq(Vec3::Y, 1e-5),
            "{}",
            pose.normal()
        );
        assert!((pose.rotation * Vec3::X).abs_diff_eq(Vec3::X, 1e-5));
        assert!((pose.rotation * Vec3::Y).abs_diff_eq(Vec3::NEG_Z, 1e-5));
    }

    #[test]
    fn stacked_quads_fill_the_panel_top_down() {
        let panel = PanelPose {
            center: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        };
        let sizes = [
            Vec2::new(0.08, 0.01),
            Vec2::new(0.08, 0.03),
            Vec2::new(0.02, 0.04),
        ];
        let quads = stacked_corners(&panel, &sizes, 0.002);
        assert_eq!(quads.len(), 3);
        let total = 0.084;
        // The first quad's top is the stack's top; the last's bottom is its bottom.
        assert!((quads[0][0].y - total * 0.5).abs() < 1e-6);
        assert!((quads[2][2].y + total * 0.5).abs() < 1e-6);
        // The gap separates them.
        assert!((quads[0][2].y - quads[1][0].y - 0.002).abs() < 1e-6);
        assert!(((quads[0][1] - quads[0][0]).length() - 0.08).abs() < 1e-6);
        // A narrower quad keeps its own width, centred across the panel.
        assert!(((quads[2][1] - quads[2][0]).length() - 0.02).abs() < 1e-6);
        assert!((quads[2][0].x + quads[2][1].x).abs() < 1e-6);
    }

    #[test]
    fn pulling_toward_the_eye_stops_at_it() {
        let point = Vec3::new(0.0, 0.0, -1.0);
        assert!(pull_toward(point, Vec3::ZERO, 0.25).abs_diff_eq(Vec3::new(0.0, 0.0, -0.75), 1e-6));
        assert_eq!(pull_toward(point, Vec3::ZERO, 5.0), Vec3::ZERO);
        assert_eq!(pull_toward(point, Vec3::ZERO, -1.0), point);
        assert_eq!(pull_toward(Vec3::ZERO, Vec3::ZERO, 1.0), Vec3::ZERO);
    }

    #[test]
    fn the_fade_out_runs_over_the_linger() {
        assert_eq!(fade_out(0.0, 0.5), 1.0);
        assert!((fade_out(0.25, 0.5) - 0.5).abs() < 1e-6);
        assert_eq!(fade_out(1.0, 0.5), 0.0);
        assert_eq!(fade_out(0.0, 0.0), 0.0);
    }

    #[test]
    fn attention_follows_health_damage_and_heat() {
        let calm = Attention {
            health: Some(1.0),
            since_damage_s: None,
            heat_level: 0,
        };
        assert!(!wants_attention(&calm, 0.35, 4.0));
        let low = Attention {
            health: Some(0.3),
            ..calm
        };
        assert!(wants_attention(&low, 0.35, 4.0));
        let hurt = Attention {
            since_damage_s: Some(1.0),
            ..calm
        };
        assert!(wants_attention(&hurt, 0.35, 4.0));
        let healed = Attention {
            since_damage_s: Some(5.0),
            ..calm
        };
        assert!(!wants_attention(&healed, 0.35, 4.0));
        let wanted = Attention {
            heat_level: 2,
            ..calm
        };
        assert!(wants_attention(&wanted, 0.35, 4.0));
    }
}
