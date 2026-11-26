//! Bounded held-prop impact strength and repeat-contact admission.

use body_rig::geometry::point_velocity;
use glam::Vec3;

use crate::controllers::physical_hands::characters::tuning::{
    FeedbackTuning, ObjectDamageTuning, StrikeTuning,
};

/// The contact-point motion before the physics step.
#[derive(Debug, Copy, Clone)]
pub(super) struct Motion {
    pub centre: Vec3,
    pub linear: Vec3,
    pub angular: Vec3,
    pub mass_kg: f32,
}

impl Motion {
    #[cfg(test)]
    pub const STILL: Self = Self {
        centre: Vec3::ZERO,
        linear: Vec3::ZERO,
        angular: Vec3::ZERO,
        mass_kg: 0.0,
    };

    pub fn velocity_at(self, point: Vec3) -> Vec3 {
        point_velocity(self.centre, self.linear, self.angular, point)
    }
}

/// The bounded momentum of an intentional held-body strike, independent of feedback admission.
pub(super) fn damage_impulse(
    held: Motion,
    target: Motion,
    at: Vec3,
    normal: Vec3,
    solver_impulse: f32,
    tuning: &StrikeTuning,
) -> Option<f32> {
    if !held.centre.is_finite()
        || !held.linear.is_finite()
        || !held.angular.is_finite()
        || !held.mass_kg.is_finite()
        || held.mass_kg <= 0.0
        || !target.centre.is_finite()
        || !target.linear.is_finite()
        || !target.angular.is_finite()
        || !at.is_finite()
        || !solver_impulse.is_finite()
        || solver_impulse <= 0.0
    {
        return None;
    }
    let normal = normal.try_normalize()?;
    let own_speed = held.velocity_at(at).dot(normal);
    let relative_speed = (held.velocity_at(at) - target.velocity_at(at)).dot(normal);
    if !relative_speed.is_finite()
        || own_speed < tuning.min_speed_mps
        || relative_speed < tuning.min_speed_mps
    {
        return None;
    }
    let momentum = held.mass_kg.clamp(tuning.min_mass_kg, tuning.max_mass_kg)
        * own_speed.min(relative_speed).min(tuning.max_speed_mps);
    Some(momentum.min(solver_impulse).min(tuning.max_impulse_ns))
        .filter(|v| *v >= tuning.min_impulse_ns)
}

/// Health points for native object impact damage, with no detonation override: at least one.
pub(super) fn object_damage(impulse_ns: f32, tuning: &ObjectDamageTuning) -> Option<i16> {
    (impulse_ns.is_finite() && impulse_ns >= tuning.min_impulse_ns).then(|| {
        (impulse_ns * tuning.health_per_ns)
            .round()
            .clamp(1.0, tuning.max_health) as i16
    })
}

/// The feedback strength of a real solver contact.
pub(super) fn strength(
    motion: Motion,
    at: Vec3,
    normal: Vec3,
    impulse_ns: f32,
    tuning: &FeedbackTuning,
) -> Option<f32> {
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
    let speed = motion
        .velocity_at(at)
        .dot(normal)
        .clamp(0.0, tuning.max_speed_mps);
    if speed < tuning.min_speed_mps {
        return None;
    }
    let momentum = motion.mass_kg.clamp(tuning.min_mass_kg, tuning.max_mass_kg) * speed;
    Some((momentum.min(impulse_ns) / tuning.full_momentum_ns).clamp(0.0, 1.0))
        .filter(|s| *s >= tuning.min_share)
}

/// Feedback is limited per hand, including contacts without a cache, to one per `repeat_s`
/// ([`FeedbackTuning::repeat_s`]).
#[derive(Default)]
pub(super) struct Gate {
    pub last: [Option<f64>; 2],
}

impl Gate {
    pub fn admit(&mut self, hand: usize, now: f64, repeat_s: f64) -> bool {
        let due = self.last[hand].is_none_or(|at| now < at || now - at >= repeat_s);
        if due {
            self.last[hand] = Some(now);
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::physical_hands::characters::tuning::CHARACTERS;

    fn strike() -> &'static StrikeTuning {
        &CHARACTERS.get().impacts.strike
    }

    fn feedback() -> &'static FeedbackTuning {
        &CHARACTERS.get().impacts.feedback
    }

    fn damage() -> &'static ObjectDamageTuning {
        &CHARACTERS.get().impacts.object_damage
    }

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
        assert!(strength(m, Vec3::Z * 1.5, Vec3::X, 20.0, feedback()).is_some());
        assert_eq!(strength(m, Vec3::ZERO, Vec3::X, 20.0, feedback()), None);
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
                100.0,
                feedback()
            ),
            None
        );
        assert_eq!(
            strength(motion(), Vec3::ZERO, Vec3::Y, 100.0, feedback()),
            None
        );
        assert_eq!(
            strength(motion(), Vec3::ZERO, Vec3::X, f32::NAN, feedback()),
            None
        );
        assert_eq!(
            strength(
                Motion {
                    mass_kg: f32::NAN,
                    ..motion()
                },
                Vec3::ZERO,
                Vec3::X,
                100.0,
                feedback()
            ),
            None
        );
    }

    #[test]
    fn separating_motion_is_not_a_strike() {
        assert_eq!(
            strength(motion(), Vec3::ZERO, Vec3::NEG_X, 100.0, feedback()),
            None
        );
    }

    #[test]
    fn mass_speed_and_solver_impulse_bound_the_feedback() {
        assert_eq!(
            strength(motion(), Vec3::ZERO, Vec3::X, 12.0, feedback()),
            Some(0.5)
        );
        assert_eq!(
            strength(
                Motion {
                    mass_kg: 1e6,
                    linear: Vec3::X * 1e6,
                    ..motion()
                },
                Vec3::ZERO,
                Vec3::X,
                1e6,
                feedback()
            ),
            Some(1.0)
        );
    }

    #[test]
    fn damage_requires_an_intentional_swing_and_relative_closing() {
        let contact =
            |held, target| damage_impulse(held, target, Vec3::ZERO, Vec3::X, 1000.0, strike());
        assert_eq!(contact(Motion::STILL, motion()), None);
        assert_eq!(
            contact(
                Motion {
                    linear: Vec3::X * 1.49,
                    ..motion()
                },
                Motion::STILL
            ),
            None
        );
        assert_eq!(contact(motion(), motion()), None);
        assert_eq!(
            contact(
                motion(),
                Motion {
                    linear: Vec3::X * 5.0,
                    ..motion()
                }
            ),
            None
        );
        assert_eq!(contact(motion(), Motion::STILL), Some(30.0));
        assert_eq!(
            contact(
                motion(),
                Motion {
                    linear: Vec3::X,
                    ..motion()
                }
            ),
            Some(20.0)
        );
        assert_eq!(
            contact(
                motion(),
                Motion {
                    linear: Vec3::NEG_X * 1000.0,
                    ..motion()
                }
            ),
            Some(30.0)
        );
    }

    #[test]
    fn damage_uses_rotational_point_speed_and_caps_mass_speed_and_solver_impulse() {
        let rotating = Motion {
            linear: Vec3::ZERO,
            angular: Vec3::Y * 3.0,
            ..motion()
        };
        assert_eq!(
            damage_impulse(rotating, Motion::STILL, Vec3::Z, Vec3::X, 1000.0, strike()),
            Some(30.0)
        );
        assert_eq!(
            damage_impulse(
                rotating,
                Motion::STILL,
                Vec3::ZERO,
                Vec3::X,
                1000.0,
                strike()
            ),
            None
        );
        assert_eq!(
            damage_impulse(motion(), Motion::STILL, Vec3::ZERO, Vec3::X, 4.0, strike()),
            Some(4.0)
        );
        let extreme = Motion {
            linear: Vec3::X * 1e6,
            mass_kg: 1e6,
            ..motion()
        };
        assert_eq!(
            damage_impulse(extreme, Motion::STILL, Vec3::ZERO, Vec3::X, 1e6, strike()),
            Some(120.0)
        );
        assert_eq!(object_damage(120.0, damage()), Some(90));
        assert_eq!(object_damage(1e6, damage()), Some(90));
        assert_eq!(object_damage(2.99, damage()), None);
        assert_eq!(object_damage(f32::NAN, damage()), None);
    }

    #[test]
    fn damage_rejects_invalid_contact_geometry_and_motion() {
        for invalid in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
            assert_eq!(
                damage_impulse(
                    motion(),
                    Motion::STILL,
                    Vec3::ZERO,
                    Vec3::X,
                    invalid,
                    strike()
                ),
                None
            );
        }
        for invalid in [Vec3::NAN, Vec3::INFINITY] {
            assert_eq!(
                damage_impulse(
                    motion(),
                    Motion {
                        linear: invalid,
                        ..Motion::STILL
                    },
                    Vec3::ZERO,
                    Vec3::X,
                    100.0,
                    strike()
                ),
                None
            );
            assert_eq!(
                damage_impulse(motion(), Motion::STILL, invalid, Vec3::X, 100.0, strike()),
                None
            );
            assert_eq!(
                damage_impulse(
                    motion(),
                    Motion::STILL,
                    Vec3::ZERO,
                    invalid,
                    100.0,
                    strike()
                ),
                None
            );
        }
    }

    #[test]
    fn repeated_contacts_do_not_buzz_and_hands_are_independent() {
        let mut gate = Gate::default();
        assert!(gate.admit(0, 1.0, feedback().repeat_s));
        assert!(!gate.admit(0, 1.1, feedback().repeat_s));
        assert!(gate.admit(1, 1.1, feedback().repeat_s));
        assert!(gate.admit(0, 1.21, feedback().repeat_s));
        assert!(gate.admit(0, 0.0, feedback().repeat_s));
    }
}
