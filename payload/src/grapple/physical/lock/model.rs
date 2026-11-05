//! The lock's swing: the character falling under gravity on the shared rope from the hook's point,
//! the brake that turns the reel's rush toward the hook into a swing when the lock takes hold, and
//! the grapple hand taking line in. Pure.

use glam::Vec3;

use crate::{
    controllers::bindings::yank::{HandLine, reach_along},
    grapple::{
        physical::config::LockConfig,
        rope::{GRAVITY_MPS2, Rope, RopeEnd, RopeFault, RopeStep, RopeTuning, held_floor},
    },
};

/// The held line between ticks.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Held {
    pub rope: Rope,
    /// The brake's time still to run, seconds; zero once the line holds its length.
    pub brake_left_s: f32,
}

impl Held {
    /// A line held at the distance from `hook` to `rider`, no shorter than `config`'s least
    /// length, braking for `config`'s brake time.
    pub(crate) fn new(hook: Vec3, rider: Vec3, config: &LockConfig) -> Option<Self> {
        let distance = hook.distance(rider);
        if !distance.is_finite() {
            return None;
        }
        Some(Self {
            rope: Rope::new(distance.max(held_floor(config.min_length_m)))?,
            brake_left_s: config.brake_s.max(0.0),
        })
    }

    /// Take `taken_m` of line in, down to `config`'s least length.
    pub(crate) fn take_in(&mut self, taken_m: f32, config: &LockConfig) {
        self.rope.shorten(taken_m, config.min_length_m);
    }

    /// Whether the brake still runs.
    pub(crate) fn braking(&self) -> bool {
        self.brake_left_s > 0.0
    }
}

/// One reel tick of the swing, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct SwingInput {
    /// The character's position and velocity, as the reel task reads them.
    pub rider: Vec3,
    pub rider_velocity: Vec3,
    pub rider_mass_kg: f32,
    /// The hook's point and its velocity.
    pub hook: Vec3,
    pub hook_velocity: Vec3,
    /// The inverse mass the line pulls the hook's point back with: the target body's, scaled by
    /// the reaction's share; zero for a target the line does not move.
    pub target_inverse_mass: f32,
    pub dt: f32,
}

/// What a tick of the swing did.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct SwingOutput {
    /// The character's velocity for the tick, its whole motion.
    pub velocity: Vec3,
    pub tension_n: f32,
    pub max_tension_n: f32,
    /// How far the character is past the held length, metres: negative while the line is slack.
    pub stretch_m: f32,
    /// The line's impulse on the hook's point this tick, newton seconds.
    pub target_impulse: Vec3,
}

/// Advance the swing by a tick: gravity (the reel's velocity carries none, so the lock adds it),
/// the brake while it runs (the held length following the character in), then the rope. The
/// velocity is measured against the hook's point, so the swing holds its offset from a moving
/// target.
pub(crate) fn swing(
    held: &mut Held,
    input: SwingInput,
    config: &LockConfig,
) -> Result<SwingOutput, RopeFault> {
    let SwingInput {
        rider,
        rider_velocity,
        rider_mass_kg,
        hook,
        hook_velocity,
        target_inverse_mass,
        dt,
    } = input;
    if !(rider_mass_kg.is_finite() && rider_mass_kg > 0.0 && dt.is_finite() && dt > 0.0) {
        return Err(RopeFault::Invalid);
    }
    let mut velocity = rider_velocity - Vec3::Y * GRAVITY_MPS2 * dt;
    if held.braking() {
        let offset = rider - hook;
        if let Some(outward) = offset.try_normalize() {
            let radial = (velocity - hook_velocity).dot(outward);
            if radial < 0.0 {
                let share = (dt / held.brake_left_s).min(1.0);
                velocity -= outward * radial * share;
            }
            let distance = offset.length();
            if distance.is_finite() {
                held.rope
                    .shorten(held.rope.length_m - distance, config.min_length_m);
            }
        }
        held.brake_left_s = (held.brake_left_s - dt).max(0.0);
    }
    let max_tension_n = config.max_accel_g.max(0.0) * GRAVITY_MPS2 * rider_mass_kg;
    let out = held.rope.step(RopeStep {
        a: RopeEnd {
            position: hook,
            velocity: hook_velocity,
            inverse_mass: target_inverse_mass.max(0.0),
        },
        b: RopeEnd {
            position: rider,
            velocity,
            inverse_mass: 1.0 / rider_mass_kg,
        },
        dt,
        tuning: RopeTuning {
            stiffness_per_s2: config.stiffness_per_s2,
            damping_per_s: config.damping_per_s,
            max_tension_n,
            max_stretch_m: config.break_stretch_m,
        },
    })?;
    Ok(SwingOutput {
        velocity: out.b_velocity,
        tension_n: out.tension_n,
        max_tension_n,
        stretch_m: out.stretch_m,
        target_impulse: out.impulse_on_a(dt),
    })
}

/// The grapple hand taking line in: how far it comes back along the aim it had when the lock took
/// hold, tick by tick. Reaching out takes nothing and pays nothing out, so a hand-over-hand pull
/// keeps taking line in.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct TakeIn {
    line: Option<Vec3>,
    previous: Option<f32>,
}

impl TakeIn {
    /// The line taken in over a tick of `dt` seconds with the hand at `hand` (`None` while
    /// untracked, which forgets the hand's place), at most `max_mps`, metres.
    pub(crate) fn step(&mut self, dt: f32, hand: Option<HandLine>, max_mps: f32) -> f32 {
        let Some(hand) = hand else {
            self.previous = None;
            return 0.0;
        };
        if self.line.is_none() && hand.aim.is_finite() {
            self.line = Some(hand.aim);
        }
        let Some(reach) = self.line.and_then(|line| reach_along(hand.grip, line)) else {
            self.previous = None;
            return 0.0;
        };
        let pulled = self.previous.map_or(0.0, |previous| previous - reach);
        self.previous = Some(reach);
        let most = if dt.is_finite() && max_mps.is_finite() {
            (max_mps * dt).max(0.0)
        } else {
            0.0
        };
        if pulled.is_finite() {
            pulled.clamp(0.0, most)
        } else {
            0.0
        }
    }
}
