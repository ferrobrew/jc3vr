//! Bounded held-prop impact strength and repeat-contact admission.

use glam::Vec3;

/// The contact-point motion before the physics step.
#[derive(Debug, Copy, Clone)]
pub(super) struct Motion {
    pub centre: Vec3,
    pub linear: Vec3,
    pub angular: Vec3,
    pub mass_kg: f32,
}

impl Motion {
    pub fn velocity_at(self, point: Vec3) -> Vec3 {
        self.linear + self.angular.cross(point - self.centre)
    }
}

/// The feedback strength of a real solver contact. Native damage and impulses are unchanged.
pub(super) fn strength(motion: Motion, at: Vec3, normal: Vec3, impulse_ns: f32) -> Option<f32> {
    if !motion.centre.is_finite()
        || !motion.linear.is_finite()
        || !motion.angular.is_finite()
        || !motion.mass_kg.is_finite()
        || motion.mass_kg <= 0.0
        || !at.is_finite()
        || !normal.is_finite()
        || !impulse_ns.is_finite()
        || impulse_ns <= 0.0
    {
        return None;
    }
    let normal = normal.try_normalize()?;
    let speed = motion.velocity_at(at).dot(normal).clamp(0.0, 12.0);
    if speed < 0.8 {
        return None;
    }
    let momentum = motion.mass_kg.clamp(0.25, 20.0) * speed;
    Some((momentum.min(impulse_ns) / 24.0).clamp(0.0, 1.0)).filter(|s| *s >= 0.05)
}

/// Feedback is limited per hand, including contacts without a cache.
#[derive(Default)]
pub(super) struct Gate {
    pub last: [Option<f64>; 2],
}

impl Gate {
    pub fn admit(&mut self, hand: usize, now: f64) -> bool {
        let due = self.last[hand].is_none_or(|at| now < at || now - at >= 0.2);
        if due {
            self.last[hand] = Some(now);
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn motion() -> Motion {
        Motion {
            centre: Vec3::ZERO,
            linear: Vec3::X * 3.0,
            angular: Vec3::ZERO,
            mass_kg: 10.0,
        }
    }

    #[test]
    fn rotation_strikes_at_the_contact_not_the_centre() {
        let m = Motion {
            linear: Vec3::ZERO,
            angular: Vec3::Y * 3.0,
            ..motion()
        };
        assert!(strength(m, Vec3::Z * 1.5, Vec3::X, 20.0).is_some());
        assert_eq!(strength(m, Vec3::ZERO, Vec3::X, 20.0), None);
    }

    #[test]
    fn resting_tangential_and_invalid_contacts_are_silent() {
        assert_eq!(
            strength(
                Motion {
                    linear: Vec3::ZERO,
                    ..motion()
                },
                Vec3::ZERO,
                Vec3::X,
                100.0
            ),
            None
        );
        assert_eq!(strength(motion(), Vec3::ZERO, Vec3::Y, 100.0), None);
        assert_eq!(strength(motion(), Vec3::ZERO, Vec3::X, f32::NAN), None);
        assert_eq!(
            strength(
                Motion {
                    mass_kg: f32::NAN,
                    ..motion()
                },
                Vec3::ZERO,
                Vec3::X,
                100.0
            ),
            None
        );
    }

    #[test]
    fn separating_motion_is_not_a_strike() {
        assert_eq!(strength(motion(), Vec3::ZERO, Vec3::NEG_X, 100.0), None);
    }

    #[test]
    fn mass_speed_and_solver_impulse_bound_the_feedback() {
        assert_eq!(strength(motion(), Vec3::ZERO, Vec3::X, 12.0), Some(0.5));
        assert_eq!(
            strength(
                Motion {
                    mass_kg: 1e6,
                    linear: Vec3::X * 1e6,
                    ..motion()
                },
                Vec3::ZERO,
                Vec3::X,
                1e6
            ),
            Some(1.0)
        );
    }

    #[test]
    fn repeated_contacts_do_not_buzz_and_hands_are_independent() {
        let mut gate = Gate::default();
        assert!(gate.admit(0, 1.0));
        assert!(!gate.admit(0, 1.1));
        assert!(gate.admit(1, 1.1));
        assert!(gate.admit(0, 1.21));
        assert!(gate.admit(0, 0.0));
    }
}
