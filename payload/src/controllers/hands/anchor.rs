//! The anchored hand tier for the grapple states: the plan of record's "Hand tiers"
//! (`docs/mod/input/controllers-and-roomscale.md`), with the engine side in
//! `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! While the character reels in along the grapple line or hangs from it (the tier table's grapple
//! reel and grapple hang rows), the grapple hand is anchored to the line where its controller is:
//!
//! - **Holding the line.** The hand stays at its controller, turned about its grip so that the
//!   grip's pointing axis (the axis through the closed fist) runs along the line to the hook's
//!   attach point ([`hold_line`]). The turn blends in and out with the tier's anchor weight.
//! - **Driving the hang.** The game's own stick-driven swing on a hang is switched off in this
//!   build, so the stick does nothing to a hanging character beyond picking the hang jump's
//!   direction. The hand drives the hang instead: the hand is fixed to the line, so moving it
//!   relative to the head moves the body the other way. A post-call detour on the hang task
//!   ([`NStateTask_GrapplingHangTask::Update`]) measures the grapple hand's horizontal displacement
//!   from where it was when the hang began, relative to the head, and pushes the hang body at the
//!   hips with an impulse against it ([`swing_impulse`]), scaled against the game's own continuous
//!   swing so that the feel matches what the designers tuned.
//!
//! The line's attach point is read once per input tick ([`input_tick`]), which also pulses the
//! grapple hand when the hook leaves the gauntlet and when it attaches.
//!
//! [`NStateTask_GrapplingHangTask::Update`]: jc3gi::equipment::grappling_hook::NStateTask_GrapplingHangTask::Update

use std::{
    ffi::c_void,
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::{Quat, Vec2, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::grappling_hook::{GrapplingHook, get_GrapplingHangTask_SmallImpulseSwingMagnitude},
    state::StateContext,
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    controllers::{
        config::ControllersConfig,
        frame::{Hand, Pose},
        hands::{self, CockpitHand, pose as hand_pose, tiers},
        scheme,
        weapons::haptics::{self, PulseSource},
    },
    headpose,
    hooks::input::locomotion::character_from_context,
};

/// The anchored tier's settings, held in
/// [`HandsConfig`](crate::controllers::config::HandsConfig). Acts only under the controller scheme,
/// so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnchorConfig {
    /// Anchor the grapple hand to the line in the grapple states. Off, those states yield the hand
    /// to the animation.
    pub enabled: bool,
    /// How long the hand takes to turn onto the line and back off it, seconds.
    pub blend_s: f32,
    /// Swing a hanging character by moving the grapple hand.
    pub hang_swing: bool,
    /// The hand displacement below which the hang is left alone, metres.
    pub swing_deadzone_m: f32,
    /// The hand displacement that swings at full strength, metres.
    pub swing_full_m: f32,
    /// The full-strength swing impulse per hang tick, as a multiple of the game's own continuous
    /// stick swing at full deflection. Negative swings the body the way the hand moves.
    pub swing_scale: f32,
}

impl AnchorConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            blend_s: 0.15,
            hang_swing: true,
            swing_deadzone_m: 0.05,
            swing_full_m: 0.3,
            swing_scale: 1.0,
        }
    }
}

impl Default for AnchorConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The anchor's hooks, aggregated by [`crate::controllers::hands::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&HANG_TASK_UPDATE_BINDER)
}

/// Turn `hand` about its grip so that the grip's pointing axis (-Z) runs along `line`, a direction
/// in the hand's frame, by `weight` (`0..=1`). The hand is returned as it is for a degenerate line.
pub fn hold_line(hand: CockpitHand, line: Vec3, weight: f32) -> CockpitHand {
    let weight = weight.clamp(0.0, 1.0);
    let (Some(line), Some(axis)) = (
        line.try_normalize(),
        (hand.grip.orientation * Vec3::NEG_Z).try_normalize(),
    ) else {
        return hand;
    };
    if weight <= 0.0 {
        return hand;
    }
    let turn = Quat::IDENTITY.slerp(Quat::from_rotation_arc(axis, line), weight);
    let pivot = hand.grip.position;
    let rotate = |p: Pose| Pose {
        position: pivot + turn * (p.position - pivot),
        orientation: (turn * p.orientation).normalize(),
    };
    CockpitHand {
        wrist: rotate(hand.wrist),
        grip: rotate(hand.grip),
        aim: rotate(hand.aim),
    }
}

/// The swing impulse for a grapple hand displaced by `displacement` (horizontal, body frame,
/// metres) from where it held the line when the hang began: against the displacement, from zero at
/// the deadzone to `full_impulse` at the full displacement, and zero inside the deadzone.
pub fn swing_impulse(displacement: Vec2, full_impulse: f32, config: &AnchorConfig) -> Vec2 {
    let length = displacement.length();
    let span = (config.swing_full_m - config.swing_deadzone_m).max(1e-3);
    if !length.is_finite() || length <= config.swing_deadzone_m {
        return Vec2::ZERO;
    }
    let strength = ((length - config.swing_deadzone_m) / span).clamp(0.0, 1.0);
    -displacement / length * strength * full_impulse * config.swing_scale
}

/// Put the anchored hands on the line. `hands` are the cockpit-frame hands indexed by
/// [`Hand::index`], `head` the head's cockpit pose, and `world_scale` the VR world scale.
pub fn apply(hands: &mut [Option<CockpitHand>; 2], head: Pose, world_scale: f32) {
    let status = tiers::status();
    // The mounted guns anchor the hands too, to their handles
    // ([`crate::controllers::vehicles::apply`]), which the line must not turn.
    let mounted = status.state == Some(tiers::TierState::MountedGun);
    let weights = status.anchor_weights;
    if mounted || weights.iter().all(|w| *w <= 0.0) {
        return;
    }
    let Some(anchor) = line_anchor() else {
        return;
    };
    let head_world = headpose::query().position;
    let body = headpose::xr::body_rotation();
    for hand in Hand::BOTH {
        let weight = weights[hand.index()];
        let Some(h) = hands[hand.index()].filter(|_| weight > 0.0) else {
            continue;
        };
        let grip = hand_pose::compose_world(head_world, body, head.position, h.grip, world_scale);
        let line = body.inverse() * (anchor - grip.position);
        hands[hand.index()] = Some(hold_line(h, line, weight));
    }
}

/// The anchor's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct AnchorStatus {
    /// The line's attach point in the world, while a line is attached.
    pub anchor: Option<Vec3>,
    /// Whether a grapple line is live.
    pub wire: bool,
    /// The grapple hand's displacement driving the hang, metres, while hanging.
    pub displacement: Option<Vec2>,
    /// The last swing impulse applied.
    pub impulse: Vec2,
    /// Hang ticks that applied an impulse.
    pub swings: u64,
}

pub fn status() -> AnchorStatus {
    STATE.lock().status
}

/// Read the grapple line for the tick, and pulse the grapple hand when the hook leaves the gauntlet
/// or attaches. Called from the input tick while the controllers are enabled.
///
/// # Safety
///
/// Main thread, outside the world update.
pub(crate) unsafe fn input_tick(config: &ControllersConfig) {
    let hook = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .and_then(|c| unsafe { c.m_Inventory.m_GrapplingHook.px.as_ref() });
    let wire = hook.is_some_and(|h| !h.m_ActiveWire.px.is_null());
    let anchor = hook.and_then(|h| unsafe { attach_point(h) });
    let mut state = STATE.lock();
    let fired = wire && !state.status.wire;
    let attached = anchor.is_some() && state.status.anchor.is_none();
    state.status.wire = wire;
    state.status.anchor = anchor;
    state.anchor_at = anchor.map(|_| Instant::now());
    drop(state);

    let haptics = &config.weapons.haptics;
    if !(haptics.grapple && scheme::is_controllers()) {
        return;
    }
    let hand = config.acting_hand().other();
    if fired {
        haptics::pulse(hand, haptics.grapple_fire, PulseSource::GrappleFire);
    }
    if attached {
        haptics::pulse(hand, haptics.grapple_attach, PulseSource::GrappleAttach);
    }
}

/// How long the input tick's attach point stays current.
const ANCHOR_MAX_AGE: Duration = Duration::from_millis(250);
/// A gap in the hang task's ticks longer than this begins a new hang.
const HANG_GAP: Duration = Duration::from_millis(300);
/// The hands' interpolation fraction for the attach point: the current tick's end.
const CURRENT_TICK: f32 = 1.0;

struct State {
    status: AnchorStatus,
    anchor_at: Option<Instant>,
    /// Where the grapple hand held the line, relative to the head, when the hang began.
    hang_reference: Option<Vec2>,
    hang_at: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    status: AnchorStatus {
        anchor: None,
        wire: false,
        displacement: None,
        impulse: Vec2::ZERO,
        swings: 0,
    },
    anchor_at: None,
    hang_reference: None,
    hang_at: None,
});

/// The line's attach point, while it is current.
fn line_anchor() -> Option<Vec3> {
    let state = STATE.lock();
    state
        .anchor_at
        .filter(|at| at.elapsed() < ANCHOR_MAX_AGE)
        .and(state.status.anchor)
}

/// The active wire's attached far end in the world, when a wire is attached.
unsafe fn attach_point(hook: &GrapplingHook) -> Option<Vec3> {
    let mut position = Vector3 { data: [0.0; 3] };
    let attached =
        unsafe { hook.GetActiveWireAttachmentGraphicsWorldPosition(CURRENT_TICK, &mut position) };
    let position = Vec3::from_array(position.data);
    (attached && position.is_finite()).then_some(position)
}

#[detour(address = jc3gi::equipment::grappling_hook::NStateTask_GrapplingHangTask::Update_ADDRESS)]
fn hang_task_update(ctx: *mut StateContext, p1: *mut c_void, p2: *mut c_void) {
    HANG_TASK_UPDATE.get().unwrap().call(ctx, p1, p2);
    // SAFETY: the task's context names its character; the post-call runs inside the character's
    // serial state-machine pass, outside the physics step, as the task's own impulses do.
    unsafe {
        if let Some(character) = character_from_context(ctx).filter(|c| c.m_IsLocalCharacter) {
            swing(character);
        }
    }
}

/// Push the local player's hang body by the grapple hand's displacement.
unsafe fn swing(character: &mut Character) {
    let config = Config::lock_query(|c| c.controllers);
    let anchor = config.hands.anchor;
    let hand = config.acting_hand().other();
    let now = Instant::now();
    // Read before this module's state is locked, so that no other lock is taken while it is held.
    let anchored = tiers::status().effective[hand.index()] == tiers::HandTier::Anchored;
    let offset = hands::active_hands_raw()
        .filter(|_| anchor.enabled && anchor.hang_swing && anchored)
        .and_then(|active| {
            let grip = active.hands[hand.index()]?.grip.position;
            let offset = grip - active.head.position;
            Some(Vec2::new(offset.x, offset.z) * active.world_scale)
        });
    let body_rotation = headpose::xr::body_rotation();

    let mut state = STATE.lock();
    if state
        .hang_at
        .is_none_or(|at| now.duration_since(at) > HANG_GAP)
    {
        state.hang_reference = None;
    }
    state.hang_at = Some(now);
    let Some(offset) = offset else {
        state.hang_reference = None;
        state.status.displacement = None;
        return;
    };
    let reference = *state.hang_reference.get_or_insert(offset);
    let displacement = offset - reference;
    state.status.displacement = Some(displacement);

    let full = unsafe { *get_GrapplingHangTask_SmallImpulseSwingMagnitude() };
    let impulse = swing_impulse(displacement, full, &anchor);
    if impulse == Vec2::ZERO {
        return;
    }
    let world = body_rotation * Vec3::new(impulse.x, 0.0, impulse.y);
    let Some(hook) = (unsafe { character.m_Inventory.m_GrapplingHook.px.as_mut() }) else {
        return;
    };
    let proxy = &mut hook.m_GrapplingHangProxy;
    if !unsafe { proxy.IsOk() } {
        return;
    }
    let Some(body) = (unsafe { proxy.m_PfxProxyBody.px.as_mut() }) else {
        return;
    };
    let mut hips = Vector3 { data: [0.0; 3] };
    unsafe { character.GetHipsPosition(&mut hips) };
    let world = Vector3 {
        data: world.to_array(),
    };
    unsafe { body.ApplyImpulse(&world, &hips, IMPULSE_DEBUG_SCALE) };
    state.status.impulse = impulse;
    state.status.swings += 1;
}

/// The debug-display scale the hang task passes with its own impulses.
const IMPULSE_DEBUG_SCALE: f32 = 0.1;

#[cfg(test)]
mod tests {
    use super::*;

    fn hand(orientation: Quat) -> CockpitHand {
        let grip = Pose {
            position: Vec3::new(0.1, 1.5, -0.2),
            orientation,
        };
        CockpitHand {
            wrist: Pose {
                position: grip.position + orientation * Vec3::new(0.0, 0.0, 0.08),
                orientation,
            },
            grip,
            aim: grip,
        }
    }

    #[test]
    fn the_hand_turns_its_fist_onto_the_line_and_stays_put() {
        let h = hand(Quat::IDENTITY);
        let line = Vec3::new(0.0, 1.0, -0.3);
        let held = hold_line(h, line, 1.0);
        let axis = held.grip.orientation * Vec3::NEG_Z;
        assert!(axis.dot(line.normalize()) > 0.9999);
        assert!(held.grip.position.distance(h.grip.position) < 1e-6);
        let untouched = hold_line(h, line, 0.0);
        assert_eq!(untouched, h);
        let degenerate = hold_line(h, Vec3::ZERO, 1.0);
        assert_eq!(degenerate, h);
    }

    #[test]
    fn half_weight_turns_half_way() {
        let h = hand(Quat::IDENTITY);
        let held = hold_line(h, Vec3::Y, 0.5);
        let axis = held.grip.orientation * Vec3::NEG_Z;
        let angle = axis.angle_between(Vec3::NEG_Z);
        assert!((angle - std::f32::consts::FRAC_PI_4).abs() < 1e-4);
    }

    #[test]
    fn the_swing_pushes_against_the_hand_and_saturates() {
        let config = AnchorConfig::new();
        assert_eq!(
            swing_impulse(Vec2::new(0.03, 0.0), 10.0, &config),
            Vec2::ZERO
        );
        let half = swing_impulse(Vec2::new(0.0, -0.175), 10.0, &config);
        assert!((half.y - 5.0).abs() < 1e-3, "{half}");
        assert!(half.x.abs() < 1e-6);
        let full = swing_impulse(Vec2::new(0.9, 0.0), 10.0, &config);
        assert!((full.x + 10.0).abs() < 1e-4);
        let inverted = AnchorConfig {
            swing_scale: -1.0,
            ..config
        };
        assert!(swing_impulse(Vec2::new(0.9, 0.0), 10.0, &inverted).x > 0.0);
        assert_eq!(
            swing_impulse(Vec2::new(f32::NAN, 0.0), 10.0, &config),
            Vec2::ZERO
        );
    }
}
