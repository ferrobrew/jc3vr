//! The neutral stance the feet stand and walk around. Pure, in Rico's model space.

use glam::{Quat, Vec3};

/// A foot's place under the root when standing, model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StanceFoot {
    pub position: Vec3,
    pub rotation: Quat,
}

/// The neutral stance: where the animation stands the feet when Rico has been still for a moment,
/// measured once and made symmetric, so that the feet walk and stand around the same place
/// whatever the animation's idle does with them afterwards. The feet's turns are kept only as a
/// mirrored toe-out about the root against the reference pose's feet: an idle's own twist of one
/// foot, and any tilt of the ground, are left out.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct NeutralStance {
    still_s: f32,
    stance: Option<[StanceFoot; 2]>,
}

impl NeutralStance {
    pub const fn new() -> Self {
        Self {
            still_s: 0.0,
            stance: None,
        }
    }

    /// The measured stance, left then right.
    pub fn get(&self) -> Option<[StanceFoot; 2]> {
        self.stance
    }

    /// Fold in a tick of the animated feet: measured once they have stood still on level ground
    /// for [`SETTLE_S`]. `reference` is the reference pose's feet's rotations, left then right.
    /// `still` must leave out aiming, whose stance is the weapon's. Returns the stance when this
    /// tick measured it.
    pub fn observe(
        &mut self,
        feet: [StanceFoot; 2],
        reference: [Quat; 2],
        still: bool,
        dt: f32,
    ) -> Option<[StanceFoot; 2]> {
        if self.stance.is_some() {
            return None;
        }
        let level = (feet[0].position.y - feet[1].position.y).abs() <= MAX_LEVEL_DIFFERENCE_M;
        if !still
            || !level
            || !feet
                .iter()
                .all(|f| f.position.is_finite() && f.rotation.is_finite())
        {
            self.still_s = 0.0;
            return None;
        }
        self.still_s += dt.max(0.0);
        if self.still_s < SETTLE_S {
            return None;
        }
        let [left, right] = feet;
        let half_width = ((right.position.x - left.position.x) * 0.5).abs();
        let height = (left.position.y + right.position.y) * 0.5;
        let depth = (left.position.z + right.position.z) * 0.5;
        // The toe-out, outward positive: the left foot's yaw is outward when positive about +Y
        // (toward -X when facing -Z), the right's when negative.
        let toe_out =
            (yaw_from(left.rotation, reference[0]) - yaw_from(right.rotation, reference[1])) * 0.5;
        let toe_out = if toe_out.is_finite() {
            toe_out.clamp(-MAX_TOE_OUT, MAX_TOE_OUT)
        } else {
            0.0
        };
        let stance = [
            StanceFoot {
                position: Vec3::new(-half_width, height, depth),
                rotation: (Quat::from_rotation_y(toe_out) * reference[0]).normalize(),
            },
            StanceFoot {
                position: Vec3::new(half_width, height, depth),
                rotation: (Quat::from_rotation_y(-toe_out) * reference[1]).normalize(),
            },
        ];
        self.stance = Some(stance);
        Some(stance)
    }
}

/// How long the feet stand still before the stance is measured, seconds.
pub const SETTLE_S: f32 = 0.5;

/// The most the feet's heights may differ for the ground under them to count as level, metres.
const MAX_LEVEL_DIFFERENCE_M: f32 = 0.03;

/// The most toe-out the stance keeps, radians: about thirty degrees.
const MAX_TOE_OUT: f32 = 0.52;

/// How far `rotation` is turned about +Y from `reference`, radians: the yaw of the reference
/// forward (-Z) carried by the turn between them.
fn yaw_from(rotation: Quat, reference: Quat) -> f32 {
    let forward = (rotation * reference.inverse()) * Vec3::NEG_Z;
    if forward.x.abs() + forward.z.abs() < 1e-6 {
        return 0.0;
    }
    (-forward.x).atan2(-forward.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stance_is_measured_once_after_settling_and_symmetric() {
        let mut stance = NeutralStance::new();
        let foot = |x: f32, z: f32| StanceFoot {
            position: Vec3::new(x, 0.1, z),
            rotation: Quat::IDENTITY,
        };
        let feet = [foot(-0.15, 0.02), foot(0.11, -0.04)];
        let reference = [Quat::IDENTITY; 2];
        assert!(stance.observe(feet, reference, true, 0.3).is_none());
        assert!(stance.observe(feet, reference, false, 0.3).is_none());
        assert!(stance.observe(feet, reference, true, 0.3).is_none());
        let measured = stance.observe(feet, reference, true, 0.3).unwrap();
        assert!((measured[0].position.x + 0.13).abs() < 1e-5);
        assert!((measured[1].position.x - 0.13).abs() < 1e-5);
        assert!((measured[0].position.z + 0.01).abs() < 1e-5);
        assert!(stance.observe(feet, reference, true, 0.3).is_none());
        assert_eq!(stance.get(), Some(measured));
    }

    #[test]
    fn the_stance_mirrors_the_toe_out_and_drops_the_idles_twist_and_tilt() {
        let mut stance = NeutralStance::new();
        let reference = [Quat::IDENTITY; 2];
        // The left foot turned out by 0.3 and tilted, the right turned in by 0.1.
        let feet = [
            StanceFoot {
                position: Vec3::new(-0.12, 0.1, 0.0),
                rotation: Quat::from_rotation_y(0.3) * Quat::from_rotation_x(0.2),
            },
            StanceFoot {
                position: Vec3::new(0.12, 0.1, 0.0),
                rotation: Quat::from_rotation_y(0.1),
            },
        ];
        stance.observe(feet, reference, true, 0.3);
        let measured = stance.observe(feet, reference, true, 0.3).unwrap();
        let left = measured[0].rotation * Vec3::NEG_Z;
        let right = measured[1].rotation * Vec3::NEG_Z;
        assert!(left.y.abs() < 1e-5 && right.y.abs() < 1e-5);
        assert!((left.x + right.x).abs() < 1e-5, "{left} {right}");
        assert!(left.x < 0.0, "the left foot does not turn out: {left}");
    }

    #[test]
    fn uneven_ground_is_not_measured() {
        let mut stance = NeutralStance::new();
        let foot = |x: f32, y: f32| StanceFoot {
            position: Vec3::new(x, y, 0.0),
            rotation: Quat::IDENTITY,
        };
        let feet = [foot(-0.12, 0.1), foot(0.12, 0.25)];
        for _ in 0..10 {
            assert!(
                stance
                    .observe(feet, [Quat::IDENTITY; 2], true, 0.3)
                    .is_none()
            );
        }
    }
}
