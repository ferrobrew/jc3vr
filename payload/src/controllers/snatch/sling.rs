//! The held line: a stiff, tension-only spring-damper between the grapple hand and the target's
//! centre at a held length, which changes only the target's velocity along the line relative to the
//! moving hand. The tension it may reach scales with the target's mass, so a heavy target takes a
//! real swing to move, but does move.

use glam::Vec3;

use crate::{
    controllers::snatch::config::{REFERENCE_MASS_KG, SwingConfig},
    grapple::rope::{self, Rope, RopeEnd, RopeFault, RopeOutput, RopeTuning},
};

/// The owner facts required before continuing any snatch phase.
pub(super) struct Continuation {
    pub enabled: bool,
    pub on_foot: bool,
    pub tracked: bool,
    pub same_hand: bool,
    pub empty_hand: bool,
    pub grip_held: bool,
}

impl Continuation {
    pub fn cancellation(&self) -> Option<&'static str> {
        if !self.tracked {
            Some("the grapple hand lost tracking")
        } else if !self.same_hand || !self.empty_hand {
            Some("the grapple hand is no longer available")
        } else if !self.enabled || !self.on_foot {
            Some("the snatch is off or the player left their feet")
        } else if !self.grip_held {
            Some("the grip opened")
        } else {
            None
        }
    }
}

/// The attached target's next operation, before any engine access.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum AttachedMode {
    Swing,
    Resume,
    Pull,
    Cancel,
}

/// What the attached target does this tick: `enabled` is the swing's switch, `button_held` the
/// snatch's hold of B, `swinging` whether the line holds it already, and `tethered` whether it is
/// too heavy to come to the hand, which swings it whatever B does.
pub(super) fn attached_mode(
    enabled: bool,
    button_held: bool,
    swinging: bool,
    tethered: bool,
) -> AttachedMode {
    match (enabled, button_held, swinging, tethered) {
        (false, _, _, true) | (false, _, true, _) => AttachedMode::Cancel,
        (true, _, _, true) | (true, true, _, _) => AttachedMode::Swing,
        (_, false, true, _) => AttachedMode::Resume,
        _ => AttachedMode::Pull,
    }
}

/// The most tension the line holds on a target of `mass_kg`, newtons: `strength_n` at
/// [`REFERENCE_MASS_KG`], growing with the mass to the power `scaling`, `0..=1`. Below 1 the
/// acceleration the line can give falls as the mass grows. A mass that is not positive gets none.
pub(super) fn line_strength_n(mass_kg: f32, strength_n: f32, scaling: f32) -> f32 {
    if !(mass_kg.is_finite() && mass_kg > 0.0 && strength_n.is_finite()) {
        return 0.0;
    }
    let scaling = if scaling.is_finite() {
        scaling.clamp(0.0, 1.0)
    } else {
        1.0
    };
    strength_n.max(0.0) * (mass_kg / REFERENCE_MASS_KG).powf(scaling)
}

/// The line's strength while braced, or not, as a multiple of one hand's `single_n`. A brace never
/// weakens the line.
pub(super) fn braced_strength_n(single_n: f32, factor: f32, braced: bool) -> f32 {
    if braced && factor.is_finite() {
        single_n * factor.max(1.0)
    } else {
        single_n
    }
}

/// The held length after one tick of `dt` reeling in at `reel_mps` toward `floor_m`, from
/// `length_m` with the target `distance_m` from the hand. The reel takes in line no faster than
/// the target comes, keeping it at most [`REEL_LEAD_M`] inside the target, so a heavy target that
/// lags is pulled hard rather than left far beyond the line. It never pays line out.
pub(super) fn reeled_length(
    length_m: f32,
    distance_m: f32,
    floor_m: f32,
    reel_mps: f32,
    dt: f32,
) -> f32 {
    if !(reel_mps.is_finite() && floor_m.is_finite() && distance_m.is_finite()) {
        return length_m;
    }
    (length_m - reel_mps.max(0.0) * dt)
        .max(floor_m.max(MIN_LENGTH_M))
        .max(distance_m - REEL_LEAD_M)
        .min(length_m)
}

/// How far inside the target the reel may take the line in, metres: at the default stiffness, a
/// pull of about the strength of any line up to a few hundred kilograms.
pub(super) const REEL_LEAD_M: f32 = 0.3;

/// How far the target may be stretched past the held length before the line breaks, metres.
pub(super) const MAX_STRETCH_M: f32 = 3.0;

/// The held line: the shared rope ([`Rope`]) from the tracked hand to the target, and the hand's
/// previous anchor, from which the hand's velocity is measured.
#[derive(Debug, Copy, Clone)]
pub(super) struct Sling {
    rope: Rope,
    anchor: Vec3,
}

/// The line's tuning for one step, with the strength for this target and brace.
#[derive(Debug, Copy, Clone)]
pub(super) struct LineTuning {
    pub stiffness_per_s2: f32,
    pub damping_per_s: f32,
    pub max_tension_n: f32,
    pub swing_length_m: f32,
    pub reel_mps: f32,
}

impl LineTuning {
    /// The tuning of `config` for a target of `mass_kg`, braced or not.
    pub fn of(config: &SwingConfig, mass_kg: f32, braced: bool) -> Self {
        let single = line_strength_n(mass_kg, config.strength_n, config.mass_scaling);
        Self {
            stiffness_per_s2: config.stiffness_per_s2,
            damping_per_s: config.damping_per_s,
            max_tension_n: braced_strength_n(single, config.brace_factor, braced),
            swing_length_m: config.swing_length_m,
            reel_mps: config.reel_mps,
        }
    }
}

/// One pre-simulation correction along the line; gravity and collisions remain the engine's work.
pub(super) struct SlingStep {
    pub centre: Vec3,
    pub velocity: Vec3,
    pub mass_kg: f32,
    pub anchor: Vec3,
    pub dt: f32,
    pub line: LineTuning,
}

impl Sling {
    pub fn new(centre: Vec3, anchor: Vec3) -> Option<Self> {
        Some(Self {
            rope: Rope::new(centre.distance(anchor))?,
            anchor,
        })
    }

    /// The line's held length, metres.
    pub fn length_m(&self) -> f32 {
        self.rope.length_m
    }

    /// Move the remembered anchor by `delta`, for a change of what the line is anchored to (a
    /// brace taken or let go) that is not the hand's motion.
    pub fn shift_anchor(&mut self, delta: Vec3) {
        if delta.is_finite() {
            self.anchor += delta;
        }
    }

    /// Reel the line in, then pull along it with the hand as the rope's kinematic end: preserve
    /// the tangent, never push a slack rope, and bound the tension by the line's strength over a
    /// bounded time step. The target is the rope's `b` end; a hand that jumped between ticks is
    /// [`RopeFault::Invalid`].
    pub fn step(&mut self, step: SlingStep) -> Result<RopeOutput, RopeFault> {
        let SlingStep {
            centre,
            velocity,
            mass_kg,
            anchor,
            dt,
            line,
        } = step;
        if !centre.is_finite()
            || !anchor.is_finite()
            || !dt.is_finite()
            || dt <= 0.0
            || !(mass_kg.is_finite() && mass_kg > 0.0)
        {
            return Err(RopeFault::Invalid);
        }
        let anchor_velocity = (anchor - self.anchor) / dt;
        self.anchor = anchor;
        if anchor_velocity.length() > 30.0 {
            return Err(RopeFault::Invalid);
        }
        self.rope.length_m = reeled_length(
            self.rope.length_m,
            centre.distance(anchor),
            line.swing_length_m,
            line.reel_mps,
            dt,
        );
        self.rope.step(rope::RopeStep {
            a: RopeEnd {
                position: anchor,
                velocity: anchor_velocity,
                inverse_mass: 0.0,
            },
            b: RopeEnd {
                position: centre,
                velocity,
                inverse_mass: 1.0 / mass_kg,
            },
            dt,
            tuning: RopeTuning {
                stiffness_per_s2: line.stiffness_per_s2,
                damping_per_s: line.damping_per_s,
                max_tension_n: line.max_tension_n,
                max_stretch_m: MAX_STRETCH_M,
            },
        })
    }
}

/// The shortest held length, metres, whatever the swing's length is set to.
const MIN_LENGTH_M: f32 = 0.5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grip_tracking_context_and_hand_ownership_loss_cancel_before_driving() {
        for lost in 0..6 {
            let mut input = Continuation {
                enabled: true,
                on_foot: true,
                tracked: true,
                same_hand: true,
                empty_hand: true,
                grip_held: true,
            };
            assert_eq!(input.cancellation(), None);
            match lost {
                0 => input.enabled = false,
                1 => input.on_foot = false,
                2 => input.tracked = false,
                3 => input.same_hand = false,
                4 => input.empty_hand = false,
                _ => input.grip_held = false,
            }
            assert!(input.cancellation().is_some());
        }
    }

    #[test]
    fn hold_bypasses_arrival_and_stall_until_resume_and_disable_cancels() {
        assert_eq!(attached_mode(true, true, false, false), AttachedMode::Swing);
        assert_eq!(attached_mode(true, true, true, false), AttachedMode::Swing);
        assert_eq!(
            attached_mode(true, false, true, false),
            AttachedMode::Resume
        );
        assert_eq!(attached_mode(true, false, false, false), AttachedMode::Pull);
        assert_eq!(
            attached_mode(false, true, true, false),
            AttachedMode::Cancel
        );
        assert_eq!(
            attached_mode(false, false, false, false),
            AttachedMode::Pull
        );
    }

    #[test]
    fn a_tethered_target_swings_whatever_b_does_and_never_resumes_the_pull() {
        for button in [false, true] {
            for swinging in [false, true] {
                assert_eq!(
                    attached_mode(true, button, swinging, true),
                    AttachedMode::Swing
                );
                assert_eq!(
                    attached_mode(false, button, swinging, true),
                    AttachedMode::Cancel
                );
            }
        }
    }

    #[test]
    fn the_strength_grows_with_the_mass_but_the_acceleration_falls() {
        let at = |mass: f32| line_strength_n(mass, 1500.0, 0.5);
        assert!((at(REFERENCE_MASS_KG) - 1500.0).abs() < 1e-3);
        let (rifle, barrel, heavy) = (at(4.0), at(100.0), at(400.0));
        assert!(rifle < at(10.0) && at(10.0) < barrel && barrel < heavy);
        assert!(rifle / 4.0 > barrel / 100.0 && barrel / 100.0 > heavy / 400.0);
        assert!(heavy / 400.0 > 9.81, "a heavy target still moves: {heavy}");
    }

    #[test]
    fn the_mass_scaling_spans_a_fixed_pull_to_a_fixed_acceleration() {
        assert!((line_strength_n(100.0, 1500.0, 0.0) - 1500.0).abs() < 1e-3);
        assert!(
            (line_strength_n(100.0, 1500.0, 1.0) / 100.0 - 1500.0 / REFERENCE_MASS_KG).abs() < 1e-2
        );
        assert_eq!(
            line_strength_n(100.0, 1500.0, 7.0),
            line_strength_n(100.0, 1500.0, 1.0)
        );
        assert_eq!(line_strength_n(0.0, 1500.0, 0.5), 0.0);
        assert_eq!(line_strength_n(f32::NAN, 1500.0, 0.5), 0.0);
    }

    #[test]
    fn a_brace_multiplies_the_strength_and_never_weakens_it() {
        assert_eq!(braced_strength_n(1000.0, 1.75, true), 1750.0);
        assert_eq!(braced_strength_n(1000.0, 1.75, false), 1000.0);
        assert_eq!(braced_strength_n(1000.0, 0.5, true), 1000.0);
        assert_eq!(braced_strength_n(1000.0, f32::NAN, true), 1000.0);
    }

    #[test]
    fn the_reel_takes_in_line_toward_the_swing_length_no_faster_than_the_target_comes() {
        assert!((reeled_length(10.0, 10.0, 3.0, 8.0, 0.02) - 9.84).abs() < 1e-4);
        assert!((reeled_length(10.0, 10.1, 3.0, 8.0, 0.1) - (10.1 - REEL_LEAD_M)).abs() < 1e-4);
        assert_eq!(reeled_length(3.1, 3.1, 3.0, 8.0, 0.1), 3.0);
        assert_eq!(
            reeled_length(2.0, 2.0, 3.0, 8.0, 0.1),
            2.0,
            "never pays out"
        );
        assert_eq!(
            reeled_length(10.0, 13.0, 3.0, 8.0, 0.1),
            10.0,
            "never pays out"
        );
        assert_eq!(reeled_length(1.0, 0.2, 0.0, 8.0, 0.1), MIN_LENGTH_M);
    }

    fn line(max_tension_n: f32) -> LineTuning {
        LineTuning {
            stiffness_per_s2: 400.0,
            damping_per_s: 30.0,
            max_tension_n,
            swing_length_m: 0.0,
            reel_mps: 0.0,
        }
    }

    fn step(centre: Vec3, velocity: Vec3, anchor: Vec3) -> SlingStep {
        SlingStep {
            centre,
            velocity,
            mass_kg: 1.0,
            anchor,
            dt: 0.01,
            line: line(120.0),
        }
    }

    #[test]
    fn tension_is_radial_and_bounded_and_release_needs_no_velocity_write() {
        let mut rope = Sling::new(Vec3::X * 2.0, Vec3::ZERO).unwrap();
        let before = Vec3::new(5.0, 7.0, -9.0);
        let after = rope.step(step(Vec3::X * 2.1, before, Vec3::ZERO)).unwrap();
        assert_eq!(after.b_velocity.y, before.y);
        assert_eq!(after.b_velocity.z, before.z);
        assert!((before.x - after.b_velocity.x - 1.2).abs() < 1e-5);
        assert_eq!(after.tension_n, 120.0);
        let released = Continuation {
            enabled: true,
            on_foot: true,
            tracked: true,
            same_hand: true,
            empty_hand: true,
            grip_held: false,
        };
        assert_eq!(released.cancellation(), Some("the grip opened"));
    }

    #[test]
    fn the_same_pull_moves_a_heavier_target_less() {
        let pull = |mass_kg: f32| {
            let mut rope = Sling::new(Vec3::X * 2.0, Vec3::ZERO).unwrap();
            let max = line_strength_n(mass_kg, 1500.0, 0.5);
            let out = rope
                .step(SlingStep {
                    mass_kg,
                    line: line(max),
                    ..step(Vec3::X * 2.2, Vec3::ZERO, Vec3::ZERO)
                })
                .unwrap();
            (-out.b_velocity.x, out.tension_n)
        };
        let (rifle_dv, rifle_n) = pull(4.0);
        let (barrel_dv, barrel_n) = pull(100.0);
        assert!(barrel_n > rifle_n, "the line pulls a barrel harder");
        assert!(barrel_dv < rifle_dv, "but the barrel speeds up less");
        assert!(barrel_dv > 0.0);
    }

    #[test]
    fn slack_never_pushes_or_drags_the_tangent() {
        let mut rope = Sling::new(Vec3::X * 2.0, Vec3::ZERO).unwrap();
        let velocity = Vec3::new(-2.0, 3.0, 4.0);
        let out = rope.step(step(Vec3::X, velocity, Vec3::ZERO)).unwrap();
        assert_eq!(out.b_velocity, velocity);
        assert_eq!(out.tension_n, 0.0);
    }

    #[test]
    fn moving_anchor_uses_relative_radial_speed() {
        let mut rope = Sling::new(Vec3::X * 2.0, Vec3::ZERO).unwrap();
        let velocity = Vec3::new(2.0, 8.0, 0.0);
        assert_eq!(
            rope.step(step(Vec3::X * 2.02, velocity, Vec3::X * 0.02))
                .unwrap()
                .b_velocity,
            velocity
        );
    }

    #[test]
    fn a_shifted_anchor_is_not_read_as_the_hand_moving() {
        let mut rope = Sling::new(Vec3::X * 2.0, Vec3::ZERO).unwrap();
        rope.shift_anchor(Vec3::X * 0.05);
        let out = rope
            .step(step(Vec3::X * 2.05, Vec3::ZERO, Vec3::X * 0.05))
            .unwrap();
        assert_eq!(out.tension_n, 0.0);
    }

    #[test]
    fn repeated_tension_keeps_a_free_swing_bounded_without_dragging_the_tangent() {
        let mut rope = Sling::new(Vec3::X * 2.0, Vec3::ZERO).unwrap();
        let mut centre = Vec3::X * 2.0;
        let mut velocity = Vec3::Y * 6.0;
        for _ in 0..2000 {
            velocity = rope
                .step(step(centre, velocity, Vec3::ZERO))
                .unwrap()
                .b_velocity;
            centre += velocity * 0.01;
            assert!(centre.length() < 2.3);
        }
        assert!(velocity.length() > 4.0);
        assert_eq!(rope.length_m(), 2.0);
    }

    #[test]
    fn a_hand_whirling_a_heavy_target_brings_it_up_to_speed() {
        // A 100 kg target at rest 3 m off, the hand circling at 0.4 m and 1.5 turns a second.
        let mass_kg = 100.0;
        let config = SwingConfig::new();
        let tuning = LineTuning::of(&config, mass_kg, false);
        let hand = |t: f32| {
            let a = t * std::f32::consts::TAU * 1.5;
            Vec3::new(a.cos(), a.sin(), 0.0) * 0.4
        };
        let mut rope = Sling::new(Vec3::X * 3.4, hand(0.0)).unwrap();
        let (mut centre, mut velocity) = (Vec3::X * 3.4, Vec3::ZERO);
        let dt = 1.0 / 60.0;
        for i in 1..=600 {
            let out = rope
                .step(SlingStep {
                    centre,
                    velocity,
                    mass_kg,
                    anchor: hand(i as f32 * dt),
                    dt,
                    line: tuning,
                })
                .unwrap();
            velocity = out.b_velocity;
            centre += velocity * dt;
        }
        assert!(velocity.length() > 8.0, "{}", velocity.length());
        assert!(centre.length() < rope.length_m() + MAX_STRETCH_M);
    }

    #[test]
    fn the_line_reels_a_far_target_in_no_faster_than_the_reel() {
        let mass_kg = 100.0;
        let config = SwingConfig::new();
        let tuning = LineTuning::of(&config, mass_kg, false);
        let mut rope = Sling::new(Vec3::X * 12.0, Vec3::ZERO).unwrap();
        let (mut centre, mut velocity) = (Vec3::X * 12.0, Vec3::ZERO);
        let dt = 1.0 / 60.0;
        let mut fastest = 0.0_f32;
        while rope.length_m() > config.swing_length_m {
            velocity = rope
                .step(SlingStep {
                    centre,
                    velocity,
                    mass_kg,
                    anchor: Vec3::ZERO,
                    dt,
                    line: tuning,
                })
                .unwrap()
                .b_velocity;
            centre += velocity * dt;
            fastest = fastest.max(-velocity.x);
        }
        assert!(fastest > config.reel_mps * 0.8, "{fastest}");
        assert!(fastest < config.reel_mps * 1.5, "{fastest}");
    }

    #[test]
    fn coincident_invalid_and_discontinuous_samples_are_safe() {
        assert!(Sling::new(Vec3::ZERO, Vec3::ZERO).is_none());
        let mut rope = Sling::new(Vec3::X, Vec3::ZERO).unwrap();
        assert_eq!(
            rope.step(step(Vec3::ZERO, Vec3::Y, Vec3::ZERO))
                .unwrap()
                .b_velocity,
            Vec3::Y
        );
        assert_eq!(
            rope.step(step(Vec3::NAN, Vec3::ZERO, Vec3::ZERO)),
            Err(RopeFault::Invalid)
        );
        assert_eq!(
            rope.step(step(Vec3::X, Vec3::ZERO, Vec3::Y)),
            Err(RopeFault::Invalid)
        );
        let mut invalid = step(Vec3::X, Vec3::ZERO, Vec3::ZERO);
        invalid.dt = 0.0;
        assert_eq!(rope.step(invalid), Err(RopeFault::Invalid));
        let mut massless = step(Vec3::X, Vec3::ZERO, Vec3::ZERO);
        massless.mass_kg = 0.0;
        assert_eq!(rope.step(massless), Err(RopeFault::Invalid));
    }

    #[test]
    fn a_target_far_past_the_held_length_breaks_the_line() {
        let mut rope = Sling::new(Vec3::X * 2.0, Vec3::ZERO).unwrap();
        assert_eq!(
            rope.step(step(Vec3::X * 5.5, Vec3::ZERO, Vec3::ZERO)),
            Err(RopeFault::Stretched)
        );
    }
}
