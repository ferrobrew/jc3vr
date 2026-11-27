//! The two-bone solve: where the middle joint of a limb of two rigid bones (an elbow, a knee) goes
//! for the limb's root to reach its end, bent toward a pole.

use glam::Vec3;

/// The middle joint of a limb whose upper bone is `upper` long and lower bone `lower` long,
/// reaching from `root` toward `end`, bent toward `pole` (a direction from the root). An end out of
/// reach straightens the limb toward it, and one nearer than the limb folds to is held at that.
/// `None` for a bone not longer than a tenth of a millimetre, an end on the root, or a pole along
/// the reach or not finite.
pub fn middle_joint(root: Vec3, end: Vec3, upper: f32, lower: f32, pole: Vec3) -> Option<Vec3> {
    let reach = end - root;
    let axis = reach.try_normalize()?;
    if !(upper > MIN_BONE_M && lower > MIN_BONE_M && pole.is_finite()) {
        return None;
    }
    let side = pole - axis * pole.dot(axis);
    if side.length() < MIN_POLE_M {
        return None;
    }
    let side = side.normalize();
    let shortest = (upper - lower).abs() + MIN_BONE_M;
    let longest = upper + lower - MIN_BONE_M;
    let distance = reach.length().clamp(shortest.min(longest), longest);
    let cos = interior_cos(upper, distance, lower);
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    Some(root + axis * (upper * cos) + side * (upper * sin))
}

/// The cosine of a triangle's angle between its sides `a` and `b`, with the side `opposite` across
/// from it (the law of cosines), held to `-1..=1` for sides that make no triangle.
pub fn interior_cos(a: f32, b: f32, opposite: f32) -> f32 {
    ((a * a + b * b - opposite * opposite) / (2.0 * a * b)).clamp(-1.0, 1.0)
}

/// The shortest bone the solve takes, and the margin it keeps from a fully straight or fully
/// folded limb, metres.
const MIN_BONE_M: f32 = 1e-4;

/// The shortest part of the pole across the reach that still says which way to bend, metres.
const MIN_POLE_M: f32 = 1e-3;

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn the_joint_bends_toward_the_pole_and_straightens_out_of_reach() {
        let joint =
            middle_joint(Vec3::ZERO, Vec3::new(0.0, 0.0, -0.4), 0.3, 0.3, Vec3::NEG_Y).unwrap();
        assert!(joint.y < 0.0 && joint.x.abs() < 1e-6);
        let far = middle_joint(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -2.0),
            0.3,
            0.25,
            Vec3::NEG_Y,
        )
        .unwrap();
        assert!(far.abs_diff_eq(Vec3::new(0.0, 0.0, -0.3), 1e-2));
        assert_eq!(
            middle_joint(Vec3::ZERO, Vec3::NEG_Z, 0.3, 0.3, Vec3::NEG_Z),
            None
        );
        assert_eq!(
            middle_joint(Vec3::ZERO, Vec3::ZERO, 0.3, 0.3, Vec3::NEG_Y),
            None
        );
        assert_eq!(
            middle_joint(Vec3::ZERO, Vec3::NEG_Z, 0.0, 0.3, Vec3::NEG_Y),
            None
        );
    }

    proptest! {
        #[test]
        fn an_end_out_of_reach_straightens_the_limb_toward_it(
            direction in proptest::array::uniform3(-1.0f32..1.0),
            pole in proptest::array::uniform3(-1.0f32..1.0),
            upper in 0.2f32..0.4,
            lower in 0.2f32..0.4,
            beyond in 0.05f32..1.0,
        ) {
            let direction = Vec3::from(direction).normalize_or_zero();
            prop_assume!(direction != Vec3::ZERO);
            let end = direction * (upper + lower + beyond);
            if let Some(joint) = middle_joint(Vec3::ZERO, end, upper, lower, Vec3::from(pole)) {
                prop_assert!((joint.length() - upper).abs() < 1e-3);
                prop_assert!(joint.normalize().dot(direction) > 0.99);
            }
        }

        #[test]
        fn the_bones_keep_their_lengths_within_reach(
            end in proptest::array::uniform3(-0.5f32..0.5),
            pole in proptest::array::uniform3(-1.0f32..1.0),
            upper in 0.2f32..0.4,
            lower in 0.2f32..0.4,
        ) {
            let (end, pole) = (Vec3::from(end), Vec3::from(pole));
            let distance = end.length();
            prop_assume!(distance > (upper - lower).abs() + 0.01 && distance < upper + lower - 0.01);
            if let Some(joint) = middle_joint(Vec3::ZERO, end, upper, lower, pole) {
                prop_assert!((joint.length() - upper).abs() < 1e-3);
                prop_assert!((joint.distance(end) - lower).abs() < 1e-3);
                prop_assert!((joint - end * joint.dot(end) / (distance * distance)).dot(pole) >= -1e-4);
            }
        }
    }
}
