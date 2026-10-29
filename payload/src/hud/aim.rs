//! The aim depth: the distance from the panel anchor to the point the player aims at, smoothed with
//! a critically-damped exponential, and the aim point itself. Its reader is the atlas's aim part,
//! the cell holding the reticles and the hit marker ([`crate::hud::reticle`]), which is drawn at
//! this depth, toward this point.
//!
//! The source is the game's own aim system: `CHUDUI::UpdateGrappleReticle` projects the smoothed
//! aim position (`CPlayerAimControl`'s `m_AimPos`, lerped) as the *first* world-to-screen call of
//! its frame, every frame the HUD reticle updates -- the base grapple reticle is the general aim
//! cursor, present whether or not the grapple is in use. Only that first call is recorded; the
//! function's later calls (the wire-attachment point and the grip-radius sample) are different
//! points entirely. When no aim point has been recorded recently, the depth eases back to the
//! caller's base distance.

use std::time::Instant;

use glam::Vec3;
use parking_lot::Mutex;

/// The nearest recorded aim depth, in meters.
pub const MIN_DEPTH: f32 = 0.5;

/// The farthest recorded aim depth, in meters: beyond it, disparity is indistinguishable from
/// infinity.
pub const MAX_DEPTH: f32 = 150.0;

/// The distance the aim depth eases back to while nothing has been aimed at recently, in meters.
pub const REST_DISTANCE: f32 = 3.0;

/// Record the aim point (game thread, from the grapple-reticle hook's first call of the frame).
/// `depth` is the distance from the panel anchor to `point`, in meters.
pub fn record(point: Vec3, depth: f32) {
    let mut state = STATE.lock();
    state.point = point;
    state.target = depth;
    state.recorded_at = Some(Instant::now());
}

/// The smoothed aim depth. Eases toward the last recorded aim depth, or toward `base_distance` when
/// the recording has gone stale. Each call advances the smoothing, so one consumer should call it
/// once per frame; readers that only display it use [`smoothed`].
pub fn current(base_distance: f32) -> f32 {
    let mut state = STATE.lock();
    let now = Instant::now();
    let target = if state.fresh() {
        state.target
    } else {
        base_distance
    };
    let dt = state
        .updated_at
        .map(|t| t.elapsed().as_secs_f32())
        .unwrap_or(0.016)
        .min(0.1);
    state.updated_at = Some(now);

    // Holden's critically-damped exponential, like the panel follow.
    let alpha = (1.0 - 2.0_f32.powf(-dt / HALFLIFE_SECONDS)).min(1.0);
    state.smoothed += (target - state.smoothed) * alpha;
    state.smoothed
}

/// The smoothed aim depth as the last [`current`] call left it, without advancing it.
pub fn smoothed() -> f32 {
    STATE.lock().smoothed
}

/// The last recorded aim point, while the recording is fresh.
pub fn point() -> Option<Vec3> {
    let state = STATE.lock();
    state.fresh().then_some(state.point)
}

/// How long a recorded aim point stays authoritative. Beyond this, the smoothed depth eases back to
/// the caller's base distance and [`point`] reports none (the reticle is not targeting anything).
const STALE_AFTER_SECONDS: f32 = 0.5;

/// The smoothing halflife, in seconds: long enough to swallow surface noise as the aim sweeps
/// across geometry, short enough to track a real target change quickly.
const HALFLIFE_SECONDS: f32 = 0.15;

/// The damped aim-depth state.
struct AimState {
    /// The smoothed depth, in meters.
    smoothed: f32,
    /// The most recently recorded aim depth.
    target: f32,
    /// The most recently recorded aim point.
    point: Vec3,
    /// When the aim point was last recorded.
    recorded_at: Option<Instant>,
    /// When the smoothed value was last advanced.
    updated_at: Option<Instant>,
}

impl AimState {
    fn fresh(&self) -> bool {
        self.recorded_at
            .is_some_and(|t| t.elapsed().as_secs_f32() <= STALE_AFTER_SECONDS)
    }
}

static STATE: Mutex<AimState> = Mutex::new(AimState {
    smoothed: REST_DISTANCE,
    target: REST_DISTANCE,
    point: Vec3::ZERO,
    recorded_at: None,
    updated_at: None,
});
