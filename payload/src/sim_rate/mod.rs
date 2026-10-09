//! The sim-rate experiment: run the game's sim tick at 45, 60, 75, or 90 Hz instead of its 30, to
//! measure what a faster tick costs and catalogue what it breaks. It is a debug switch, not a
//! feature: it defaults to 30 Hz, which leaves the game untouched, and only the debug UI's Game tab
//! reaches it. The design, the risks, and the test procedure are in
//! `docs/mod/performance/sim-rate.md`.
//!
//! Above 30 Hz, [`frame_tick`] makes three changes through the hooks' patcher, all of which the
//! patcher reverts when the rate returns to 30 or the payload unloads:
//!
//! - it writes the rate into [`Game::m_UpdateFrequency`], from which the engine derives each tick's
//!   step;
//! - it removes the physics system's pin of its step to the 30 Hz constant (the `movaps` at
//!   [`PhysicsSystem::UPDATE_STEP_PIN_OFFSET`]), so the world steps by the tick's step instead of
//!   running two or three times too fast. The instruction is patched rather than the constant,
//!   which other code shares;
//! - it forces [`Game::m_InterpolationOverride`] to the accumulated method, which runs a frame's
//!   whole ticks (up to the engine's per-frame cap) instead of at most one, so a frame rate below
//!   the tick rate catches up rather than slowing the game down.
//!
//! The mod's own code that assumes the 30 Hz tick reads the step from here: [`world_step_seconds`]
//! for what the physics world advances in one step, [`tick_seconds`] for the sim tick, and
//! [`scale_ticks`] for counts of ticks that stand for a duration. At 30 Hz each returns exactly what
//! the code used before, so the default changes nothing.
//!
//! Threading: [`frame_tick`] runs on the game thread before the game's update; the readers run on
//! the game thread inside it (the input tick, the pre-simulation callbacks). The applied state is a
//! pair of atomics, written only by [`frame_tick`].

mod config;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use jc3gi::{
    game::{Game, get_Game_MaxGameUpdatesPerRender},
    physics::PhysicsSystem,
};

pub(crate) use crate::sim_rate::config::{SimRate, SimRateConfig};
use crate::{config::Config, hooks};

/// Apply the configured rate when it differs from the applied one. Call once per frame on the game
/// thread, before the game's update, so that a change takes effect from that frame's ticks and the
/// pre-simulation callbacks never see the pin and the published step disagree.
pub(crate) fn frame_tick() {
    let wanted = Config::lock_query(|c| c.sim_rate.rate);
    if wanted.hz() == APPLIED_HZ.load(Ordering::Relaxed) {
        return;
    }
    // SAFETY: the game thread, outside the game's update; the singleton is the live game object.
    let Some(game) = (unsafe { Game::get() }) else {
        return;
    };
    let Some(mut patcher) = hooks::patcher() else {
        return;
    };
    let frequency = (&raw mut game.m_UpdateFrequency) as usize;
    let interpolation = (&raw mut game.m_InterpolationOverride) as usize;
    let pin = PhysicsSystem::Update_ADDRESS + PhysicsSystem::UPDATE_STEP_PIN_OFFSET as usize;
    // SAFETY: each address is a live field of the game object or the pin instruction inside
    // `PhysicsSystem::Update`, written on the game thread outside the game's update, when neither
    // is read and the physics step is not running; the patcher keeps the original bytes and
    // restores them on unpatch and on unload.
    unsafe {
        if wanted == SimRate::DEFAULT {
            patcher.unpatch(frequency);
            patcher.unpatch(interpolation);
            patcher.unpatch(pin);
            PIN_REMOVED.store(false, Ordering::Relaxed);
        } else {
            patcher.patch(frequency, &wanted.hz().to_le_bytes());
            let accumulated = Game::INTERPOLATION_METHOD_ACCUMULATED as i32;
            patcher.patch(interpolation, &accumulated.to_le_bytes());
            patcher.patch(pin, &NOPS[..PhysicsSystem::UPDATE_STEP_PIN_LENGTH as usize]);
            PIN_REMOVED.store(true, Ordering::Relaxed);
        }
    }
    let previous = APPLIED_HZ.swap(wanted.hz(), Ordering::Relaxed);
    if wanted == SimRate::DEFAULT {
        tracing::info!(
            target: "sim_rate",
            previous_hz = previous,
            "sim rate: back at the game's 30 Hz, with the physics pin and the frame pacing restored",
        );
    } else {
        tracing::warn!(
            target: "sim_rate",
            hz = wanted.hz(),
            previous_hz = previous,
            "sim rate: the experiment is on: the game ticks at {} Hz, the physics steps by the \
             tick, and the frame pacing accumulates ticks",
            wanted.hz(),
        );
    }
}

/// The sim tick's step, seconds: one over the applied rate, which is what the engine puts in each
/// tick's update context. At 30 Hz it is [`PhysicsSystem::STEP_SECONDS`] exactly.
pub(crate) fn tick_seconds() -> f32 {
    step_for_hz(APPLIED_HZ.load(Ordering::Relaxed))
}

/// The step the physics world takes, seconds. With the pin in place, the world always steps by
/// [`PhysicsSystem::STEP_SECONDS`]. With it removed, the world steps by the time the physics update
/// was given: `given`, when the caller has it (a pre-simulation callback's `dt`) and it is a usable
/// step, and the tick's step otherwise.
pub(crate) fn world_step_seconds(given: Option<f32>) -> f32 {
    world_step(PIN_REMOVED.load(Ordering::Relaxed), given, tick_seconds())
}

/// How many ticks at the applied rate last as long as `ticks_at_30_hz` ticks at the game's 30 Hz,
/// for a count of ticks that stands for a duration. At 30 Hz it is `ticks_at_30_hz`.
pub(crate) fn scale_ticks<T: TickCount>(ticks_at_30_hz: T) -> T {
    T::scale(ticks_at_30_hz, APPLIED_HZ.load(Ordering::Relaxed))
}

/// What the debug UI shows of the experiment.
pub(crate) struct Status {
    /// The rate applied, ticks per second.
    pub(crate) applied_hz: u32,
    /// Whether the physics step's pin is removed.
    pub(crate) pin_removed: bool,
    /// The game's own tick rate field.
    pub(crate) game_hz: Option<u32>,
    /// The game's interpolation override.
    pub(crate) interpolation_override: Option<i32>,
    /// The most ticks the accumulated method runs in a frame.
    pub(crate) max_ticks_per_frame: u32,
}

/// The experiment's state for the debug UI. The game thread only.
pub(crate) fn status() -> Status {
    // SAFETY: the game thread; reads the live game object and a global the engine only reads.
    let (game_hz, interpolation_override, max_ticks_per_frame) = unsafe {
        let game = Game::get();
        (
            game.as_ref().map(|g| g.m_UpdateFrequency),
            game.as_ref().map(|g| g.m_InterpolationOverride),
            *get_Game_MaxGameUpdatesPerRender(),
        )
    };
    Status {
        applied_hz: APPLIED_HZ.load(Ordering::Relaxed),
        pin_removed: PIN_REMOVED.load(Ordering::Relaxed),
        game_hz,
        interpolation_override,
        max_ticks_per_frame,
    }
}

/// A count of ticks [`scale_ticks`] can scale.
pub(crate) trait TickCount: Copy {
    /// `ticks_at_30_hz` scaled to the same duration at `hz`, rounded up so that a count never
    /// shrinks below its duration, and at least one when it was at least one.
    fn scale(ticks_at_30_hz: Self, hz: u32) -> Self;
}

impl TickCount for u32 {
    fn scale(ticks_at_30_hz: Self, hz: u32) -> Self {
        u32::try_from(scale_count(u64::from(ticks_at_30_hz), hz)).unwrap_or(u32::MAX)
    }
}

impl TickCount for u64 {
    fn scale(ticks_at_30_hz: Self, hz: u32) -> Self {
        scale_count(ticks_at_30_hz, hz)
    }
}

/// The rate applied, ticks per second; the game's 30 until [`frame_tick`] applies another.
static APPLIED_HZ: AtomicU32 = AtomicU32::new(SimRate::DEFAULT.hz());

/// Whether the physics step's pin is removed.
static PIN_REMOVED: AtomicBool = AtomicBool::new(false);

/// The bytes that replace the pin: single-byte `nop`s, as many as the pin is long.
const NOPS: [u8; 8] = [0x90; 8];

const _: () = assert!(PhysicsSystem::UPDATE_STEP_PIN_LENGTH as usize <= NOPS.len());

/// One over `hz` in single precision, as the engine computes its step; [`PhysicsSystem::STEP_SECONDS`]
/// for a rate of zero, which the engine raises to one but this never applies.
fn step_for_hz(hz: u32) -> f32 {
    if hz == 0 {
        return PhysicsSystem::STEP_SECONDS;
    }
    1.0 / hz as f32
}

fn world_step(pin_removed: bool, given: Option<f32>, tick_s: f32) -> f32 {
    if !pin_removed {
        return PhysicsSystem::STEP_SECONDS;
    }
    given
        .filter(|dt| dt.is_finite() && *dt > MIN_STEP_S)
        .unwrap_or(tick_s)
}

/// The least time the physics update steps the world for; below it, the world does not step.
const MIN_STEP_S: f32 = 1e-5;

fn scale_count(ticks_at_30_hz: u64, hz: u32) -> u64 {
    let default = u64::from(SimRate::DEFAULT.hz());
    let hz = u64::from(hz.max(1));
    ticks_at_30_hz.saturating_mul(hz).div_ceil(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_rate_steps_by_the_engine_constant() {
        assert_eq!(
            step_for_hz(SimRate::DEFAULT.hz()),
            PhysicsSystem::STEP_SECONDS
        );
        assert_eq!(step_for_hz(0), PhysicsSystem::STEP_SECONDS);
        assert!((step_for_hz(90) - 1.0 / 90.0).abs() < 1e-9);
    }

    #[test]
    fn the_pinned_world_steps_by_the_constant_whatever_it_is_given() {
        let tick = step_for_hz(60);
        assert_eq!(
            world_step(false, Some(tick), tick),
            PhysicsSystem::STEP_SECONDS
        );
        assert_eq!(world_step(false, None, tick), PhysicsSystem::STEP_SECONDS);
    }

    #[test]
    fn the_unpinned_world_steps_by_what_it_is_given() {
        let tick = step_for_hz(90);
        assert_eq!(world_step(true, Some(0.0125), tick), 0.0125);
        assert_eq!(world_step(true, None, tick), tick);
        assert_eq!(world_step(true, Some(0.0), tick), tick);
        assert_eq!(world_step(true, Some(f32::NAN), tick), tick);
    }

    #[test]
    fn counts_keep_their_duration() {
        for n in 0..100u64 {
            assert_eq!(scale_count(n, 30), n);
        }
        assert_eq!(scale_count(2, 60), 4);
        assert_eq!(scale_count(2, 45), 3);
        assert_eq!(scale_count(3, 75), 8);
        assert_eq!(scale_count(90, 90), 270);
        assert_eq!(scale_count(1, 45), 2);
        assert_eq!(scale_count(0, 90), 0);
        assert_eq!(scale_count(u64::MAX, 90), u64::MAX / 30 + 1);
    }

    #[test]
    fn a_scaled_count_never_lasts_less_than_the_original() {
        for rate in SimRate::ALL {
            for n in 1..60u64 {
                let scaled = scale_count(n, rate.hz());
                let original_s = n as f64 / 30.0;
                let scaled_s = scaled as f64 / f64::from(rate.hz());
                assert!(scaled_s >= original_s - 1e-12, "{n} at {}", rate.hz());
                assert!(scaled_s < original_s + 1.0 / f64::from(rate.hz()) + 1e-12);
            }
        }
    }

    #[test]
    fn u32_counts_saturate() {
        assert_eq!(<u32 as TickCount>::scale(u32::MAX, 90), u32::MAX);
        assert_eq!(<u32 as TickCount>::scale(6, 60), 12);
    }
}
