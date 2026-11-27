//! Grapple comfort: keeps the grapple from throwing the VR view around (issue #36).
//!
//! The player aims the grapple by looking at the target, so the head pose already carries the
//! rotation toward it. The engine then rotates the character's root toward that same target —
//! first the fire act's directional alignment, then `NReeledInController::RotateToGrappleTarget`
//! during the reel — and the headpose composes the head onto the body's full world rotation, so
//! the target rotation is applied twice and the view swings violently past whatever was being
//! looked at. In VR that is a rapid rotation the inner ear never felt: a strong vestibular
//! conflict in the game's most-used traversal mechanic.
//!
//! This module filters the *body-driven* inputs to the headpose composition while the grapple owns
//! the character. The HMD's own tracking passes through untouched, in both rotation and position.
//! Three cooperating pieces:
//!
//! - **The body-frame hold** ([`filter_body_rotation`]): while the grapple is active, the body
//!   rotation the head composes onto is blended toward a filtered frame per
//!   [`GrappleComfortMode`] — by default the frame the previous render composed with, held from
//!   fire to landing so the view stays world-stable. The hold engages the moment a fire is
//!   committed (the fire act whips the body toward the target before the wire even spawns) and
//!   releases by blending the accumulated rotation back over the tuning's
//!   [`release_s`](tuning::ComfortTuning::release_s).
//! - **The yaw handoff**: an on-foot landing would otherwise return 10–25° of accumulated yaw to
//!   the view through the release blend. Instead the held heading is posted for the VR body-turn
//!   accumulator ([`take_body_yaw_retarget`]), the character turns to face where the player is
//!   looking via the game's own rate-limited turn machinery, and the hold stays engaged until the
//!   heading converges — the view never sweeps through the yaw.
//! - **The landing-snap absorber** ([`filter_anchor`]): the attach/landing animation teleports the
//!   head anchor up to a metre against the root in one sim tick. A tick's change in the anchor's
//!   offset from the root beyond the tuning's
//!   [`anchor_snap_m`](tuning::ComfortTuning::anchor_snap_m) is absorbed into an offset that
//!   decays out of the view; the root's own motion, at any speed, passes through 1:1, and the filtered anchor is
//!   always `raw + offset`, so the engine's tick interpolation is undisturbed.
//!
//! The filter advances on the engine's input tick and again on every rendered VR frame
//! ([`advance`]): the body starts rotating the instant the grapple acts, and a tick-only (~33 Hz)
//! state read leaves up to a tick of that rotation in the view before the filter can engage.
//!
//! The filtered rotation's yaw does not track the real body while the hold is engaged, so
//! consumers that steer the actual character (the VR body-turn accumulator) read the raw rotation
//! ([`jc3vr_view::headpose::xr::body_rotation_raw`]), never the filtered one. Lock order across the
//! headpose is SIM → BODY_YAW → FILTER: this module's lock is innermost, so nothing under it may
//! call back into a locking `headpose` accessor.
//!
//! [`telemetry`] captures the filter's inputs and outputs to a CSV for offline analysis; every
//! behaviour above was diagnosed and verified through it.

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use body_rig::math::{
    angle::{heading, wrap},
    smooth::follow_factor,
};
use glam::{Quat, Vec3};
use jc3gi::equipment::grappling_hook::GrapplingHookState;
use jc3vr_core::published::{Stamp, Window};
use parking_lot::Mutex;

pub mod device;
pub mod line_grab;
pub mod physical;
pub(crate) mod rope;
pub mod seat;
pub mod telemetry;
pub mod tuning;

use jc3vr_body::controllers::physics::{grapple_hook as hook, grapple_hook::HookRead};
use jc3vr_view::{
    headpose,
    headpose::{HeadPoseConfig, config::GrappleComfortMode},
};

use crate::grapple::tuning::GRAPPLE;

/// Advance the filter one step: read the hook's phase, run the engage/hold/handoff/release state
/// machine, and update the anchor absorber. `body` is the raw body rotation as of this advance,
/// and `rooted` the head anchor's offset from the character's root (`None` without anchors);
/// `on_foot` gates the yaw handoff, and the head pose settings' `grapple` section picks the mode.
///
/// Called from both cadences — the input tick ([`jc3vr_view::headpose::sim::on_input_tick`], the poll
/// stage, in the engine's input job in play) and the VR frame loop (render thread) — under one
/// mutex, with the step `dt` measured internally. `on_foot` arrives as a parameter because the
/// input-tick caller already holds the sim lock, and reading `sim::mode()` here would deadlock (SIM
/// → FILTER is the only legal lock order).
pub fn advance(body: Quat, rooted: Option<Vec3>, on_foot: bool, config: &HeadPoseConfig) {
    let config = &config.grapple;
    let tuning = &GRAPPLE.get().comfort;
    let mut s = FILTER.lock();
    let now = Stamp::now();
    let dt = s
        .last_advance
        .map(|last| now.saturating_duration_since(last).as_secs_f32())
        .unwrap_or(0.0)
        .clamp(0.0, 0.1);
    s.last_advance = Some(now);

    let (reeling, engaged_other) = reel_flags();
    let active = config.mode != GrappleComfortMode::Off && (reeling || engaged_other);

    // Reel end, on foot: start the yaw handoff. The body-turn accumulator is aimed at the held
    // view heading, and the hold stays engaged while the game's own turn machinery brings the
    // body around; the release blend then only carries the small pitch/roll residual. Airborne
    // releases (jumping off mid-reel into a fall or parachute) skip it: the accumulator only
    // steers on foot, so the hold would pin the view against a flying world for the full timeout.
    if s.was_active
        && !active
        && on_foot
        && config.mode == GrappleComfortMode::HoldView
        && headpose::source() == headpose::Source::Vr
    {
        s.pending_retarget = Some((heading(s.held), now));
        s.handoff_deadline = Some(now + Duration::from_secs_f32(tuning.handoff_timeout_s));
    }
    s.was_active = active;

    // The handoff ends when the body's heading converges on the held one, at the deadline (the
    // body can be blocked from turning), when the character leaves its feet, or when a new reel
    // starts.
    if let Some(deadline) = s.handoff_deadline {
        let dyaw = wrap(heading(body) - heading(s.held)).abs();
        if active
            || !on_foot
            || now >= deadline
            || dyaw < tuning.handoff_converged_deg * std::f32::consts::PI / 180.0
        {
            s.handoff_deadline = None;
        }
    }
    let hold = active || s.handoff_deadline.is_some();

    // On the engage edge, hold the frame the view *last composed*: the previous advance's body
    // under the current blend. The previous advance's body, not this one's, because the body
    // snaps toward the fire direction in the same step the grapple state flips — the current body
    // already carries rotation that would jump the view at engage. Under the current blend, not
    // raw, because a re-grapple during the release tail must hold the partially blended frame the
    // player is already looking through. Seamless at any blend level.
    if hold && !s.was_hold {
        let pre_snap = s.prev_body.unwrap_or(body);
        s.held = filter_with(pre_snap, s.mode, s.held, s.factor);
    }
    s.was_hold = hold;
    s.prev_body = Some(body);
    s.mode = config.mode;

    let target = if hold { 1.0 } else { 0.0 };
    let tau = if target > s.factor {
        ENGAGE_S
    } else {
        tuning.release_s
    };
    s.factor = s.factor + (target - s.factor) * follow_factor(dt, tau);
    // The release decay is exponential and never truly reaches zero; snap the tail off so the
    // filter (and the absorber's arming) fully disarms a couple of seconds after a release.
    if !hold && s.factor < FACTOR_DISARM {
        s.factor = 0.0;
    }
    FACTOR_BITS.store(s.factor.to_bits(), Ordering::Relaxed);

    advance_absorber(&mut s, rooted, reeling, dt);
}

/// Apply the body-frame hold to a body rotation. At blend `0` the rotation passes through
/// untouched; at `1` the head composes onto the fully filtered frame.
pub(crate) fn filter_body_rotation(body: Quat) -> Quat {
    let s = FILTER.lock();
    filter_with(body, s.mode, s.held, s.factor)
}

/// Apply the landing-snap absorber to the body-driven head anchor: the raw anchor plus the
/// decaying absorbed offset (zero outside a snap). Always `raw + offset`, so pose-pair deltas
/// built from it keep their exact tick spacing.
pub fn filter_anchor(anchor: Vec3) -> Vec3 {
    anchor + FILTER.lock().anchor_offset
}

/// The current blend factor in `[0, 1]`, for UI display.
pub fn blend_factor() -> f32 {
    f32::from_bits(FACTOR_BITS.load(Ordering::Relaxed))
}

/// Take the pending yaw-handoff heading (world radians about +Y), if one is still fresh. Consumed
/// by the VR body-yaw accumulator on its on-foot ticks: the accumulator adopts it as the turn
/// target, so the game's own turn machinery brings the body around to the held view heading. A
/// landing may be a few ticks away from the release that requested it (the landing transition has
/// to settle before the character counts as on foot), so the request waits; it expires after a
/// short window so one that is never consumed — a reel that ends into a vehicle — cannot steer a
/// much later on-foot tick.
pub fn take_body_yaw_retarget() -> Option<f32> {
    let mut s = FILTER.lock();
    let (yaw, at) = s.pending_retarget.take()?;
    let window = Window::new(Duration::from_secs_f32(
        GRAPPLE.get().comfort.retarget_fresh_s,
    ));
    window.holds(at, Stamp::now()).then_some(yaw)
}

/// The landing-snap absorber step (see the module docs). The detection runs on the anchor's offset
/// from the root, which the root's own motion does not change: measured against the world, a fast
/// flight's ticks of uneven length read as velocity changes, and the absorber pulled the view a
/// few tenths of a metre behind the body after every reel. The offset changes only when a new
/// tick's anchors are published, so per-advance steps are zero between ticks.
fn advance_absorber(s: &mut GrappleFilter, rooted: Option<Vec3>, reeling: bool, dt: f32) {
    // Armed only from the attach/landing onward, never during the zip: the only positional snap
    // the grapple produces is at the landing.
    let armed = s.factor > f32::EPSILON && !reeling;
    if let (Some(previous), Some(rooted)) = (s.prev_rooted, rooted) {
        s.anchor_offset = absorb_step(s.anchor_offset, rooted - previous, armed);
    }
    s.prev_rooted = rooted;
    s.anchor_offset *= (-dt / GRAPPLE.get().comfort.anchor_snap_ease_s).exp();
}

/// The absorbed offset after the anchor's offset from the root moved by `step` in one advance:
/// while `armed`, a step beyond [`anchor_snap_m`](tuning::ComfortTuning::anchor_snap_m) is taken
/// out of the view, and anything else passes. A step beyond
/// [`anchor_teleport_m`](tuning::ComfortTuning::anchor_teleport_m) is no animation and resets the
/// offset.
fn absorb_step(offset: Vec3, step: Vec3, armed: bool) -> Vec3 {
    let tuning = &GRAPPLE.get().comfort;
    let length = step.length();
    if length > tuning.anchor_teleport_m {
        Vec3::ZERO
    } else if armed && length > tuning.anchor_snap_m {
        // Hard-capped so a mis-detection degrades into a bounded, decaying lag, never a runaway
        // detach from the body.
        (offset - step).clamp_length_max(tuning.anchor_offset_max_m)
    } else {
        offset
    }
}

/// The blend-in time constant (seconds) when the filter engages. The filter holds the frame the
/// previous render composed with, so an instant engage is seamless; a ramp here would only let the
/// grapple's opening rotation leak through.
const ENGAGE_S: f32 = 0.0;

/// The blend below which a releasing filter snaps to fully disarmed (its rotational effect is long
/// imperceptible there).
const FACTOR_DISARM: f32 = 1e-3;

/// The filter's state: written by [`advance`] in the input tick's poll stage (the engine's
/// input job in play) and the render thread's frame loop, read per-eye on the render thread.
struct GrappleFilter {
    /// The smoothed blend factor in `[0, 1]`.
    factor: f32,
    /// The held body frame: captured on the engage edge, frozen through the hold, blended out on
    /// release.
    held: Quat,
    /// The mode as of the last advance, so [`filter_body_rotation`] needs no config access.
    mode: GrappleComfortMode,
    /// The previous advance's instant, for the blend step `dt` (advances arrive on two cadences).
    last_advance: Option<Stamp>,
    /// Whether the previous advance saw the grapple active (release-edge detection for the
    /// handoff).
    was_active: bool,
    /// Whether the previous advance held (engage-edge detection for the held-frame capture; the
    /// hold outlasts `was_active` through the handoff).
    was_hold: bool,
    /// While `Some`, the post-reel yaw handoff is running: the hold stays engaged and the body
    /// turns toward the held heading.
    handoff_deadline: Option<Stamp>,
    /// The heading the yaw handoff wants the body turned toward, awaiting consumption by the VR
    /// body-yaw accumulator (see [`take_body_yaw_retarget`]).
    pending_retarget: Option<(f32, Stamp)>,
    /// The body rotation as of the previous advance — the frame the last render composed with —
    /// captured into the hold on the engage edge (the current advance's body already carries the
    /// grapple's fire-snap).
    prev_body: Option<Quat>,
    /// The anchor's offset from the root as of the previous advance, for the snap detection.
    prev_rooted: Option<Vec3>,
    /// The absorbed landing-snap offset, decaying toward zero (see [`filter_anchor`]).
    anchor_offset: Vec3,
}

static FILTER: Mutex<GrappleFilter> = Mutex::new(GrappleFilter {
    factor: 0.0,
    held: Quat::IDENTITY,
    mode: GrappleComfortMode::Off,
    last_advance: None,
    was_active: false,
    was_hold: false,
    handoff_deadline: None,
    pending_retarget: None,
    prev_body: None,
    prev_rooted: None,
    anchor_offset: Vec3::ZERO,
});

/// [`GrappleFilter::factor`]'s `f32` bits, mirrored for the lock-free UI readout.
static FACTOR_BITS: AtomicU32 = AtomicU32::new(0);

/// A snapshot of the filter's blend state, for [`telemetry`].
fn filter_snapshot() -> (GrappleComfortMode, f32, Quat) {
    let s = FILTER.lock();
    (s.mode, s.factor, s.held)
}

/// The pure filter: blend `body` toward the mode's filtered frame by `factor`.
fn filter_with(body: Quat, mode: GrappleComfortMode, held: Quat, factor: f32) -> Quat {
    if factor <= f32::EPSILON {
        return body;
    }
    let filtered = match mode {
        GrappleComfortMode::Off => return body,
        GrappleComfortMode::HoldView => held,
        GrappleComfortMode::LevelPitch => Quat::from_rotation_y(heading(body)),
    };
    body.slerp(filtered, factor.min(1.0)).normalize()
}

/// Read the local player's grappling hook, or `None` when there is no local character or hook. A
/// plain field walk, safe on both advance cadences.
fn hook_snapshot() -> Option<HookRead> {
    // SAFETY: the hook's plain fields, read where the game does not free it.
    unsafe { hook::local_hook() }.map(|hook| HookRead::of(hook))
}

/// The local player's grapple phase as `(reeling, engaged_other)`: actively zipping
/// (`GHS_REELING_IN`), or otherwise in a phase the hold must cover:
///
/// - **Fire committed** (`m_WaitingFor*Fire`): the fire act's directional alignment whips the body
///   toward the target before the wire even spawns — measured up to 130° at ~1000°/s — and the
///   player aimed by looking there, so it is the same double-count as the reel itself.
/// - **Hook in flight** (`GHS_INACTIVE` with a live wire): bridges the gap between the fire act
///   and `GHS_REELING_IN`, so the hold never flickers between the two.
/// - **Reeled attachments**, gated on the active wire still existing: the hook's state records
///   the *last* reel outcome as much as the current one (leaving an attachment does not reliably
///   return it to `GHS_INACTIVE`; observed parked at `GHS_REELED_ATTACHED` for minutes of
///   ordinary play), while the wire is dropped when the attachment genuinely ends.
fn reel_flags() -> (bool, bool) {
    let Some(hook) = hook_snapshot() else {
        return (false, false);
    };
    match hook.state {
        GrapplingHookState::GHS_REELING_IN => (true, false),
        GrapplingHookState::GHS_REELED_ATTACHED
        | GrapplingHookState::GHS_REELED_HANG
        | GrapplingHookState::GHS_REELED_UPSIDEDOWN
        | GrapplingHookState::GHS_REELED_STUNT => (false, hook.active_wire || hook.firing),
        GrapplingHookState::GHS_INACTIVE => (false, hook.active_wire || hook.firing),
        _ => (false, hook.firing),
    }
}

#[cfg(test)]
mod tests {
    use glam::EulerRot;

    use super::*;

    /// At full blend, hold-view replaces the live body frame with the held one: the composed view
    /// stays wherever it was at reel start, no matter how the body rotates.
    #[test]
    fn hold_view_pins_to_held_frame() {
        let held = Quat::from_euler(EulerRot::YXZ, 0.3, 0.0, 0.0);
        let body = Quat::from_euler(EulerRot::YXZ, 1.2, 0.9, 0.0);
        let filtered = filter_with(body, GrappleComfortMode::HoldView, held, 1.0);
        assert!((filtered.dot(held).abs() - 1.0).abs() < 1e-6);
    }

    /// At full blend, level-pitch flattens a pitched body to its yaw alone.
    #[test]
    fn level_pitch_keeps_yaw_drops_pitch() {
        let body = Quat::from_euler(EulerRot::YXZ, 1.0, 0.8, 0.0);
        let filtered = filter_with(body, GrappleComfortMode::LevelPitch, Quat::IDENTITY, 1.0);
        let (yaw, pitch, roll) = filtered.to_euler(EulerRot::YXZ);
        assert!((yaw - 1.0).abs() < 1e-5);
        assert!(pitch.abs() < 1e-5);
        assert!(roll.abs() < 1e-5);
    }

    /// At zero blend the body rotation passes through untouched in every mode.
    #[test]
    fn zero_blend_is_identity() {
        let body = Quat::from_euler(EulerRot::YXZ, -0.4, 0.6, 0.1);
        for mode in [
            GrappleComfortMode::Off,
            GrappleComfortMode::HoldView,
            GrappleComfortMode::LevelPitch,
        ] {
            let filtered = filter_with(body, mode, Quat::IDENTITY, 0.0);
            assert!((filtered.dot(body).abs() - 1.0).abs() < 1e-6);
        }
    }

    /// A flight's ticks move the root, not the head against it, so however fast and unevenly the
    /// body travels, nothing is absorbed.
    #[test]
    fn root_motion_is_never_absorbed() {
        let mut offset = Vec3::ZERO;
        for step in [0.03, -0.02, 0.05, 0.25, -0.1] {
            offset = absorb_step(offset, Vec3::new(step, 0.0, 0.0), true);
        }
        assert_eq!(offset, Vec3::ZERO);
    }

    /// A landing's jump of the head against the root is taken out of the view while armed, capped,
    /// and passed through while disarmed.
    #[test]
    fn a_landing_snap_is_absorbed_only_while_armed() {
        let snap = Vec3::new(0.0, -0.9, 0.3);
        assert_eq!(absorb_step(Vec3::ZERO, snap, true), -snap);
        assert_eq!(absorb_step(Vec3::ZERO, snap, false), Vec3::ZERO);
        let capped = absorb_step(Vec3::ZERO, Vec3::new(0.0, 3.0, 0.0), true);
        assert!((capped.length() - GRAPPLE.get().comfort.anchor_offset_max_m).abs() < 1e-5);
        assert_eq!(
            absorb_step(capped, Vec3::new(10.0, 0.0, 0.0), true),
            Vec3::ZERO
        );
    }
}
