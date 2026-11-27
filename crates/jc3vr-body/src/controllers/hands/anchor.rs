//! The anchored hand tier for the grapple states: the plan of record's "Hand tiers"
//! (`docs/mod/input/controllers-and-roomscale.md`), with the engine side in
//! `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! While the character reels in along the grapple line or hangs from it (the tier table's grapple
//! reel and grapple hang rows), the grapple hand is anchored to the line where its controller is:
//!
//! - **Holding the line.** The hand stays at its controller, turned about its grip so that the
//!   grip's pointing axis (the axis through the closed fist) runs along the line to the hook's
//!   attach point ([`hold_line`]). The turn blends in and out with the tier's anchor weight. Only
//!   the drawn hand turns: the controller's aim pose is left where the controller points, because
//!   the grapple's aim leaves along it, and a fire from the reel or the hang (the game's re-fire
//!   from a reel) latches whatever that aim points at. Turned with the fist, the aim would point
//!   off the controller by the angle between the grip's axis and the aim's, about a right angle's
//!   worth on some controllers, and the second grapple would fly where the player never pointed.
//! - **Driving the hang.** The game's own stick-driven swing on a hang is switched off in this
//!   build, so the stick does nothing to a hanging character beyond picking the hang jump's
//!   direction. The hand drives the hang instead: the hand is fixed to the line, so moving it
//!   relative to the head moves the body the other way. A post-call detour on the hang task
//!   ([`NStateTask_GrapplingHangTask::Update`]) measures the grapple hand's horizontal displacement
//!   from where it was when the hang began, relative to the head, and pushes the hang body at the
//!   hips with an impulse against it ([`swing_impulse`]), scaled against the game's own continuous
//!   swing so that the feel matches what the designers tuned. The hand's motion pumps the swing on
//!   top, through the pump the grapple registers
//!   ([`crate::contributions::BodyContributions::hang_pump`]): while the hand moves
//!   relative to the head, the body is pushed against the motion, and the hold and the pump
//!   together are capped per hang tick.
//!
//! The line's attach point is read once per input tick ([`input_tick`]), which also pulses the
//! grapple hand when the hook leaves the gauntlet and when it attaches.
//!
//! [`NStateTask_GrapplingHangTask::Update`]: jc3gi::equipment::grappling_hook::NStateTask_GrapplingHangTask::Update

use std::ffi::c_void;

use detours_macro::detour;
use glam::{Quat, Vec2, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::grappling_hook::{GrapplingHook, get_GrapplingHangTask_SmallImpulseSwingMagnitude},
    state::StateContext,
    types::math::Vector3,
};
use jc3vr_core::published::Stamp;
use jc3vr_input::{
    frame::{Hand, Pose},
    haptics::cues::Event,
};
use jc3vr_view::{controllers::cockpit::CockpitTransform, headpose};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    contributions,
    controllers::{
        body_frame,
        fingers::grip::{self, Grip, GripSource},
        hands::{self, CockpitHand, tiers, tuning::HANDS},
        haptics,
        physics::grapple_hook as hook,
        scheme,
    },
    hooks::input::locomotion::character_from_context,
    tuning::FRESHNESS,
};

/// The grapple's pump of the hang swing by the grapple hand's motion relative to the head, which
/// the hang task's detour adds to the hold
/// ([`crate::contributions::BodyContributions::hang_pump`]). Each function takes the
/// game's own continuous stick swing at full deflection as `full_impulse`.
#[derive(Clone, Copy)]
pub struct HangPump {
    /// The hand's smoothed velocity: the previous estimate, a new sample, and the seconds between
    /// them.
    pub smooth_velocity: fn(previous: Vec2, sample: Vec2, dt: f32) -> Vec2,
    /// The pump's impulse for the hand's smoothed velocity.
    pub impulse: fn(velocity: Vec2, full_impulse: f32) -> Vec2,
    /// The hold's and the pump's impulses together, capped.
    pub combine: fn(hold: Vec2, pumped: Vec2, full_impulse: f32) -> Vec2,
}

/// The anchor's hooks, aggregated by [`crate::controllers::hands::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&HANG_TASK_UPDATE_BINDER)
}

/// Turn `hand`'s wrist and grip about its grip so that the grip's pointing axis (-Z) runs along
/// `line`, a direction in the hand's frame, by `weight` (`0..=1`). The aim pose stays where the
/// controller points (see the module docs). The hand is returned as it is for a degenerate line.
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
        aim: hand.aim,
    }
}

/// The swing impulse for a grapple hand displaced by `displacement` (horizontal, body frame,
/// metres) from where it held the line when the hang began: against the displacement, from zero at
/// the deadzone to `full_impulse` at the full displacement, and zero inside the deadzone.
pub fn swing_impulse(displacement: Vec2, full_impulse: f32) -> Vec2 {
    let tuning = &HANDS.get().anchor;
    let length = displacement.length();
    let span = tuning.swing_full_m - tuning.swing_deadzone_m;
    if !length.is_finite() || length <= tuning.swing_deadzone_m {
        return Vec2::ZERO;
    }
    let strength = ((length - tuning.swing_deadzone_m) / span).clamp(0.0, 1.0);
    -displacement / length * strength * full_impulse * tuning.swing_scale
}

/// Put the anchored hands on the line. `hands` are the cockpit-frame hands indexed by
/// [`Hand::index`], `head` the head's cockpit pose, and `world_scale` the VR world scale.
pub fn apply(hands: &mut [Option<CockpitHand>; 2], head: Pose, world_scale: f32) {
    let status = body_frame::tiers();
    // The mounted guns anchor the hands too, to their handles
    // (the gameplay crate's `controllers::vehicles::apply`), which the line must not turn.
    let mounted = status.state == Some(tiers::TierState::MountedGun);
    let weights = status.anchor_weights;
    if mounted || weights.iter().all(|w| *w <= 0.0) {
        return;
    }
    let Some(anchor) = line_anchor() else {
        return;
    };
    let xf = CockpitTransform::current(head.position, world_scale);
    for hand in Hand::BOTH {
        let weight = weights[hand.index()];
        let Some(h) = hands[hand.index()].filter(|_| weight > 0.0) else {
            continue;
        };
        let grip = xf.pose_to_world(h.grip);
        let line = xf.direction_to_cockpit(anchor - grip.position);
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
    /// The last swing impulse applied, the hold and the pump together.
    pub impulse: Vec2,
    /// The pump's part of the last swing impulse.
    pub pump: Vec2,
    /// Hang ticks that applied an impulse.
    pub swings: u64,
    /// Hang ticks whose impulse the hand's motion pumped.
    pub pumps: u64,
}

pub fn status() -> AnchorStatus {
    STATE.lock().status
}

/// Read the grapple line for the tick, and pulse the grapple hand when the hook leaves the gauntlet
/// or attaches. Called from the game stage while the controllers are enabled, with the hand the
/// roles follow ([`scheme::acting_hand`](crate::controllers::scheme::acting_hand)).
///
/// # Safety
///
/// The game stage: the game thread, before the world update.
pub unsafe fn input_tick(dominant: Hand) {
    let hook = unsafe { hook::local_hook() }.map(|hook| &*hook);
    let wire = hook.is_some_and(|h| h.m_ActiveWire.exists());
    let anchor = hook.and_then(|h| unsafe { attach_point(h) });
    let mut state = STATE.lock();
    let fired = wire && !state.status.wire;
    let attached = anchor.is_some() && state.status.anchor.is_none();
    state.status.wire = wire;
    state.status.anchor = anchor;
    state.anchor_at = anchor.map(|_| Stamp::now());
    drop(state);
    declare_line_grip(dominant.other());

    let hand = dominant.other();
    if fired {
        haptics::play(hand, Event::GrappleFire);
    }
    if attached {
        haptics::play(hand, Event::GrappleAttach);
    }
}

/// Declare the grapple hand's grip on the line while the grapple states anchor it, for its finger
/// pose, at the anchor's share.
fn declare_line_grip(hand: Hand) {
    let status = body_frame::tiers();
    let grapple = matches!(
        status.state,
        Some(tiers::TierState::GrappleReel | tiers::TierState::GrappleHang)
    );
    let weight = status.anchor_weights[hand.index()];
    if grapple && status.effective[hand.index()] == tiers::HandTier::Anchored && weight > 0.0 {
        grip::declare(hand, Grip::new(GripSource::GrappleLine).share(weight));
    }
}

/// The hands' interpolation fraction for the attach point: the current tick's end.
const CURRENT_TICK: f32 = 1.0;

struct State {
    status: AnchorStatus,
    anchor_at: Option<Stamp>,
    /// Where the grapple hand held the line, relative to the head, when the hang began.
    hang_reference: Option<Vec2>,
    hang_at: Option<Stamp>,
    /// The grapple hand's offset from the head on the last hang tick, for the pump's velocity.
    hang_offset: Option<Vec2>,
    /// The grapple hand's smoothed velocity relative to the head, metres per second.
    hang_velocity: Vec2,
}

static STATE: Mutex<State> = Mutex::new(State {
    status: AnchorStatus {
        anchor: None,
        wire: false,
        displacement: None,
        impulse: Vec2::ZERO,
        pump: Vec2::ZERO,
        swings: 0,
        pumps: 0,
    },
    anchor_at: None,
    hang_reference: None,
    hang_at: None,
    hang_offset: None,
    hang_velocity: Vec2::ZERO,
});

/// The line's attach point, while it is current.
fn line_anchor() -> Option<Vec3> {
    let state = STATE.lock();
    state
        .anchor_at
        .filter(|at| {
            FRESHNESS
                .get()
                .hands
                .anchor_ms
                .window()
                .holds(*at, Stamp::now())
        })
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

/// Push the local player's hang body by the grapple hand's displacement and motion.
unsafe fn swing(character: &mut Character) {
    let pump_fns = contributions::get().and_then(|contributions| contributions.hang_pump.as_ref());
    let hand = scheme::acting_hand().other();
    let now = Stamp::now();
    // Read before this module's state is locked, so that no other lock is taken while it is held.
    let anchored = body_frame::tiers().effective[hand.index()] == tiers::HandTier::Anchored;
    let offset = hands::active_hands_raw()
        .filter(|_| anchored)
        .and_then(|active| {
            let grip = active.hands[hand.index()]?.grip.position;
            let offset = grip - active.head.position;
            Some(Vec2::new(offset.x, offset.z) * active.world_scale)
        });
    let body_rotation = headpose::xr::body_rotation();

    let mut state = STATE.lock();
    let dt = state
        .hang_at
        .map(|at| now.saturating_duration_since(at))
        .filter(|gap| *gap <= FRESHNESS.get().hands.hang_gap_ms.duration());
    if dt.is_none() {
        state.hang_reference = None;
        state.hang_offset = None;
        state.hang_velocity = Vec2::ZERO;
    }
    state.hang_at = Some(now);
    let Some(offset) = offset else {
        state.hang_reference = None;
        state.hang_offset = None;
        state.hang_velocity = Vec2::ZERO;
        state.status.displacement = None;
        return;
    };
    let reference = *state.hang_reference.get_or_insert(offset);
    let displacement = offset - reference;
    state.status.displacement = Some(displacement);
    if let (Some(previous), Some(dt), Some(fns)) =
        (state.hang_offset, dt.map(|d| d.as_secs_f32()), pump_fns)
        && dt > f32::EPSILON
    {
        state.hang_velocity =
            (fns.smooth_velocity)(state.hang_velocity, (offset - previous) / dt, dt);
    }
    state.hang_offset = Some(offset);

    let full = unsafe { *get_GrapplingHangTask_SmallImpulseSwingMagnitude() };
    let hold = swing_impulse(displacement, full);
    let pumped = pump_fns.map_or(Vec2::ZERO, |fns| (fns.impulse)(state.hang_velocity, full));
    let impulse = pump_fns.map_or(hold, |fns| (fns.combine)(hold, pumped, full));
    if impulse == Vec2::ZERO {
        return;
    }
    let world = body_rotation * Vec3::new(impulse.x, 0.0, impulse.y);
    let Some(hook) = (unsafe { hook::of_mut(character) }) else {
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
    state.status.pump = pumped;
    state.status.swings += 1;
    if pumped != Vec2::ZERO {
        state.status.pumps += 1;
    }
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
        let mut h = hand(Quat::IDENTITY);
        h.aim.orientation = Quat::from_rotation_x(-0.7);
        let line = Vec3::new(0.0, 1.0, -0.3);
        let held = hold_line(h, line, 1.0);
        let axis = held.grip.orientation * Vec3::NEG_Z;
        assert!(axis.dot(line.normalize()) > 0.9999);
        assert!(held.grip.position.distance(h.grip.position) < 1e-6);
        assert_eq!(held.aim, h.aim, "the aim stays on the controller");
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
        let tuning = &HANDS.get().anchor;
        let (deadzone, full_m) = (tuning.swing_deadzone_m, tuning.swing_full_m);
        let scale = 10.0 * tuning.swing_scale;
        let inside = swing_impulse(Vec2::new(deadzone * 0.5, 0.0), 10.0);
        assert_eq!(inside, Vec2::ZERO);
        let half = swing_impulse(Vec2::new(0.0, -(deadzone + full_m) / 2.0), 10.0);
        assert!((half.y - scale / 2.0).abs() < 1e-3, "{half}");
        assert!(half.x.abs() < 1e-6);
        let full = swing_impulse(Vec2::new(full_m * 3.0, 0.0), 10.0);
        assert!((full.x + scale).abs() < 1e-4);
        assert_eq!(swing_impulse(Vec2::new(f32::NAN, 0.0), 10.0), Vec2::ZERO);
    }
}
