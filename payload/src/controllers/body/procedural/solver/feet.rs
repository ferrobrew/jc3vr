//! Planted feet that step: each foot holds its place in the world while the body moves and turns
//! above it, and steps to where the animation would have it once it has fallen too far behind.
//! Pure, in world space.
//!
//! One foot steps at a time, the worse one first; the other waits until it lands. A step is an
//! eased arc from the planted place to the wanted one, lifted at its middle. The ground is wherever
//! the wanted place is: the animation's foot over the root, so the heights are the root's.

use glam::{Quat, Vec3};

/// A foot's place and orientation in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FootPose {
    pub position: Vec3,
    pub rotation: Quat,
}

/// The stepping's tuning.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StepTuning {
    /// How far, horizontally, a foot may be from its wanted place before it steps, metres.
    pub threshold_m: f32,
    /// How far a foot may be turned from its wanted orientation before it steps, radians.
    pub threshold_rad: f32,
    /// How long a step takes, seconds.
    pub duration_s: f32,
    /// How high a step lifts the foot at its middle, metres.
    pub height_m: f32,
}

/// The two feet, left then right.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Feet {
    feet: [Option<Foot>; 2],
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum Foot {
    Planted(FootPose),
    Stepping {
        from: FootPose,
        to: FootPose,
        /// The progress through the step, `0..1`.
        t: f32,
    },
}

impl Feet {
    pub const fn new() -> Self {
        Self { feet: [None; 2] }
    }

    /// Lift both feet out of the world: the next step plants them where they are wanted.
    pub fn reset(&mut self) {
        self.feet = [None; 2];
    }

    /// Whether either foot is mid-step.
    pub fn stepping(&self) -> bool {
        self.feet
            .iter()
            .any(|f| matches!(f, Some(Foot::Stepping { .. })))
    }

    /// Advance by `dt` seconds toward the `wanted` poses, and return where the feet are.
    pub fn step(&mut self, wanted: [FootPose; 2], dt: f32, tuning: &StepTuning) -> [FootPose; 2] {
        for (foot, wanted) in self.feet.iter_mut().zip(wanted) {
            if foot.is_none() || !is_finite(foot.as_ref().map(Foot::pose)) {
                *foot = Some(Foot::Planted(wanted));
            }
        }
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let mut stepping = false;
        for (foot, wanted) in self.feet.iter_mut().zip(wanted) {
            if let Some(Foot::Stepping { to, t, .. }) = foot {
                // The wanted place moves on while the foot is in the air; the step lands there.
                *to = wanted;
                *t += if tuning.duration_s > 0.0 {
                    dt / tuning.duration_s
                } else {
                    1.0
                };
                if *t >= 1.0 {
                    *foot = Some(Foot::Planted(*to));
                } else {
                    stepping = true;
                }
            }
        }
        if !stepping {
            let errors = [0, 1].map(|i| match self.feet[i] {
                Some(Foot::Planted(pose)) => error(&pose, &wanted[i], tuning),
                _ => 0.0,
            });
            let worse = if errors[0] >= errors[1] { 0 } else { 1 };
            if errors[worse] > 1.0
                && let Some(Foot::Planted(pose)) = self.feet[worse]
            {
                self.feet[worse] = Some(Foot::Stepping {
                    from: pose,
                    to: wanted[worse],
                    t: 0.0,
                });
            }
        }
        [0, 1].map(|i| match self.feet[i] {
            Some(Foot::Planted(pose)) => pose,
            Some(Foot::Stepping { from, to, t }) => arc(&from, &to, t, tuning.height_m),
            None => wanted[i],
        })
    }
}

impl Foot {
    fn pose(&self) -> FootPose {
        match *self {
            Foot::Planted(pose) => pose,
            Foot::Stepping { from, .. } => from,
        }
    }
}

/// How far a planted foot is from its wanted pose, as a share of the step thresholds: above one,
/// it steps.
fn error(planted: &FootPose, wanted: &FootPose, tuning: &StepTuning) -> f32 {
    let offset = wanted.position - planted.position;
    let distance = Vec3::new(offset.x, 0.0, offset.z).length();
    let turn = planted.rotation.angle_between(wanted.rotation);
    let by_distance = distance / tuning.threshold_m.max(1e-3);
    let by_turn = turn / tuning.threshold_rad.max(1e-3);
    by_distance.max(by_turn)
}

/// The foot's pose `t` of the way through a step: eased along the line, lifted on a sine arch.
fn arc(from: &FootPose, to: &FootPose, t: f32, height: f32) -> FootPose {
    let t = t.clamp(0.0, 1.0);
    let eased = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    FootPose {
        position: from.position.lerp(to.position, eased)
            + Vec3::Y * (height * (std::f32::consts::PI * t).sin()),
        rotation: from.rotation.slerp(to.rotation, eased),
    }
}

fn is_finite(pose: Option<FootPose>) -> bool {
    pose.is_some_and(|p| p.position.is_finite() && p.rotation.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 30.0;

    fn tuning() -> StepTuning {
        StepTuning {
            threshold_m: 0.12,
            threshold_rad: 35f32.to_radians(),
            duration_s: 0.3,
            height_m: 0.06,
        }
    }

    fn at(x: f32, z: f32) -> FootPose {
        FootPose {
            position: Vec3::new(x, 0.0, z),
            rotation: Quat::IDENTITY,
        }
    }

    #[test]
    fn small_motion_leaves_the_feet_planted() {
        let mut feet = Feet::new();
        feet.step([at(-0.1, 0.0), at(0.1, 0.0)], DT, &tuning());
        let placed = feet.step([at(-0.05, 0.05), at(0.15, 0.05)], DT, &tuning());
        assert_eq!(placed, [at(-0.1, 0.0), at(0.1, 0.0)]);
    }

    #[test]
    fn a_far_body_steps_one_foot_then_the_other() {
        let mut feet = Feet::new();
        feet.step([at(-0.1, 0.0), at(0.1, 0.0)], DT, &tuning());
        let wanted = [at(-0.1, -0.3), at(0.1, -0.3)];
        let mut first_landed = None;
        let mut peak: f32 = 0.0;
        for tick in 0..60 {
            let placed = feet.step(wanted, DT, &tuning());
            peak = peak.max(placed[0].position.y.max(placed[1].position.y));
            // Never both in the air.
            assert!(placed[0].position.y < 1e-6 || placed[1].position.y < 1e-6);
            let landed = [0, 1].map(|i| placed[i].position.abs_diff_eq(wanted[i].position, 1e-5));
            if first_landed.is_none() && (landed[0] || landed[1]) {
                first_landed = Some(tick);
            }
            if landed[0] && landed[1] {
                assert!(peak > 0.05);
                assert!(!feet.stepping());
                return;
            }
        }
        panic!("the feet never both arrived");
    }

    #[test]
    fn a_turn_in_place_steps_the_feet() {
        let mut feet = Feet::new();
        feet.step([at(-0.1, 0.0), at(0.1, 0.0)], DT, &tuning());
        let turned = Quat::from_rotation_y(1.0);
        let wanted = [at(-0.1, 0.0), at(0.1, 0.0)].map(|p| FootPose {
            position: turned * p.position,
            rotation: turned,
        });
        feet.step(wanted, DT, &tuning());
        assert!(feet.stepping());
    }

    #[test]
    fn a_reset_plants_where_wanted() {
        let mut feet = Feet::new();
        feet.step([at(0.0, 0.0), at(0.0, 0.0)], DT, &tuning());
        feet.reset();
        assert_eq!(
            feet.step([at(1.0, 1.0), at(2.0, 2.0)], DT, &tuning()),
            [at(1.0, 1.0), at(2.0, 2.0)]
        );
    }
}
