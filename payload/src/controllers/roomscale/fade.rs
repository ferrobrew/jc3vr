//! The comfort fade: a full-screen darkening of the headset's view, shared by the head-through-
//! geometry probe, the push-back at walls, and the seat transitions. Each source sets a target level; the fade eases toward
//! the darkest target at the configured rates, and the VR blit ([`crate::vr`]) scales the image by
//! `1 - level` as it copies each eye into the swapchain.
//!
//! The engine has its own screen fade (the post-effect fade quad), but the game drives it for its
//! own transitions (loading, teleports, cutscenes), and a second writer would fight them. The blit
//! is the mod's own last stage before the compositor, so darkening there leaves the game's fades
//! alone and never reaches the flat mirror.

use std::sync::atomic::{AtomicU32, Ordering};

use parking_lot::Mutex;

use crate::controllers::config::FadeConfig;

/// Who wants the view darkened.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FadeSource {
    /// The head is inside geometry the body cannot follow it through.
    HeadInGeometry,
    /// A seat transition is hiding an entry or exit animation.
    Seat,
    /// The head has gone past a wall further than the push-back slides the view
    /// ([`crate::controllers::body::rig::root::pushback`]).
    PushBack,
}

impl FadeSource {
    const COUNT: usize = 3;

    const fn index(self) -> usize {
        match self {
            FadeSource::HeadInGeometry => 0,
            FadeSource::Seat => 1,
            FadeSource::PushBack => 2,
        }
    }
}

/// The current darkness, `0` (clear) to `1` (black), for the blit.
pub fn level() -> f32 {
    f32::from_bits(LEVEL.load(Ordering::Relaxed))
}

/// The fade's state, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct FadeStatus {
    pub level: f32,
    /// Each source's target, by [`FadeSource`].
    pub head: f32,
    pub seat: f32,
    pub pushback: f32,
}

pub fn status() -> FadeStatus {
    let targets = FADE.lock().targets;
    FadeStatus {
        level: level(),
        head: targets[FadeSource::HeadInGeometry.index()],
        seat: targets[FadeSource::Seat.index()],
        pushback: targets[FadeSource::PushBack.index()],
    }
}

/// Set a source's target darkness, `0..=1`.
pub fn set_target(source: FadeSource, target: f32) {
    let target = if target.is_finite() {
        target.clamp(0.0, 1.0)
    } else {
        0.0
    };
    FADE.lock().targets[source.index()] = target;
}

/// Advance the fade by `dt` seconds of real time toward the darkest target. Called once per
/// rendered VR frame.
pub fn advance(dt: f32, config: &FadeConfig) {
    let target = FADE.lock().targets.iter().copied().fold(0.0, f32::max);
    let next = step(level(), target, dt, config);
    LEVEL.store(next.to_bits(), Ordering::Relaxed);
}

/// Clear every source and the level at once, when the headset stops rendering.
pub fn reset() {
    FADE.lock().targets = [0.0; FadeSource::COUNT];
    LEVEL.store(0f32.to_bits(), Ordering::Relaxed);
}

/// One step of the easing: toward `target` at `1 / out_s` per second while darkening and
/// `1 / in_s` per second while clearing. A zero duration jumps.
pub fn step(level: f32, target: f32, dt: f32, config: &FadeConfig) -> f32 {
    let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let duration = if target > level {
        config.out_s
    } else {
        config.in_s
    };
    if !(duration.is_finite() && duration > 0.0) {
        return target;
    }
    let max_step = dt / duration;
    level + (target - level).clamp(-max_step, max_step)
}

struct Fade {
    targets: [f32; FadeSource::COUNT],
}

static FADE: Mutex<Fade> = Mutex::new(Fade {
    targets: [0.0; FadeSource::COUNT],
});

/// The level as `f32` bits, read lock-free by the blit.
static LEVEL: AtomicU32 = AtomicU32::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: FadeConfig = FadeConfig {
        out_s: 0.2,
        in_s: 0.4,
    };

    #[test]
    fn the_fade_darkens_and_clears_at_their_own_rates() {
        let darker = step(0.0, 1.0, 0.1, &CONFIG);
        assert!((darker - 0.5).abs() < 1e-5);
        let clearer = step(1.0, 0.0, 0.1, &CONFIG);
        assert!((clearer - 0.75).abs() < 1e-5);
    }

    #[test]
    fn the_fade_stops_at_the_target() {
        assert_eq!(step(0.9, 1.0, 1.0, &CONFIG), 1.0);
        assert_eq!(step(0.1, 0.0, 1.0, &CONFIG), 0.0);
        assert_eq!(step(0.4, 0.4, 0.1, &CONFIG), 0.4);
    }

    #[test]
    fn a_zero_duration_jumps_and_a_bad_dt_holds() {
        let instant = FadeConfig {
            out_s: 0.0,
            in_s: 0.0,
        };
        assert_eq!(step(0.0, 1.0, 0.0, &instant), 1.0);
        assert_eq!(step(0.3, 1.0, f32::NAN, &CONFIG), 0.3);
    }
}
