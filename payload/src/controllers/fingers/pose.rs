//! The finger bones' bend, pure: from a hand's curls to per-joint angles, and from the angles to
//! the model-space joints written over the animated ones.
//!
//! The bend is added to the animated pose rather than replacing it, because the bones' rest frames
//! are not known: each joint turns about an axis built from the animated hand, so no bone's own
//! axes are needed. A finger bends toward the palm about the axis square to the direction from the
//! wrist to its knuckle and to the palm's normal, and turns sideways about the palm's normal. The
//! thumb bends across the palm, toward the little finger's side.

use glam::{Quat, Vec3};

use crate::controllers::{
    fingers::{
        config::FingersConfig,
        curl::{Finger, FingerCurls},
    },
    frame::Hand,
    hands::pose as hand_pose,
};

/// One bone's model-space joint.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BoneJoint {
    pub position: Vec3,
    pub orientation: Quat,
}

/// The joint angles to add to one hand's fingers, radians, indexed by [`Finger::index`] and then
/// from the finger's root joint outward.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandBend {
    pub bend: [[f32; 3]; 5],
    /// The knuckle's sideways turn toward the thumb side.
    pub splay: [f32; 5],
    /// Which fingers are driven; the others keep the animated pose.
    pub driven: [bool; 5],
}

/// The bend for a hand's `curls`, scaled by the hand's tier `weight`. A hand that holds a weapon
/// drives only its trigger finger unless the configuration drives every finger.
pub fn hand_bend(
    curls: &FingerCurls,
    config: &FingersConfig,
    weight: f32,
    holding: bool,
) -> HandBend {
    let weight = weight.clamp(0.0, 1.0);
    let max_splay = config.max_splay_deg.max(0.0).to_radians();
    let mut out = HandBend::default();
    for finger in Finger::ALL {
        let i = finger.index();
        let driven = !holding || config.drive_all_when_holding || finger == Finger::Index;
        if !driven {
            continue;
        }
        out.driven[i] = true;
        let share = config.ranges[i].at(curls.curl[i]) * weight;
        let full = match finger {
            Finger::Thumb => config.thumb_bend_deg,
            _ => config.finger_bend_deg,
        };
        out.bend[i] = full.map(|deg| deg.to_radians() * share);
        out.splay[i] = (curls.splay[i] * config.splay_scale).clamp(-max_splay, max_splay) * weight;
    }
    out
}

/// The hand's frame for bending, from the animated wrist and knuckles (model space).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PalmFrame {
    pub wrist: Vec3,
    /// Out of the palm (the opposite of the back of the hand).
    pub palm: Vec3,
    /// Across the knuckles toward the thumb side.
    pub toward_thumb: Vec3,
}

impl PalmFrame {
    /// The frame from the wrist and the index, middle, and little fingers' knuckles, or `None` for
    /// a degenerate hand.
    pub fn from_bones(
        hand: Hand,
        wrist: Vec3,
        index: Vec3,
        middle: Vec3,
        little: Vec3,
    ) -> Option<Self> {
        let frame = hand_pose::wrist_frame_from_bones(hand, wrist, index, middle)?;
        let back = frame * Vec3::Y;
        let across = index - little;
        let toward_thumb = (across - back * across.dot(back)).try_normalize()?;
        Some(PalmFrame {
            wrist,
            palm: -back,
            toward_thumb,
        })
    }
}

/// A finger's three joints (root outward) bent by `angles` and turned sideways by `splay`, all
/// about the joints' own positions, each joint carrying the ones beyond it.
pub fn bend_finger(
    chain: [BoneJoint; 3],
    palm: &PalmFrame,
    angles: [f32; 3],
    splay: f32,
) -> [BoneJoint; 3] {
    let mut chain = chain;
    let Some(direction) = (chain[0].position - palm.wrist).try_normalize() else {
        return chain;
    };
    let mut flex_axis = direction.cross(palm.palm).try_normalize();
    if splay != 0.0
        && let Some(axis) = direction.cross(palm.toward_thumb).try_normalize()
    {
        let turn = Quat::from_axis_angle(axis, splay);
        rotate_from(&mut chain, 0, turn);
        flex_axis = flex_axis.map(|a| turn * a);
    }
    if let Some(axis) = flex_axis {
        for (k, &angle) in angles.iter().enumerate() {
            rotate_from(&mut chain, k, Quat::from_axis_angle(axis, angle));
        }
    }
    chain
}

/// The thumb's three joints (root outward) bent by `angles` across the palm.
pub fn bend_thumb(chain: [BoneJoint; 3], palm: &PalmFrame, angles: [f32; 3]) -> [BoneJoint; 3] {
    let mut chain = chain;
    let toward = (palm.palm - palm.toward_thumb).normalize_or_zero();
    let Some(axis) = (chain[1].position - chain[0].position)
        .cross(toward)
        .try_normalize()
    else {
        return chain;
    };
    for (k, &angle) in angles.iter().enumerate() {
        rotate_from(&mut chain, k, Quat::from_axis_angle(axis, angle));
    }
    chain
}

/// Rotate joint `from` and every joint beyond it by `rotation` about joint `from`'s position.
fn rotate_from(chain: &mut [BoneJoint; 3], from: usize, rotation: Quat) {
    let pivot = chain[from].position;
    for joint in &mut chain[from..] {
        joint.position = pivot + rotation * (joint.position - pivot);
        joint.orientation = (rotation * joint.orientation).normalize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::fingers::config::CurlRange;

    /// A right hand in the wrist frame's convention: fingers along -Z, the back of the hand up
    /// (+Y), and the little finger at +X.
    fn right_palm() -> PalmFrame {
        PalmFrame::from_bones(
            Hand::Right,
            Vec3::ZERO,
            Vec3::new(-0.03, 0.0, -0.09),
            Vec3::new(0.0, 0.0, -0.09),
            Vec3::new(0.05, 0.0, -0.09),
        )
        .unwrap()
    }

    fn straight_finger(x: f32) -> [BoneJoint; 3] {
        [0.09, 0.135, 0.165].map(|z| BoneJoint {
            position: Vec3::new(x, 0.0, -z),
            orientation: Quat::IDENTITY,
        })
    }

    #[test]
    fn the_palm_frame_faces_down_with_the_thumb_to_the_left() {
        let palm = right_palm();
        assert!(palm.palm.abs_diff_eq(Vec3::NEG_Y, 1e-5), "{palm:?}");
        assert!(palm.toward_thumb.abs_diff_eq(Vec3::NEG_X, 1e-5), "{palm:?}");
    }

    #[test]
    fn a_bend_curls_the_finger_toward_the_palm_about_each_joint() {
        let palm = right_palm();
        let chain = straight_finger(0.0);
        let bent = bend_finger(chain, &palm, [0.5, 0.5, 0.5], 0.0);
        assert_eq!(bent[0].position, chain[0].position, "the knuckle stays");
        assert!(bent[1].position.y < 0.0 && bent[2].position.y < bent[1].position.y);
        for k in 0..2 {
            let before = chain[k + 1].position.distance(chain[k].position);
            let after = bent[k + 1].position.distance(bent[k].position);
            assert!((before - after).abs() < 1e-5, "bone lengths are kept");
        }
        let (_, angle) = bent[2].orientation.to_axis_angle();
        assert!(
            (angle - 1.5).abs() < 1e-4,
            "the last joint carries every bend"
        );
    }

    #[test]
    fn a_splay_turns_the_finger_toward_the_thumb() {
        let palm = right_palm();
        let bent = bend_finger(straight_finger(0.0), &palm, [0.0; 3], 0.2);
        assert!(bent[2].position.x < -0.01);
        assert!(
            bent[2].position.y.abs() < 1e-5,
            "a splay stays in the palm's plane"
        );
    }

    #[test]
    fn the_thumb_bends_across_and_into_the_palm() {
        let palm = right_palm();
        let chain = [
            BoneJoint {
                position: Vec3::new(-0.02, -0.01, -0.02),
                orientation: Quat::IDENTITY,
            },
            BoneJoint {
                position: Vec3::new(-0.05, -0.01, -0.05),
                orientation: Quat::IDENTITY,
            },
            BoneJoint {
                position: Vec3::new(-0.07, -0.01, -0.08),
                orientation: Quat::IDENTITY,
            },
        ];
        let bent = bend_thumb(chain, &palm, [0.2, 0.4, 0.4]);
        assert!(
            bent[2].position.x > chain[2].position.x,
            "toward the little finger"
        );
        assert!(bent[2].position.y < chain[2].position.y, "into the palm");
    }

    #[test]
    fn a_holding_hand_drives_only_the_trigger_finger() {
        let mut config = FingersConfig::new();
        config.ranges = [CurlRange {
            open: 0.0,
            closed: 1.0,
        }; 5];
        let curls = FingerCurls {
            curl: [1.0; 5],
            splay: [0.0; 5],
        };
        let held = hand_bend(&curls, &config, 1.0, true);
        assert_eq!(held.driven, [false, true, false, false, false]);
        assert_eq!(held.bend[Finger::Middle.index()], [0.0; 3]);
        assert!(
            (held.bend[Finger::Index.index()][0] - config.finger_bend_deg[0].to_radians()).abs()
                < 1e-6
        );
        config.drive_all_when_holding = true;
        assert_eq!(hand_bend(&curls, &config, 1.0, true).driven, [true; 5]);
        let half = hand_bend(&curls, &config, 0.5, false);
        assert!(
            (half.bend[Finger::Thumb.index()][2] - config.thumb_bend_deg[2].to_radians() * 0.5)
                .abs()
                < 1e-6
        );
    }

    #[test]
    fn the_splay_is_clamped() {
        let config = FingersConfig::new();
        let curls = FingerCurls {
            curl: [0.0; 5],
            splay: [1.0; 5],
        };
        let bend = hand_bend(&curls, &config, 1.0, false);
        assert!(
            (bend.splay[Finger::Ring.index()] - config.max_splay_deg.to_radians()).abs() < 1e-6
        );
    }
}
