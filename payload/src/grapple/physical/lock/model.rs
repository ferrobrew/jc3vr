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
    /// Whether the take still runs: the character coasts in toward the hook on its own momentum
    /// and the held length follows it, until it stops closing on the hook.
    coasting: bool,
    /// Whether the take keeps the character's motion, letting the line go taut where its approach
    /// ends, or stops the character against the hook's point at once (a hang).
    keeps_speed: bool,
    /// The length the line pays out to, at [`PAY_OUT_MPS`], until the hand takes line in: a hang
    /// lowers the character from the hook to its length.
    pay_out_to_m: Option<f32>,
    /// The line the hand has taken in since the pay-out last ran, metres: a pull past
    /// [`PAY_OUT_TAKE_IN_M`] ends the pay-out, and a hand's drift does not.
    taken_since_pay_out_m: f32,
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
            coasting: true,
            keeps_speed: true,
            pay_out_to_m: None,
            taken_since_pay_out_m: 0.0,
        })
    }

    /// A hang: the line held from the character's distance, paid out to `length_m`, and a take that
    /// stops the character against the hook's point rather than letting it coast.
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

    /// Whether the take still runs: the character still closes on the hook with the line
    /// following it in.
    pub(crate) fn coasting(&self) -> bool {
        self.coasting
    }

    /// Take `taken_m` of line in, down to `config`'s least length.
    pub(crate) fn take_in(&mut self, taken_m: f32, dt: f32, config: &LockConfig) {
        self.take_in_mps = 0.0;
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let before = self.rope.length_m;
        self.rope.shorten(taken_m, config.min_length_m);
        let taken = before - self.rope.length_m;
        self.take_in_mps = (taken / dt).min(config.reel_in_mps.max(0.0));
        if self.pay_out_to_m.is_some() && taken > 0.0 {
            self.taken_since_pay_out_m += taken;
            if self.taken_since_pay_out_m > PAY_OUT_TAKE_IN_M {
                self.pay_out_to_m = None;
            }
        }
    }

    /// Pay line out toward the hang's length over a tick of `dt` seconds, or take it in toward it
    /// for a hang that took hold further out (under a helicopter, at the reel's distance).
    fn pay_out(&mut self, dt: f32, min_length_m: f32) {
        let Some(target) = self.pay_out_to_m else {
            return;
        };
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        // A hand's drift back and forth along the aim adds up its pulls; the leak lets it go.
        self.taken_since_pay_out_m = (self.taken_since_pay_out_m - PAY_OUT_TAKE_IN_M * dt).max(0.0);
        if self.rope.length_m > target {
            let taken = (self.rope.length_m - target).min(PAY_OUT_MPS * dt);
            self.rope.shorten(taken, min_length_m);
            if self.rope.length_m <= target + 1e-4 {
                self.pay_out_to_m = None;
            }
            return;
        }
        self.rope.length_m = (self.rope.length_m + PAY_OUT_MPS * dt).min(target);
        if self.rope.length_m >= target {
            self.pay_out_to_m = None;
        }
    }

    /// Damp a hang's swing across the line toward the hook's own motion over a tick of `dt`
    /// seconds, while the hook moves (an airframe): the rope damps only along itself, and the
    /// airframe's turns keep pumping a pendulum across it. A hang from a still hook swings freely,
    /// for the arm to pump.
    fn damp_hang(
        &self,
        velocity: &mut Vec3,
        rider: Vec3,
        hook: Vec3,
        hook_velocity: Vec3,
        dt: f32,
    ) {
        if self.keeps_speed
            || !dt.is_finite()
            || dt <= 0.0
            || hook_velocity.length().is_nan()
            || hook_velocity.length() <= MOVING_HOOK_MPS
        {
            return;
        }
        let Some(direction) = (rider - hook).try_normalize() else {
            return;
        };
        let relative = *velocity - hook_velocity;
        let across = relative - direction * relative.dot(direction);
        *velocity -= across * follow_factor(dt, HANG_SWING_DAMPING_S);
    }

    /// The take: while the character still closes on the hook's point (measured against the hook's
    /// own motion), it coasts on its own momentum and the held length follows it in, so the line
    /// goes taut exactly when its approach ends, with no speed along the line left to stop and
    /// nothing across the line turned. From there the rope swings it. Only a coast that would
    /// carry the character in past the shortest length the line can swing it on (or past its
    /// least length) is braked, along the line alone, to stop there. A hang instead starts at
    /// rest against the hook.
    fn take(
        &mut self,
        velocity: &mut Vec3,
        rider: Vec3,
        hook: Vec3,
        hook_velocity: Vec3,
        dt: f32,
        config: &LockConfig,
    ) {
        if !self.coasting || !dt.is_finite() || dt <= 0.0 {
            return;
        }
        if !self.keeps_speed {
            // A hang starts at rest against the hook, as the game's own ceiling attach clears the
            // character's velocities.
            *velocity = hook_velocity;
            self.coasting = false;
            return;
        }
        let offset = rider - hook;
        let distance = offset.length();
        let Some(direction) = offset.try_normalize() else {
            self.coasting = false;
            return;
        };
        let relative = *velocity - hook_velocity;
        let along = relative.dot(direction);
        if along >= 0.0 {
            self.coasting = false;
            return;
        }
        // The line held at a length must turn the speed across it, with gravity along it at the
        // swing's bottom. The speed across grows as the character closes in, keeping its momentum
        // about the hook, so the shortest length the line bears the swing at is where that
        // growth meets the line's strength. The coast stops short of it, as of the least length:
        // a shorter line would only break.
        let across = (relative - direction * along).length();
        let bearable = (config.max_accel_g * COAST_STRENGTH_SHARE - 1.0) * GRAVITY_MPS2;
        let shortest = if bearable > 0.0 {
            ((across * distance).powi(2) / bearable).cbrt()
        } else {
            f32::INFINITY
        };
        if shortest >= distance {
            self.coasting = false;
            return;
        }
        let room = distance - shortest.max(held_floor(config.min_length_m));
        let stopping = if room > 0.0 {
            along * along / (2.0 * room)
        } else {
            f32::INFINITY
        };
        if stopping > COAST_BRAKE_MPS2 {
            let braked = (along + stopping.min(along.abs() / dt) * dt).min(0.0);
            *velocity += direction * (braked - along);
        }
        if distance < self.rope.length_m {
            self.rope
                .shorten(self.rope.length_m - distance, config.min_length_m);
        }
    }
}

/// The deceleration toward the hook beyond which the coast is braked, metres per second squared:
/// about what the line holds, so a coast that would otherwise carry in too far stops at the
/// shortest length the line bears instead.
const COAST_BRAKE_MPS2: f32 = 3.0 * GRAVITY_MPS2;

/// The share of the line's strength a coast leaves the swing to turn on the length it reaches:
/// the rest is the margin for the pump and the hook's motion.
const COAST_STRENGTH_SHARE: f32 = 0.8;

/// How much line the hand must take in during a hang's pay-out to end it, metres: a deliberate
/// pull, past a hand's drift back along the aim.
const PAY_OUT_TAKE_IN_M: f32 = 0.05;

/// How long a hang's swing across the line under a moving hook takes to die away, seconds: the
/// time constant of its damping.
const HANG_SWING_DAMPING_S: f32 = 1.0;

/// The hook speed above which a hang counts as hung from a moving body, metres per second.
const MOVING_HOOK_MPS: f32 = 0.5;

/// How fast a hang pays line out to its length, or takes it in, metres per second.
const PAY_OUT_MPS: f32 = 3.0;

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
    held.damp_hang(&mut velocity, rider, hook, hook_velocity, dt);
    held.pay_out(dt, config.min_length_m);
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

/// The free hand taking line in while it holds the line ahead of the grapple hand
/// ([`crate::grapple::line_grab`]): a [`TakeIn`] of its own along the line's direction, started
/// afresh at each hold, so a hand that lets go, reaches out, and takes hold again keeps taking line
/// in.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct FreeHand {
    take_in: TakeIn,
    held: bool,
}

impl FreeHand {
    pub(crate) fn held(&self) -> bool {
        self.held
    }

    /// The line the free hand at `hand` (`None` while it does not hold the line) takes in over a
    /// tick of `dt` seconds, at most `max_mps`, metres, and whether it took hold or let go this
    /// tick.
    pub(crate) fn step(&mut self, dt: f32, hand: Option<HandLine>, max_mps: f32) -> (f32, bool) {
        let held = hand.is_some();
        let changed = held != self.held;
        if changed {
            *self = Self {
                take_in: TakeIn::default(),
                held,
            };
        }
        (self.take_in.step(dt, hand, max_mps), changed)
    }
}
