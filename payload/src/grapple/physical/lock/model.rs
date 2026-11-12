use body_rig::smooth::follow_factor;
use glam::{Quat, Vec3};

use crate::{
    controllers::{
        bindings::{
            grapple_button::LockButton,
            yank::{ArmedLine, HandLine},
        },
        physical_hands::physics::drive::rig_jumped,
    },
    grapple::{
        physical::config::{LockConfig, SwingTuning},
        rope::{GRAVITY_MPS2, Rope, RopeEnd, RopeFault, RopeStep, RopeTuning, held_floor},
    },
};

/// Whether a press asks the lock to take hold, given whether it asked last tick (`armed`, cleared
/// when the lock takes hold or refuses) and what the button asks now: a press arms the lock, and
/// the press held keeps it armed, so a press in the reel's wind-up locks the line once the reel
/// carries the character; the button coming up disarms it.
pub(crate) fn armed(armed: bool, button: Option<LockButton>) -> bool {
    match button {
        Some(LockButton::Pressed) => true,
        Some(LockButton::Held) => armed,
        Some(LockButton::Tapped | LockButton::LetGo) | None => false,
    }
}

/// The held line between ticks.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Held {
    pub rope: Rope,
    take_in_mps: f32,
    /// The take's remaining time, seconds: on take, the line turns the character's rush toward the
    /// hook into the swing.
    take_remaining_s: f32,
    /// The speed the take keeps, relative to the hook's point: the character's speed on the take's
    /// first tick, capped so that the swing it starts stays within the line's strength. `None`
    /// until that tick.
    take_speed_mps: Option<f32>,
    /// Whether the take keeps the character's speed, turning it across the line, or stops the
    /// character against the hook's point at once (a hang).
    keeps_speed: bool,
    /// The length the line pays out to, at [`PAY_OUT_MPS`], until the hand takes line in: a hang
    /// lowers the character from the hook to its length.
    pay_out_to_m: Option<f32>,
}

impl Held {
    pub(crate) fn new(hook: Vec3, rider: Vec3, config: &LockConfig) -> Option<Self> {
        let distance = hook.distance(rider);
        if !distance.is_finite() {
            return None;
        }
        Some(Self {
            rope: Rope::new(distance.max(held_floor(config.min_length_m)))?,
            take_in_mps: 0.0,
            take_remaining_s: if config.brake_s.is_finite() {
                config.brake_s.max(0.0)
            } else {
                0.0
            },
            take_speed_mps: None,
            keeps_speed: true,
            pay_out_to_m: None,
        })
    }

    /// A hang: the line held from the character's distance, paid out to `length_m`, and a take that
    /// stops the character against the hook's point rather than turning its rush into a swing.
    pub(crate) fn hang(
        hook: Vec3,
        rider: Vec3,
        length_m: f32,
        config: &LockConfig,
    ) -> Option<Self> {
        let length_m = length_m.is_finite().then_some(length_m)?;
        Some(Self {
            keeps_speed: false,
            pay_out_to_m: Some(length_m.max(held_floor(config.min_length_m))),
            ..Self::new(hook, rider, config)?
        })
    }

    /// Whether the take still turns the character's rush toward the hook into the swing.
    pub(crate) fn braking(&self) -> bool {
        self.take_remaining_s > 0.0
    }

    /// Take `taken_m` of line in, down to `config`'s least length.
    pub(crate) fn take_in(&mut self, taken_m: f32, dt: f32, config: &LockConfig) {
        self.take_in_mps = 0.0;
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let before = self.rope.length_m;
        self.rope.shorten(taken_m, config.min_length_m);
        self.take_in_mps = ((before - self.rope.length_m) / dt).min(config.reel_in_mps.max(0.0));
        if self.take_in_mps > 0.0 {
            self.pay_out_to_m = None;
        }
    }

    /// Pay line out toward the hang's length over a tick of `dt` seconds.
    fn pay_out(&mut self, dt: f32) {
        let Some(target) = self.pay_out_to_m else {
            return;
        };
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.rope.length_m =
            (self.rope.length_m + PAY_OUT_MPS * dt).min(target.max(self.rope.length_m));
        if self.rope.length_m >= target {
            self.pay_out_to_m = None;
        }
    }

    /// Turn the rush toward the hook into the swing over the take's remaining time, keeping the
    /// character's speed: the speed toward the hook's point, measured against the hook's own
    /// motion, falls linearly to zero as the take ends, and the speed across the line rises so
    /// that the whole speed stays what it was on the take's first tick ([`take_speed`]). The swing
    /// runs the way the character was going across the line, or, for a rush straight along it,
    /// the way it was going over the ground, and failing that downward. The held length follows
    /// the character in, so the rush does not leave the line slack.
    fn take(
        &mut self,
        velocity: &mut Vec3,
        rider: Vec3,
        hook: Vec3,
        hook_velocity: Vec3,
        dt: f32,
        config: &LockConfig,
    ) {
        if self.take_remaining_s <= 0.0 || !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let offset = rider - hook;
        let distance = offset.length();
        let Some(direction) = offset.try_normalize() else {
            self.take_remaining_s = 0.0;
            return;
        };
        let relative = *velocity - hook_velocity;
        if !self.keeps_speed {
            // A hang starts at rest against the hook, as the game's own ceiling attach clears the
            // character's velocities.
            *velocity = hook_velocity;
            self.take_remaining_s = 0.0;
            return;
        }
        let kept = *self
            .take_speed_mps
            .get_or_insert_with(|| take_speed(relative.length(), direction, distance, config));
        let step_s = dt.min(self.take_remaining_s);
        let along = relative.dot(direction);
        if along < 0.0 {
            let radial = along * (1.0 - (step_s / self.take_remaining_s).clamp(0.0, 1.0));
            let across = relative - direction * along;
            let swing = swing_direction(across, relative, direction);
            let across_speed = match swing {
                Some(_) => across
                    .length()
                    .max((kept * kept - radial * radial).max(0.0).sqrt()),
                None => across.length(),
            };
            let across = swing.map_or(across, |swing| swing * across_speed);
            *velocity = hook_velocity + across + direction * radial;
        }
        if distance < self.rope.length_m {
            self.rope
                .shorten(self.rope.length_m - distance, config.min_length_m);
        }
        self.take_remaining_s = (self.take_remaining_s - step_s).max(0.0);
    }
}

/// The speed the take keeps for a character moving at `speed_mps` relative to the hook's point,
/// `direction` from the hook's point toward it at `distance_m`: as much of the speed as the line,
/// at its strength, can hold at the bottom of the swing it starts, after the drop to there. A
/// pendulum's line pulls hardest at the bottom, with gravity and the speed gained falling to it
/// added to the turn, so the entry's speed is bounded by what is left of the line's strength.
fn take_speed(speed_mps: f32, direction: Vec3, distance_m: f32, config: &LockConfig) -> f32 {
    let drop_share = 1.0 + direction.y.clamp(-1.0, 1.0);
    let spare_g = config.max_accel_g.max(0.0) - 1.0 - 2.0 * drop_share;
    let most = (spare_g.max(0.0) * GRAVITY_MPS2 * distance_m.max(0.0)).sqrt();
    let speed = if speed_mps.is_finite() {
        speed_mps
    } else {
        0.0
    };
    speed.min(most)
}

/// The direction across the line the take swings the character in, `direction` pointing from the
/// hook's point to it: the way its relative velocity already goes across the line, or for a
/// velocity along the line, the way it goes over the ground, and failing that downward, each
/// taken across the line. `None` for a line straight up or down with no way across.
fn swing_direction(across: Vec3, relative: Vec3, direction: Vec3) -> Option<Vec3> {
    let across_line = |v: Vec3| (v - direction * v.dot(direction)).try_normalize();
    let ground = Vec3::new(relative.x, 0.0, relative.z);
    if across.length() > ACROSS_MIN_MPS {
        return across.try_normalize();
    }
    across_line(ground)
        .filter(|_| ground.length() > ACROSS_MIN_MPS)
        .or_else(|| across_line(Vec3::NEG_Y))
        .filter(|d| d.is_finite())
}

/// How fast a hang pays line out to its length, metres per second.
const PAY_OUT_MPS: f32 = 3.0;

/// Below this speed across the line, metres per second, the take finds the swing's way from the
/// ground track or gravity rather than from the velocity's own crossing.
const ACROSS_MIN_MPS: f32 = 0.5;

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
    held.take(&mut velocity, rider, hook, hook_velocity, dt, config);
    held.pay_out(dt);
    let take_in_velocity = (rider - hook).normalize_or_zero() * held.take_in_mps;
    let max_tension_n = config.max_accel_g.max(0.0) * GRAVITY_MPS2 * rider_mass_kg;
    let out = held.rope.step(RopeStep {
        a: RopeEnd {
            position: hook,
            velocity: hook_velocity,
            inverse_mass: target_inverse_mass.max(0.0),
        },
        b: RopeEnd {
            position: rider,
            velocity: velocity + take_in_velocity,
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
        velocity: out.b_velocity - take_in_velocity,
        tension_n: out.tension_n,
        max_tension_n,
        stretch_m: out.stretch_m,
        target_impulse: out.impulse_on_a(dt),
    })
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct PumpSample {
    pub hand: Vec3,
    pub head: Vec3,
    pub rotation: Quat,
    pub world_scale: f32,
    pub recenters: u32,
}

#[derive(Debug, Copy, Clone, Default)]
pub(crate) struct Pump {
    previous: Option<PumpSample>,
    velocity: Vec3,
    remaining_s: f32,
}

impl Pump {
    pub(crate) fn sample(&mut self, sample: Option<PumpSample>, dt: f32, tuning: &SwingTuning) {
        self.remaining_s = 0.0;
        let Some(sample) = sample.filter(|s| {
            s.hand.is_finite()
                && s.head.is_finite()
                && s.rotation.is_finite()
                && s.rotation.is_normalized()
                && s.world_scale.is_finite()
                && s.world_scale > 0.0
                && dt.is_finite()
                && dt > 0.0
                && dt <= 0.1
                && tuning.pump
        }) else {
            *self = Self::default();
            return;
        };
        let previous = self.previous.replace(sample);
        let Some(previous) = previous else { return };
        if rig_jumped(
            Some((previous.rotation, previous.recenters)),
            sample.rotation,
            sample.recenters,
        ) || (previous.world_scale - sample.world_scale).abs() > 1e-4
        {
            self.velocity = Vec3::ZERO;
            return;
        }
        let displacement = (sample.hand - sample.head) - (previous.hand - previous.head);
        let velocity = sample.rotation * (displacement * sample.world_scale / dt);
        if displacement.length() > 0.4 || !velocity.is_finite() || velocity.length() > 8.0 {
            self.velocity = Vec3::ZERO;
            return;
        }
        self.velocity = self
            .velocity
            .lerp(velocity, follow_factor(dt, tuning.smoothing_s));
        self.remaining_s = dt;
    }

    pub(crate) fn take(
        &mut self,
        dt: f32,
        offset: Vec3,
        length_m: f32,
        tuning: &SwingTuning,
    ) -> Vec3 {
        if !dt.is_finite() || dt <= 0.0 || !offset.is_finite() || !length_m.is_finite() {
            return Vec3::ZERO;
        }
        let seconds = dt.min(self.remaining_s);
        self.remaining_s = (self.remaining_s - seconds).max(0.0);
        let distance = offset.length();
        let Some(radial) = offset.try_normalize() else {
            return Vec3::ZERO;
        };
        let tangent = self.velocity - radial * self.velocity.dot(radial);
        let speed = tangent.length();
        let strength = ((speed - tuning.deadzone_mps)
            / (tuning.full_mps - tuning.deadzone_mps).max(1e-3))
        .clamp(0.0, 1.0);
        let tautness = ((distance - length_m) / 0.05).clamp(0.0, 1.0);
        let acceleration =
            -tangent.normalize_or_zero() * strength * tautness * (0.35 * GRAVITY_MPS2);
        acceleration * seconds
    }

    pub(crate) fn expire(&mut self) {
        *self = Self::default();
    }
}

/// The grapple hand taking line in: how far it comes back along the aim it had when the lock took
/// hold, tick by tick. Reaching out takes nothing and pays nothing out, so a hand-over-hand pull
/// keeps taking line in.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct TakeIn {
    line: ArmedLine,
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
        let Some(reach) = self.line.reach(Some(hand)) else {
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
