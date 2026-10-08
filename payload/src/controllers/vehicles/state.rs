//! The vehicle handling's tick-to-tick state: the readout the debug UI and the status page read,
//! the held controls and handles, the mounted gun's hold, and the arm panel the headset reads.

use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use glam::{Quat, Vec3};
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    hands::{self, CockpitHand, tiers},
    overlay::{primitive::Source, store},
    vehicles::{
        arm_steering as arm,
        config::{MountedConfig, ParachuteHandlesConfig},
        driving_view,
        fit::VehicleLatch,
        mounted::{MountedTarget, Reference},
        mounted_grip::MountedGrip,
        mounted_recoil,
        parachute::{self, HandleEvent, HandleHold, HandleState},
        physical::{self, ControlEvent, ControlState, Hold, Outputs, Placement},
        types::{ArmReadout, VehicleSummary, VehiclesStatus},
        wheel_render::{self, RenderWheel},
    },
};

/// The wrists' mean tilt now, degrees, positive with the leading edges down: what the wingsuit's
/// wrist-tilt pitch measures, for capturing the player's relaxed tilt as its neutral. `None` unless
/// both hands are tracked under the controller scheme. Takes the configuration lock and the
/// controller frame slot, one after the other.
pub fn wrist_tilt_now_deg() -> Option<f32> {
    let active = hands::active_hands_raw()?;
    let [left, right] = active.hands;
    let tilt = arm::wrists_tilt(left?.wrist.orientation, right?.wrist.orientation).to_degrees();
    tilt.is_finite().then_some(tilt)
}

/// The arm steering's readout for the headset panel, while that panel is switched on and the arms
/// act in the wingsuit. A leaf slot: the panel reads it on the draw task.
pub fn arm_panel() -> Option<ArmReadout> {
    *ARM_PANEL.lock()
}

pub fn status() -> VehiclesStatus {
    STATE.lock().status.clone()
}

/// How far the parachute's held handles brake, `0..=1`, as the last input tick read them: the
/// share both are pulled past the brake threshold ([`parachute::steer`]). Zero while neither
/// handle is held or the handles do not act. Takes the vehicle handling's lock briefly, so a caller
/// holding another lock that the vehicle handling takes must not ask.
pub fn parachute_brake() -> f32 {
    let brake = STATE
        .lock()
        .status
        .handles
        .and_then(|h| h.handles)
        .map_or(0.0, |stick| -stick.y);
    if brake.is_finite() {
        brake.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub fn summary() -> VehicleSummary {
    let state = STATE.lock();
    let s = &state.status;
    VehicleSummary {
        active: s.active,
        seat: s.seat,
        offered: s.offered,
        holds: s.holds,
        outputs: s.outputs,
        cues: s.cues,
        afterburner_lights: s.afterburner_lights,
        button_presses: s.button_presses,
        mounted: s.mounted.is_some(),
        mounted_holds: s.mounted_holds,
        mounted_grip_events: s.mounted_grip_events,
    }
}

/// Whether `weapon` is the mounted gun the hands last aimed. Lock-free, for the per-weapon update
/// of every character's weapons to rule a weapon out before taking any lock.
pub fn is_mounted_weapon(weapon: usize) -> bool {
    weapon != 0 && MOUNTED_WEAPON.load(Ordering::Relaxed) == weapon
}

/// The mounted gun's aim point for the per-weapon update, when `weapon` is the gun the hands aim.
pub fn mounted_target(weapon: usize) -> Option<Vec3> {
    STATE
        .lock()
        .mounted_target
        .filter(|t| t.weapon == weapon && t.fresh())
        .map(|t| t.point)
}

/// The mounted gun's aim point for the character-level aim point, which draws the gun.
pub fn mounted_aim_point() -> Option<Vec3> {
    STATE
        .lock()
        .mounted_target
        .filter(MountedTarget::fresh)
        .map(|t| t.point)
}

/// Stop acting: let go of every control and forget the mounted gun. Called whenever the bindings
/// do not inject.
pub(crate) fn stand_down() {
    let mut state = STATE.lock();
    let mut events = Vec::new();
    let mut handle_events = Vec::new();
    physical::release_all(&mut state.controls, &mut events);
    parachute::release_all(&mut state.handles, &mut handle_events);
    state.controls = ControlState::default();
    state.handles_on = false;
    state.handles_torso = None;
    state.last_holds = [None; 2];
    state.last_handles = [None; 2];
    state.anchor_weights = [0.0; 2];
    state.mounted_reference = None;
    state.mounted_target = None;
    state.mounted_last = None;
    state.mounted_grip = MountedGrip::new();
    state.mounted_barrel = None;
    state.mounted_handle_points = [None; 2];
    mounted_recoil::set_holds([false; 2]);
    MOUNTED_WEAPON.store(0, Ordering::Relaxed);
    state.mounted_handles = None;
    state.placement = None;
    state.latch = None;
    driving_view::publish(None);
    state.render = None;
    state.steering_turn = None;
    state.afterburner = false;
    wheel_render::publish(None);
    state.status.active = false;
    state.status.seat = None;
    state.status.volumes = [None; 2];
    state.status.holds = [None; 2];
    state.status.mounted = None;
    state.status.arms = None;
    state.status.handles = None;
    state.status.fit = None;
    state.status.wheel = None;
    state.status.rendered_angle = None;
    push_events(&mut state.status, events);
    drop(state);
    *ARM_PANEL.lock() = None;
    log_handle_events(&handle_events);
    store::clear(Source::VehicleControls);
}

/// Put the hands that hold a physical control, a parachute handle, or a mounted gun's handles on
/// them, keeping the controllers' rotation. `hands` are the cockpit-frame hands indexed by
/// [`Hand::index`], as the controllers hold them.
pub fn apply(hands: &mut [Option<CockpitHand>; 2]) {
    let tier_status = tiers::status();
    // The grapple states' anchor weights are the line's
    // ([`crate::controllers::hands::anchor`]), not the handles'.
    let grapple = matches!(
        tier_status.state,
        Some(tiers::TierState::GrappleReel | tiers::TierState::GrappleHang)
    );
    let grips = hands.map(|h| h.map(|h| h.grip.position));
    let kick_offset = mounted_recoil::offset();
    let state = STATE.lock();
    let kick = state
        .mounted_barrel
        .filter(|_| state.mounted_config.recoil)
        .map_or(Vec3::ZERO, |barrel| {
            mounted_recoil::handle_shift(kick_offset, barrel)
        });
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some(h) = hands[i] else {
            continue;
        };
        let control = state.last_holds[i]
            .filter(|_| state.anchor_weights[i] > 0.0)
            .zip(state.placement)
            .and_then(|(hold, placement)| {
                let point = physical::anchor_point(&state.controls, &placement, hold, hand, grips)?;
                Some((point, state.anchor_weights[i]))
            });
        let handle = state.last_handles[i]
            .filter(|_| state.anchor_weights[i] > 0.0)
            .map(|hold| {
                let point = parachute::anchor_point(&state.handles_config, hold, h.grip.position);
                (point, state.anchor_weights[i])
            });
        let mounted = state
            .mounted_handles
            .and_then(|handles| handles[i])
            .filter(|_| !grapple)
            .map(|point| {
                let share = state.mounted_grip.shares(&state.mounted_config)[i];
                (point + kick, tier_status.anchor_weights[i] * share)
            })
            .filter(|(_, weight)| *weight > 0.0);
        if let Some((point, weight)) = control.or(handle).or(mounted) {
            hands[i] = Some(move_grip(h, point, weight));
        }
    }
}

/// How many grabs and releases the readout keeps.
const MAX_EVENTS: usize = 6;

pub(super) struct State {
    pub(super) status: VehiclesStatus,
    pub(super) controls: ControlState,
    pub(super) placement: Option<Placement>,
    /// The control each hand held last, kept while its anchor blends out.
    pub(super) last_holds: [Option<Hold>; 2],
    /// The handle each hand held last, kept while its anchor blends out.
    pub(super) last_handles: [Option<HandleHold>; 2],
    /// Each hand's share of the pose on its control or handle.
    pub(super) anchor_weights: [f32; 2],
    pub(super) handles: HandleState,
    /// Whether the parachute handles act this tick: under the parachute, with them switched on.
    pub(super) handles_on: bool,
    /// The handles' settings as the last tick read them, kept while a hand blends off its handle.
    pub(super) handles_config: ParachuteHandlesConfig,
    /// The torso heading the handles last hung along, and when.
    pub(super) handles_torso: Option<(Quat, Instant)>,
    pub(super) mounted_reference: Option<Reference>,
    pub(super) mounted_target: Option<MountedTarget>,
    /// The gun the hands last aimed and the direction they gave it, in the body frame, which it
    /// keeps while no hand holds a handle.
    pub(super) mounted_last: Option<(usize, Vec3)>,
    /// The hands' hold on the mounted gun's handles.
    pub(super) mounted_grip: MountedGrip,
    /// The mounted gun's barrel in the cockpit frame, for the recoil's kick.
    pub(super) mounted_barrel: Option<Vec3>,
    /// The mounted gun's handles in the cockpit frame, where it places them, for the hold.
    pub(super) mounted_handle_points: [Option<Vec3>; 2],
    /// The mounted-gun settings as the last tick read them, for the hands at every rate.
    pub(super) mounted_config: MountedConfig,
    /// The mounted gun's handles in the cockpit frame, kept while the hands blend off them.
    pub(super) mounted_handles: Option<[Option<Vec3>; 2]>,
    /// What the fit keeps for the driven vehicle.
    pub(super) latch: Option<VehicleLatch>,
    /// The vehicle's wheel part the rendered wheel turns, while the animation is on.
    pub(super) render: Option<RenderWheel>,
    /// The driven vehicle's own steering, as the last fit read it.
    pub(super) steering_turn: Option<f32>,
    /// Whether the throttle was through its afterburner gate on the last tick.
    pub(super) afterburner: bool,
}

pub(super) static STATE: Mutex<State> = Mutex::new(State {
    status: VehiclesStatus {
        active: false,
        seat: None,
        offered: &[],
        volumes: [None; 2],
        holds: [None; 2],
        controls: ControlState::new(),
        outputs: Outputs::NONE,
        events: Vec::new(),
        mounted: None,
        arms: None,
        handles: None,
        fit: None,
        wheel: None,
        rendered_angle: None,
        last_cue: None,
        cues: 0,
        afterburner_lights: 0,
        button_presses: 0,
        mounted_grip_events: 0,
        mounted_holds: [false; 2],
    },
    controls: ControlState::new(),
    placement: None,
    last_holds: [None; 2],
    last_handles: [None; 2],
    anchor_weights: [0.0; 2],
    handles: HandleState::new(),
    handles_on: false,
    handles_config: ParachuteHandlesConfig::new(),
    handles_torso: None,
    mounted_reference: None,
    mounted_target: None,
    mounted_last: None,
    mounted_grip: MountedGrip::new(),
    mounted_barrel: None,
    mounted_handle_points: [None; 2],
    mounted_config: MountedConfig::new(),
    mounted_handles: None,
    latch: None,
    render: None,
    steering_turn: None,
    afterburner: false,
});

/// The arm steering's readout for the headset panel ([`arm_panel`]).
pub(super) static ARM_PANEL: Mutex<Option<ArmReadout>> = Mutex::new(None);

/// The address of the weapon in [`State::mounted_target`], or zero: a lock-free copy for
/// [`is_mounted_weapon`].
pub(super) static MOUNTED_WEAPON: AtomicUsize = AtomicUsize::new(0);

pub(super) fn log_handle_events(events: &[HandleEvent]) {
    for event in events {
        tracing::debug!(target: "controllers", ?event, "parachute handle");
    }
}

/// `hand` with its grip moved toward `point` by `weight`, keeping its rotation.
pub(super) fn move_grip(hand: CockpitHand, point: Vec3, weight: f32) -> CockpitHand {
    let shift = (point - hand.grip.position) * weight.clamp(0.0, 1.0);
    if !shift.is_finite() {
        return hand;
    }
    let moved = |p: Pose| Pose {
        position: p.position + shift,
        orientation: p.orientation,
    };
    CockpitHand {
        wrist: moved(hand.wrist),
        grip: moved(hand.grip),
        aim: moved(hand.aim),
    }
}

pub(super) fn push_events(status: &mut VehiclesStatus, events: Vec<ControlEvent>) {
    for event in &events {
        tracing::debug!(target: "controllers", ?event, "vehicle control");
    }
    status.events.extend(events);
    let excess = status.events.len().saturating_sub(MAX_EVENTS);
    status.events.drain(..excess);
}
