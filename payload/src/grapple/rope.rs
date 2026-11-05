//! One rope between two ends: a tension-only spring-damper on the stretch past a held length,
//! normalised by the ends' reduced mass so that the same stretch asks the same acceleration of any
//! pair, and capped at a tension. Pure.
//!
//! Each end is a point with a velocity and an inverse mass; an inverse mass of zero is a kinematic
//! end, which the rope never moves: a hand, static geometry, or a target whose reaction is not
//! applied. The tension is shared by inverse mass, so a light end takes most of the motion.
//!
//! Two lines use it, never at once: the snatch's held line ([`crate::controllers::snatch`]), whose
//! kinematic end is the grapple hand and whose dynamic end the target it swings, and the grapple's
//! lock ([`crate::grapple::physical::lock`]), whose ends are the hook's point and the character.
//! The caller owns the held length's changes (a reel, a hand taking line in) and applies the
//! velocities it is handed.

use glam::Vec3;

/// One end of the rope, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RopeEnd {
    pub position: Vec3,
    pub velocity: Vec3,
    /// The end's inverse mass, per kilogram; zero for an end the rope does not move.
    pub inverse_mass: f32,
}

/// The rope's stiffness, damping, and limits for one step.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RopeTuning {
    /// The acceleration per metre of stretch, per second squared, for the ends' reduced mass.
    pub stiffness_per_s2: f32,
    /// The acceleration per metre per second of the ends parting along the rope, per second.
    pub damping_per_s: f32,
    /// The most tension the rope holds, newtons.
    pub max_tension_n: f32,
    /// How far past its held length the rope stretches before it breaks, metres.
    pub max_stretch_m: f32,
}

/// One step of the rope.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RopeStep {
    pub a: RopeEnd,
    pub b: RopeEnd,
    pub dt: f32,
    pub tuning: RopeTuning,
}

/// What a step of the rope did.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RopeOutput {
    /// The ends' velocities after the rope's pull.
    pub a_velocity: Vec3,
    pub b_velocity: Vec3,
    /// The rope's tension this step, newtons.
    pub tension_n: f32,
    /// The unit direction from `a` to `b`, or zero for coincident ends.
    pub direction: Vec3,
    /// The distance between the ends, and how far it is past the held length, metres.
    pub distance_m: f32,
    pub stretch_m: f32,
}

impl RopeOutput {
    /// The impulse the rope gave `a` this step, newton seconds: `-b`'s.
    pub fn impulse_on_a(&self, dt: f32) -> Vec3 {
        self.direction * self.tension_n * dt
    }

    /// The impulse the rope gave `b` this step, newton seconds: `-a`'s.
    pub fn impulse_on_b(&self, dt: f32) -> Vec3 {
        -self.impulse_on_a(dt)
    }
}

/// Why the rope could not take a step.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RopeFault {
    /// A sample was not finite, the step's length was out of bounds, or neither end can move.
    Invalid,
    /// The ends are farther apart than the rope stretches.
    Stretched,
}

/// The rope's held length.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Rope {
    pub length_m: f32,
}

impl Rope {
    /// A rope held at `length_m`, or `None` for a length that is not finite and positive.
    pub fn new(length_m: f32) -> Option<Self> {
        (length_m.is_finite() && length_m > MIN_LENGTH_M).then_some(Self { length_m })
    }

    /// Pull the ends together along the rope: preserve their motion across it, never push them
    /// apart through a slack rope, and bound the tension by the rope's strength.
    pub fn step(&self, step: RopeStep) -> Result<RopeOutput, RopeFault> {
        let RopeStep { a, b, dt, tuning } = step;
        let end_ok = |end: &RopeEnd| {
            end.position.is_finite()
                && end.velocity.is_finite()
                && end.inverse_mass.is_finite()
                && end.inverse_mass >= 0.0
        };
        if !end_ok(&a)
            || !end_ok(&b)
            || a.inverse_mass + b.inverse_mass <= 0.0
            || !dt.is_finite()
            || dt <= 0.0
            || dt > MAX_DT_S
            || !self.length_m.is_finite()
            || !tuning.stiffness_per_s2.is_finite()
            || !tuning.damping_per_s.is_finite()
            || !tuning.max_tension_n.is_finite()
            || !tuning.max_stretch_m.is_finite()
        {
            return Err(RopeFault::Invalid);
        }
        let radial = b.position - a.position;
        let distance_m = radial.length();
        let slack = |direction: Vec3| RopeOutput {
            a_velocity: a.velocity,
            b_velocity: b.velocity,
            tension_n: 0.0,
            direction,
            distance_m,
            stretch_m: distance_m - self.length_m,
        };
        let Some(direction) = radial.try_normalize() else {
            return Ok(slack(Vec3::ZERO));
        };
        if distance_m > self.length_m + tuning.max_stretch_m.max(0.0) {
            return Err(RopeFault::Stretched);
        }
        let extension = (distance_m - self.length_m).max(0.0);
        let outward = (b.velocity - a.velocity).dot(direction);
        let predicted_extension = (distance_m + outward * dt - self.length_m).max(0.0);
        if predicted_extension <= 0.0 {
            return Ok(slack(direction));
        }
        let accel = (tuning.stiffness_per_s2.max(0.0) * extension
            + tuning.damping_per_s.max(0.0) * outward)
            .max(0.0);
        let reduced_mass = 1.0 / (a.inverse_mass + b.inverse_mass);
        let tension_n = (accel * reduced_mass).min(tuning.max_tension_n.max(0.0));
        let impulse = tension_n * dt;
        Ok(RopeOutput {
            a_velocity: a.velocity + direction * impulse * a.inverse_mass,
            b_velocity: b.velocity - direction * impulse * b.inverse_mass,
            tension_n,
            direction,
            distance_m,
            stretch_m: distance_m - self.length_m,
        })
    }
}

/// Standard gravity, metres per second squared: what the lines, and the throws and holds beside
/// them, work against.
pub const GRAVITY_MPS2: f32 = 9.81;

/// `tension_n` as a share of the rope's strength `max_tension_n`, `0..=1`: none for a strength
/// that is not positive or a tension that is not finite.
pub fn tension_share(tension_n: f32, max_tension_n: f32) -> f32 {
    if max_tension_n > 0.0 && tension_n.is_finite() {
        (tension_n / max_tension_n).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The longest step the rope takes, seconds: a longer one is a hitch, not a tick.
const MAX_DT_S: f32 = 0.1;

/// The shortest held length, metres.
const MIN_LENGTH_M: f32 = 0.01;

#[cfg(test)]
mod tests {
    use super::*;

    const TUNING: RopeTuning = RopeTuning {
        stiffness_per_s2: 400.0,
        damping_per_s: 30.0,
        max_tension_n: 120.0,
        max_stretch_m: 3.0,
    };

    fn end(position: Vec3, velocity: Vec3, inverse_mass: f32) -> RopeEnd {
        RopeEnd {
            position,
            velocity,
            inverse_mass,
        }
    }

    fn step(a: RopeEnd, b: RopeEnd) -> RopeStep {
        RopeStep {
            a,
            b,
            dt: 0.01,
            tuning: TUNING,
        }
    }

    #[test]
    fn a_kinematic_end_is_never_moved_and_the_other_takes_the_whole_pull() {
        let rope = Rope::new(2.0).unwrap();
        let out = rope
            .step(step(
                end(Vec3::ZERO, Vec3::ZERO, 0.0),
                end(Vec3::X * 2.1, Vec3::new(5.0, 7.0, -9.0), 1.0),
            ))
            .unwrap();
        assert_eq!(out.a_velocity, Vec3::ZERO);
        assert_eq!(out.tension_n, 120.0);
        assert!((5.0 - out.b_velocity.x - 1.2).abs() < 1e-5);
        assert_eq!(out.b_velocity.y, 7.0);
        assert_eq!(out.b_velocity.z, -9.0);
    }

    #[test]
    fn the_tension_is_shared_by_inverse_mass_and_conserves_momentum() {
        let rope = Rope::new(2.0).unwrap();
        let tuning = RopeTuning {
            max_tension_n: f32::MAX,
            ..TUNING
        };
        let (light, heavy) = (1.0 / 80.0, 1.0 / 1500.0);
        let out = rope
            .step(RopeStep {
                tuning,
                ..step(
                    end(Vec3::ZERO, Vec3::ZERO, heavy),
                    end(Vec3::X * 2.2, Vec3::X * 3.0, light),
                )
            })
            .unwrap();
        let before = Vec3::X * 3.0 / light;
        let after = out.a_velocity / heavy + out.b_velocity / light;
        assert!((before - after).length() < 1e-2, "{before} {after}");
        let (dv_heavy, dv_light) = (out.a_velocity.x, 3.0 - out.b_velocity.x);
        assert!(dv_light > dv_heavy * 10.0);
        assert!((out.impulse_on_a(0.01) - out.a_velocity / heavy).length() < 1e-3);
    }

    #[test]
    fn the_same_stretch_asks_the_same_acceleration_of_any_pair() {
        let rope = Rope::new(2.0).unwrap();
        let tuning = RopeTuning {
            damping_per_s: 0.0,
            max_tension_n: f32::MAX,
            ..TUNING
        };
        let relative = |ia: f32, ib: f32| {
            let out = rope
                .step(RopeStep {
                    tuning,
                    ..step(
                        end(Vec3::ZERO, Vec3::ZERO, ia),
                        end(Vec3::X * 2.1, Vec3::ZERO, ib),
                    )
                })
                .unwrap();
            (out.a_velocity - out.b_velocity).x
        };
        let alone = relative(0.0, 1.0 / 90.0);
        let paired = relative(1.0 / 500.0, 1.0 / 90.0);
        assert!((alone - paired).abs() < 1e-4, "{alone} {paired}");
    }

    #[test]
    fn a_slack_rope_neither_pushes_nor_drags() {
        let rope = Rope::new(2.0).unwrap();
        let velocity = Vec3::new(-2.0, 3.0, 4.0);
        let out = rope
            .step(step(
                end(Vec3::ZERO, Vec3::ZERO, 0.0),
                end(Vec3::X, velocity, 1.0),
            ))
            .unwrap();
        assert_eq!(out.b_velocity, velocity);
        assert_eq!(out.tension_n, 0.0);
        assert!(out.stretch_m < 0.0);
    }

    #[test]
    fn the_motion_is_measured_against_the_moving_end() {
        // Both ends move together at the held length: nothing to pull, however fast.
        let rope = Rope::new(2.0).unwrap();
        let carried = Vec3::new(30.0, 0.0, 12.0);
        let out = rope
            .step(step(
                end(Vec3::ZERO, carried, 0.0),
                end(Vec3::X * 2.0, carried, 1.0),
            ))
            .unwrap();
        assert_eq!(out.tension_n, 0.0);
        assert_eq!(out.b_velocity, carried);
    }

    #[test]
    fn overstretch_breaks_and_bad_samples_are_refused() {
        let rope = Rope::new(2.0).unwrap();
        let fixed = end(Vec3::ZERO, Vec3::ZERO, 0.0);
        assert_eq!(
            rope.step(step(fixed, end(Vec3::X * 5.5, Vec3::ZERO, 1.0))),
            Err(RopeFault::Stretched)
        );
        assert_eq!(
            rope.step(step(fixed, end(Vec3::NAN, Vec3::ZERO, 1.0))),
            Err(RopeFault::Invalid)
        );
        assert_eq!(
            rope.step(step(fixed, end(Vec3::X, Vec3::ZERO, 0.0))),
            Err(RopeFault::Invalid),
            "two kinematic ends"
        );
        let mut long = step(fixed, end(Vec3::X, Vec3::ZERO, 1.0));
        long.dt = 0.5;
        assert_eq!(rope.step(long), Err(RopeFault::Invalid));
        assert!(Rope::new(0.0).is_none());
        assert!(Rope::new(f32::NAN).is_none());
        let coincident = rope
            .step(step(fixed, end(Vec3::ZERO, Vec3::Y, 1.0)))
            .unwrap();
        assert_eq!(coincident.b_velocity, Vec3::Y);
    }

    #[test]
    fn the_tension_share_is_bounded_and_refuses_bad_samples() {
        assert_eq!(tension_share(60.0, 120.0), 0.5);
        assert_eq!(tension_share(240.0, 120.0), 1.0);
        assert_eq!(tension_share(-5.0, 120.0), 0.0);
        assert_eq!(tension_share(60.0, 0.0), 0.0);
        assert_eq!(tension_share(f32::NAN, 120.0), 0.0);
        assert_eq!(tension_share(f32::INFINITY, 120.0), 0.0);
    }
}
