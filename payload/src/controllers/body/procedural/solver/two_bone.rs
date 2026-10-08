//! Analytic two-bone IK: the rotations that bring a chain of two segments (thigh and shin) to a
//! target, bending toward a pole. Pure.

use glam::{Quat, Vec3};

/// The two rotations a solve produces, each to be applied about its joint: `upper` about the root
/// (to the upper bone and everything under it), then `lower` about the middle joint once `upper`
/// has moved it (to the lower bone and everything under it).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TwoBoneRotations {
    pub upper: Quat,
    pub lower: Quat,
}

/// Solve the chain `root` → `middle` → `end` toward `target`. The middle joint bends toward
/// `pole`, so a knee points where the pole says whatever the chain did before; the plane the chain
/// already bends in breaks the tie only when the pole lies along the line to the target, and even
/// then the bend never points away from the pole. A target out of reach is approached along its
/// direction, as far as the chain reaches (a little short of straight, so the bend direction is
/// never lost).
pub fn solve(root: Vec3, middle: Vec3, end: Vec3, target: Vec3, pole: Vec3) -> TwoBoneRotations {
    let identity = TwoBoneRotations {
        upper: Quat::IDENTITY,
        lower: Quat::IDENTITY,
    };
    let upper_len = (middle - root).length();
    let lower_len = (end - middle).length();
    let to_target = target - root;
    let distance = to_target.length();
    if !(upper_len > EPSILON && lower_len > EPSILON && distance > EPSILON && to_target.is_finite())
    {
        return identity;
    }
    let direction = to_target / distance;
    let reach = distance.clamp(
        (upper_len - lower_len).abs() + EPSILON,
        (upper_len + lower_len) * MAX_EXTENSION,
    );

    let bend = bend_direction(root, middle, end, direction, pole);
    if bend == Vec3::ZERO {
        return identity;
    }

    let cos_upper = ((upper_len * upper_len + reach * reach - lower_len * lower_len)
        / (2.0 * upper_len * reach))
        .clamp(-1.0, 1.0);
    let sin_upper = (1.0 - cos_upper * cos_upper).max(0.0).sqrt();
    let new_middle = root + (direction * cos_upper + bend * sin_upper) * upper_len;
    let new_end = root + direction * reach;

    let upper =
        Quat::from_rotation_arc((middle - root).normalize(), (new_middle - root).normalize());
    let moved_end = root + upper * (end - root);
    let lower = Quat::from_rotation_arc(
        (moved_end - new_middle).normalize(),
        (new_end - new_middle).normalize(),
    );
    TwoBoneRotations { upper, lower }
}

/// The unit direction the middle joint bends toward, square to `direction`: the pole's, or the
/// chain's own bend when the pole lies along `direction`, turned toward the pole's side; zero
/// when neither gives one.
fn bend_direction(root: Vec3, middle: Vec3, end: Vec3, direction: Vec3, pole: Vec3) -> Vec3 {
    let square = |v: Vec3| v - direction * v.dot(direction);
    let from_pole = square(pole);
    if from_pole.length_squared() > MIN_POLE * MIN_POLE * pole.length_squared().max(EPSILON) {
        return from_pole.normalize_or_zero();
    }
    let along = (end - root).normalize_or_zero();
    let own = square((middle - root) - along * (middle - root).dot(along));
    let own = if own.dot(pole) < 0.0 { -own } else { own };
    own.normalize_or_zero()
}

const EPSILON: f32 = 1e-5;

/// The least share of the pole square to the target's direction that still sets the bend.
const MIN_POLE: f32 = 0.05;

/// The furthest the chain straightens, as a share of its length.
const MAX_EXTENSION: f32 = 0.999;

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(root: Vec3, middle: Vec3, end: Vec3, r: TwoBoneRotations) -> (Vec3, Vec3) {
        let middle2 = root + r.upper * (middle - root);
        let end1 = root + r.upper * (end - root);
        let end2 = middle2 + r.lower * (end1 - middle2);
        (middle2, end2)
    }

    #[test]
    fn a_reachable_target_is_reached_and_lengths_kept() {
        let (root, middle, end) = (
            Vec3::new(0.0, 0.9, 0.0),
            Vec3::new(0.0, 0.5, -0.02),
            Vec3::new(0.0, 0.08, 0.0),
        );
        let target = Vec3::new(0.05, 0.2, 0.1);
        let r = solve(root, middle, end, target, Vec3::NEG_Z);
        let (m, e) = apply(root, middle, end, r);
        assert!(e.abs_diff_eq(target, 1e-4), "{e:?}");
        assert!(((m - root).length() - (middle - root).length()).abs() < 1e-4);
        assert!(((e - m).length() - (end - middle).length()).abs() < 1e-4);
    }

    #[test]
    fn the_knee_keeps_its_side() {
        let (root, middle, end) = (
            Vec3::new(0.0, 0.9, 0.0),
            Vec3::new(0.0, 0.5, -0.05),
            Vec3::new(0.0, 0.08, 0.0),
        );
        // Crouch: the foot stays, the hip drops.
        let r = solve(root, middle, end, Vec3::new(0.0, 0.3, 0.0), Vec3::NEG_Z);
        let (m, _) = apply(root, middle, end, r);
        assert!(m.z < -0.05, "the knee bent backwards: {m:?}");
    }

    #[test]
    fn an_unreachable_target_straightens_toward_it() {
        let (root, middle, end) = (
            Vec3::new(0.0, 0.9, 0.0),
            Vec3::new(0.0, 0.5, -0.05),
            Vec3::ZERO,
        );
        let target = Vec3::new(0.0, -2.0, 0.0);
        let r = solve(root, middle, end, target, Vec3::NEG_Z);
        let (_, e) = apply(root, middle, end, r);
        assert!((e - root).normalize().abs_diff_eq(Vec3::NEG_Y, 1e-3));
    }

    #[test]
    fn the_pole_sets_the_bend_whatever_the_chain_did() {
        // The chain bends backward, as a clip's leg can mid-stride.
        let (root, middle, end) = (
            Vec3::new(0.0, 0.9, 0.0),
            Vec3::new(0.0, 0.5, 0.05),
            Vec3::new(0.0, 0.08, 0.0),
        );
        let r = solve(root, middle, end, Vec3::new(0.0, 0.3, 0.0), Vec3::NEG_Z);
        let (m, _) = apply(root, middle, end, r);
        assert!(m.z < -0.05, "the knee kept the backward bend: {m:?}");
        // A pole turned out to the side turns the knee with it.
        let pole = Vec3::new(1.0, 0.0, -1.0);
        let r = solve(root, middle, end, Vec3::new(0.0, 0.3, 0.0), pole);
        let (m, _) = apply(root, middle, end, r);
        assert!(m.x > 0.05 && m.z < -0.05, "{m:?}");
    }

    #[test]
    fn a_degenerate_chain_is_left_alone() {
        let r = solve(Vec3::ZERO, Vec3::ZERO, Vec3::Y, Vec3::X, Vec3::Z);
        assert_eq!(r.upper, Quat::IDENTITY);
        assert_eq!(r.lower, Quat::IDENTITY);
    }
}
