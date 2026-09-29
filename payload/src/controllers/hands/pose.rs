//! Hand pose composition, all pure: from a controller's grip pose to the character's wrist frame,
//! through the body-scale remap and the head's world transform.
//!
//! The chain, per hand:
//!
//! 1. The grip pose, in the cockpit frame ([`crate::controllers::frame`]).
//! 2. Times the grip offset ([`grip_to_wrist`]): the **wrist frame**, the hand-tracking wrist
//!    convention (origin at the wrist joint, -Z toward the fingers, +Y out of the back of the
//!    hand, X = Y × Z).
//! 3. The Bonelab-style remap ([`remap_wrist`]): the wrist's offset from the player's shoulder,
//!    scaled by the ratio of the arm spans, placed at the character's shoulder.
//! 4. Into the world through the head's transform ([`compose_world`]), the same composition the
//!    head pose uses, so that the hands and the view agree.
//!
//! On the character's side, the same wrist frame is built from the animated bones
//! ([`wrist_frame_from_bones`]): the direction to the middle knuckle and the palm's plane. The IK
//! rotation target is the delta between the two ([`wrist_rotation_delta`]), which never needs to
//! know how the game's hand bone is oriented at rest.

use glam::{EulerRot, Mat3, Quat, Vec3};

use crate::controllers::{
    body::{self, BodyDimensions},
    config::GripOffset,
    frame::{Hand, Pose},
};

/// One hand's poses in the cockpit frame, after the grip offset and the remap.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CockpitHand {
    /// The character's wrist frame (see the module docs).
    pub wrist: Pose,
    /// The controller's grip pose, carried by the remap's shift.
    pub grip: Pose,
    /// The controller's aim pose, carried by the remap's shift.
    pub aim: Pose,
}

/// A hand's cockpit-frame poses: the wrist frame from the grip pose and the grip offset, and, with
/// `remap` (the player's and the character's dimensions), all three poses shifted by the remap of
/// the wrist, so the controller's rays leave the remapped hand.
pub fn cockpit_hand(
    hand: Hand,
    grip: Pose,
    aim: Pose,
    head: Pose,
    offset: &GripOffset,
    remap: Option<(&BodyDimensions, &BodyDimensions)>,
) -> CockpitHand {
    let wrist = mul(grip, grip_to_wrist(hand, offset));
    let shift = remap.map_or(Vec3::ZERO, |(player, character)| {
        remap_wrist(wrist.position, hand, head, player, character) - wrist.position
    });
    let shifted = |p: Pose| Pose {
        position: p.position + shift,
        orientation: p.orientation,
    };
    CockpitHand {
        wrist: shifted(wrist),
        grip: shifted(grip),
        aim: shifted(aim),
    }
}

/// The wrist frame in the grip frame of `hand`: the offset `offset` describes for the right hand,
/// mirrored across the body's sagittal plane for the left.
///
/// The OpenXR grip frames of the two hands are mirror images with one axis flipped to stay
/// right-handed (the left grip's +Y points the other way), and so are the two wrist frames (the
/// left wrist's +X points the other way). Mirroring a right-hand transform `(R, t)` therefore gives
/// `(F_g R F_w, F_g t)` for the left, with `F_g` negating the grip's Y and `F_w` negating the
/// wrist's X.
pub fn grip_to_wrist(hand: Hand, offset: &GripOffset) -> Pose {
    let adjust = offset.rotation_deg;
    let rotation = Mat3::from_quat(Quat::from_euler(
        EulerRot::ZYX,
        adjust.z.to_radians(),
        adjust.y.to_radians(),
        adjust.x.to_radians(),
    )) * right_wrist_in_grip();
    let (rotation, position) = match hand {
        Hand::Right => (rotation, offset.position_m),
        Hand::Left => {
            let flip_grip = Mat3::from_diagonal(Vec3::new(1.0, -1.0, 1.0));
            let flip_wrist = Mat3::from_diagonal(Vec3::new(-1.0, 1.0, 1.0));
            (
                flip_grip * rotation * flip_wrist,
                flip_grip * offset.position_m,
            )
        }
    };
    Pose {
        position,
        orientation: Quat::from_mat3(&rotation).normalize(),
    }
}

/// Remap a wrist position in the cockpit frame from the player's body onto the character's: the
/// offset from the player's shoulder on that side, scaled by the arm-span ratio, placed at the
/// character's shoulder. The shoulders are estimated from the head (the eye midpoint) and its yaw,
/// for both bodies at the same eye point, because the view puts the character's eyes where the
/// player's are.
pub fn remap_wrist(
    wrist: Vec3,
    hand: Hand,
    head: Pose,
    player: &BodyDimensions,
    character: &BodyDimensions,
) -> Vec3 {
    let torso = yaw_only(head.orientation);
    let player_shoulder = body::shoulder_positions(head.position, torso, player)[hand.index()];
    let character_shoulder =
        body::shoulder_positions(head.position, torso, character)[hand.index()];
    body::remap_hand_position(
        wrist,
        player_shoulder,
        character_shoulder,
        player,
        character,
    )
}

/// A cockpit-frame pose in the world, composed relative to the head: `head_world` is the head's
/// world position, `body` the body-frame rotation the head composes with, `head_cockpit` the head's
/// cockpit position, and `world_scale` the VR world scale. The same composition as the head pose
/// (`headpose::xr::compose`), anchored at the head so that the hands and the view agree.
pub fn compose_world(
    head_world: Vec3,
    body: Quat,
    head_cockpit: Vec3,
    pose: Pose,
    world_scale: f32,
) -> Pose {
    Pose {
        position: head_world + body * ((pose.position - head_cockpit) * world_scale),
        orientation: (body * pose.orientation).normalize(),
    }
}

/// The body-frame rotation behind a composed head pose: the head's world orientation with its
/// cockpit orientation divided out.
pub fn body_of_head(head_world: Quat, head_cockpit: Quat) -> Quat {
    (head_world * head_cockpit.inverse()).normalize()
}

/// The character's wrist frame orientation, in the space the bone positions are given in, from
/// the wrist joint and the index and middle knuckles (the proximal phalanges' origins, which are
/// fixed relative to the hand bone). `None` for a degenerate hand (coincident points).
///
/// -Z runs from the wrist to the middle knuckle. +Y is the back of the hand: the normal of the
/// plane through the wrist and the two knuckles, whose sign depends on the side because the index
/// finger is on the thumb side of each hand.
pub fn wrist_frame_from_bones(hand: Hand, wrist: Vec3, index: Vec3, middle: Vec3) -> Option<Quat> {
    let to_middle = middle - wrist;
    let to_index = index - wrist;
    let back = match hand {
        Hand::Right => to_middle.cross(to_index),
        Hand::Left => to_index.cross(to_middle),
    };
    let z = (-to_middle).try_normalize()?;
    let y = (back - z * back.dot(z)).try_normalize()?;
    let x = y.cross(z);
    Some(Quat::from_mat3(&Mat3::from_cols(x, y, z)).normalize())
}

/// The model-space rotation that carries the animated wrist frame `animated` onto `target`, as the
/// axis and angle HumanIK's rotation target takes: the engine pre-multiplies it onto the effector's
/// pre-solve orientation, so any fixed offset between the wrist frame and the bone's own frame
/// cancels.
pub fn wrist_rotation_delta(target: Quat, animated: Quat) -> (Vec3, f32) {
    let delta = (target * animated.inverse()).normalize();
    // The shorter way round, so that the solver is never asked to spin the wrist past 180 degrees.
    let delta = if delta.w < 0.0 { -delta } else { delta };
    delta.to_axis_angle()
}

/// `a` then `b`, as rigid transforms: `b` expressed in `a`'s frame.
pub fn mul(a: Pose, b: Pose) -> Pose {
    Pose {
        position: a.position + a.orientation * b.position,
        orientation: (a.orientation * b.orientation).normalize(),
    }
}

/// The inverse rigid transform.
pub fn inverse(p: Pose) -> Pose {
    let orientation = p.orientation.inverse();
    Pose {
        position: orientation * -p.position,
        orientation,
    }
}

/// Interpolate between two poses: a linear position and a spherical orientation.
pub fn interp(a: Pose, b: Pose, t: f32) -> Pose {
    Pose {
        position: a.position.lerp(b.position, t),
        orientation: a.orientation.slerp(b.orientation, t).normalize(),
    }
}

/// The world-space correction that carries `reference` onto `now`: `now × reference⁻¹`.
pub fn delta(now: Pose, reference: Pose) -> Pose {
    mul(now, inverse(reference))
}

/// The correction for a weighted move from `reference` toward `now`.
///
/// Interpolating the poses before deriving the correction keeps a rotation about a translated
/// wrist physically anchored at that wrist. Interpolating the world-space correction from identity
/// would instead move the correction's pivot toward the world origin.
pub fn weighted_delta(now: Pose, reference: Pose, weight: f32) -> Pose {
    let weighted_now = interp(reference, now, weight.clamp(0.0, 1.0));
    delta(weighted_now, reference)
}

/// `rotation` with its pitch and roll removed, keeping the heading of its -Z axis.
fn yaw_only(rotation: Quat) -> Quat {
    let forward = rotation * Vec3::NEG_Z;
    match Vec3::new(forward.x, 0.0, forward.z).try_normalize() {
        Some(flat) => Quat::from_rotation_arc(Vec3::NEG_Z, flat),
        None => Quat::IDENTITY,
    }
}

/// The right wrist frame in the right grip frame, from the two OpenXR definitions for a hand
/// closed on a vertical handle with the thumb on top: the grip's -Z runs up the tube of the fingers
/// and its +X into the palm, while the wrist frame's fingers point forward, square to the tube, with
/// the back of the hand outward and the little finger (the wrist's +X) down.
fn right_wrist_in_grip() -> Mat3 {
    Mat3::from_cols(
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < EPS
    }

    fn same_rotation(a: Quat, b: Quat) -> bool {
        a.dot(b).abs() > 1.0 - EPS
    }

    const NO_OFFSET: GripOffset = GripOffset {
        position_m: Vec3::ZERO,
        rotation_deg: Vec3::ZERO,
    };

    /// A right hand closed on a vertical handle, thumb up, arm forward, in a world with +X right,
    /// +Y up, -Z forward: the grip's -Z is up and its +X into the palm (leftward).
    fn right_grip_on_vertical_handle() -> Quat {
        let x = Vec3::NEG_X;
        let z = Vec3::NEG_Y;
        Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z))
    }

    /// The same for the left hand: -Z up, +X away from the palm (the palm faces right).
    fn left_grip_on_vertical_handle() -> Quat {
        let x = Vec3::X;
        let z = Vec3::NEG_Y;
        Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z))
    }

    #[test]
    fn the_base_offset_points_the_fingers_forward_with_the_back_of_the_hand_out() {
        for (hand, grip, outward) in [
            (Hand::Right, right_grip_on_vertical_handle(), Vec3::X),
            (Hand::Left, left_grip_on_vertical_handle(), Vec3::NEG_X),
        ] {
            let wrist = grip * grip_to_wrist(hand, &NO_OFFSET).orientation;
            assert!(close(wrist * Vec3::NEG_Z, Vec3::NEG_Z), "{hand:?} fingers");
            assert!(close(wrist * Vec3::Y, outward), "{hand:?} back of hand");
        }
    }

    #[test]
    fn the_left_offset_mirrors_the_right() {
        let offset = GripOffset {
            position_m: Vec3::new(-0.02, -0.07, 0.01),
            rotation_deg: Vec3::new(-15.0, 5.0, 3.0),
        };
        let right = right_grip_on_vertical_handle();
        let left = left_grip_on_vertical_handle();
        let r = mul(
            Pose {
                position: Vec3::ZERO,
                orientation: right,
            },
            grip_to_wrist(Hand::Right, &offset),
        );
        let l = mul(
            Pose {
                position: Vec3::ZERO,
                orientation: left,
            },
            grip_to_wrist(Hand::Left, &offset),
        );
        let mirror = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
        assert!(close(l.position, mirror(r.position)));
        // Mirrored hands: the fingers and the back of the hand mirror; the wrist's X flips too.
        assert!(close(
            l.orientation * Vec3::NEG_Z,
            mirror(r.orientation * Vec3::NEG_Z)
        ));
        assert!(close(
            l.orientation * Vec3::Y,
            mirror(r.orientation * Vec3::Y)
        ));
        assert!(close(
            l.orientation * Vec3::X,
            -mirror(r.orientation * Vec3::X)
        ));
    }

    #[test]
    fn the_grip_offsets_are_rotations() {
        for hand in Hand::BOTH {
            let q = grip_to_wrist(hand, &GripOffset::INDEX_BEST_GUESS).orientation;
            assert!((q.length() - 1.0).abs() < EPS);
            assert!((Mat3::from_quat(q).determinant() - 1.0).abs() < EPS);
        }
    }

    #[test]
    fn a_negative_pitch_tilts_the_handle_toward_the_fingers() {
        let tilted = GripOffset {
            position_m: Vec3::ZERO,
            rotation_deg: Vec3::new(-15.0, 0.0, 0.0),
        };
        let fingers = grip_to_wrist(Hand::Right, &tilted).orientation * Vec3::NEG_Z;
        // The grip's -Z is the handle; the fingers now lean toward it.
        assert!(fingers.dot(Vec3::NEG_Z) > 0.2);
    }

    #[test]
    fn the_bone_frame_matches_the_wrist_convention() {
        // A right hand palm down, fingers forward: the index knuckle is on the thumb side (left).
        let wrist = Vec3::new(0.3, 1.0, 0.0);
        let index = wrist + Vec3::new(-0.02, 0.0, -0.09);
        let middle = wrist + Vec3::new(0.0, 0.0, -0.09);
        let q = wrist_frame_from_bones(Hand::Right, wrist, index, middle).unwrap();
        assert!(close(q * Vec3::NEG_Z, Vec3::NEG_Z));
        assert!(close(q * Vec3::Y, Vec3::Y));
        assert!(close(q * Vec3::X, Vec3::X), "the little-finger side is +X");

        // The left hand mirrored: the index knuckle on the right.
        let index = wrist + Vec3::new(0.02, 0.0, -0.09);
        let q = wrist_frame_from_bones(Hand::Left, wrist, index, middle).unwrap();
        assert!(close(q * Vec3::Y, Vec3::Y));
        assert!(close(q * Vec3::NEG_Z, Vec3::NEG_Z));
    }

    #[test]
    fn a_degenerate_hand_has_no_frame() {
        let p = Vec3::ONE;
        assert!(wrist_frame_from_bones(Hand::Right, p, p, p).is_none());
        let middle = p + Vec3::Z;
        assert!(wrist_frame_from_bones(Hand::Right, p, middle, middle).is_none());
    }

    #[test]
    fn the_rotation_delta_carries_the_animated_frame_onto_the_target() {
        let animated = Quat::from_euler(EulerRot::YXZ, 0.4, -0.2, 0.9);
        let target = Quat::from_euler(EulerRot::YXZ, -1.1, 0.5, 0.1);
        let (axis, angle) = wrist_rotation_delta(target, animated);
        assert!(angle <= std::f32::consts::PI + EPS);
        assert!(same_rotation(
            Quat::from_axis_angle(axis, angle) * animated,
            target
        ));
        // A bone frame at a fixed offset from the wrist frame lands where it should too.
        let bone_offset = Quat::from_rotation_x(0.7);
        let bone = Quat::from_axis_angle(axis, angle) * (animated * bone_offset);
        assert!(same_rotation(bone, target * bone_offset));
    }

    #[test]
    fn the_remap_is_the_identity_for_the_same_body() {
        let head = Pose {
            position: Vec3::new(0.1, 1.6, 0.2),
            orientation: Quat::from_rotation_y(0.5),
        };
        let wrist = Vec3::new(0.4, 1.1, -0.3);
        let dims = BodyDimensions::GENERIC_ADULT;
        assert!(close(
            remap_wrist(wrist, Hand::Right, head, &dims, &dims),
            wrist
        ));
    }

    #[test]
    fn the_remap_extends_a_short_players_reach() {
        let player = BodyDimensions {
            arm_span: 1.5,
            ..BodyDimensions::GENERIC_ADULT
        };
        let character = BodyDimensions::CHARACTER_ESTIMATE;
        let head = Pose {
            position: Vec3::new(0.0, player.eye_height, 0.0),
            orientation: Quat::IDENTITY,
        };
        let shoulder = body::shoulder_positions(head.position, Quat::IDENTITY, &player)[1];
        let wrist = shoulder + Vec3::NEG_Z * 0.5;
        let remapped = remap_wrist(wrist, Hand::Right, head, &player, &character);
        let character_shoulder =
            body::shoulder_positions(head.position, Quat::IDENTITY, &character)[1];
        let reach = (remapped - character_shoulder).length();
        assert!((reach - 0.5 * character.arm_span / player.arm_span).abs() < EPS);
    }

    #[test]
    fn composition_matches_the_head_pose() {
        let body = Quat::from_rotation_y(1.2);
        let head_cockpit = Vec3::new(0.1, 0.05, -0.2);
        let head_world = Vec3::new(100.0, 20.0, -50.0);
        let hand = Pose {
            position: Vec3::new(0.3, -0.4, -0.5),
            orientation: Quat::from_rotation_x(0.3),
        };
        let world = compose_world(head_world, body, head_cockpit, hand, 1.0);
        // The same as composing both through an anchor: anchor + body × cockpit.
        let anchor = head_world - body * head_cockpit;
        assert!(close(world.position, anchor + body * hand.position));
        assert!(same_rotation(world.orientation, body * hand.orientation));
        let recovered = body_of_head(
            body * Quat::from_rotation_y(0.2),
            Quat::from_rotation_y(0.2),
        );
        assert!(same_rotation(recovered, body));
    }

    #[test]
    fn a_weighted_delta_carries_the_reference_toward_now() {
        let reference = Pose {
            position: Vec3::new(1.0, 2.0, 3.0),
            orientation: Quat::from_rotation_z(0.4),
        };
        let now = Pose {
            position: Vec3::new(1.1, 2.0, 2.9),
            orientation: Quat::from_rotation_z(0.5) * Quat::from_rotation_x(0.1),
        };
        let d = delta(now, reference);
        let carried = mul(d, reference);
        assert!(close(carried.position, now.position));
        assert!(same_rotation(carried.orientation, now.orientation));

        let none = weighted_delta(now, reference, 0.0);
        assert!(close(none.position, Vec3::ZERO));
        assert!(same_rotation(none.orientation, Quat::IDENTITY));
        let whole = weighted_delta(now, reference, 1.0);
        assert!(close(whole.position, d.position));
        assert!(same_rotation(whole.orientation, d.orientation));
    }

    #[test]
    fn a_weighted_rotation_stays_anchored_at_a_translated_origin() {
        let reference = Pose {
            position: Vec3::new(1000.0, -200.0, 300.0),
            orientation: Quat::IDENTITY,
        };
        let now = Pose {
            orientation: Quat::from_rotation_y(0.4),
            ..reference
        };
        let correction = weighted_delta(now, reference, 0.5);
        let carried = mul(correction, reference);
        assert!(close(carried.position, reference.position));
        assert!(same_rotation(
            carried.orientation,
            Quat::from_rotation_y(0.2)
        ));
    }
}
