//! The vehicle handling's tick-to-tick state: the readout the debug UI and the status rows read,
//! the held controls and handles, the mounted gun's hold, and the arm panel the headset reads.

use std::sync::atomic::{AtomicUsize, Ordering};

use glam::Vec3;
use jc3vr_body::controllers::{
    body_frame, facts, hands,
    hands::{CockpitHand, tiers},
    mode::VehicleSeat,
    physics::refs::Strong,
};
use jc3vr_core::published::{EventLog, Published, Stage};
use jc3vr_input::frame::{Hand, Pose};
use jc3vr_view::controllers::overlay::{primitive::Source, store};
use parking_lot::Mutex;

use crate::vehicles::{
    arm_steering as arm, driving_view,
    entry::VehicleEntryIdentity,
    fit::VehicleLatch,
    mounted::{MountedTarget, Reference},
    mounted_grip::MountedGrip,
    mounted_recoil, parachute,
    parachute::{HandleEvent, HandleHold, HandleState},
    physical,
    physical::{ControlEvent, ControlState, Hold, Outputs, Placement},
    tuning::VEHICLES,
    types::VehiclesStatus,
    wheel_render,
    wheel_render::RenderWheel,
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

pub fn status() -> VehiclesStatus {
    STATE.lock().status.clone()
}

/// Whether each hand holds its mounted gun's handle, as the last input tick left it.
pub(super) fn mounted_holds() -> [bool; 2] {
    STATE.lock().status.mounted_holds
}

/// Copy current handle owners without reconstructing them from diagnostic booleans.
#[inline]
pub fn parachute_holds() -> [Option<HandleHold>; 2] {
    let state = STATE.lock();
    if state.status.active && state.handles_on {
        state.handles.holds
    } else {
        [None; 2]
    }
}

/// Publish each hand's hold on a mounted gun's handle ([`facts::mounted::holds`]).
pub(super) fn publish_mounted_holds(holds: [bool; 2]) {
    facts::mounted::publish_holds(holds);
}

/// Whether `weapon` is the mounted gun the hands last aimed. Lock-free, for the per-weapon update
/// of every character's weapons to rule a weapon out before taking any lock.
pub fn is_mounted_weapon(weapon: usize) -> bool {
    weapon != 0 && MOUNTED_WEAPON.load(Ordering::Relaxed) == weapon
}

/// The mounted gun's aim point for the per-weapon update, when `weapon` is the gun the hands aim.
pub fn mounted_target(weapon: usize) -> Option<Vec3> {
    fresh_mounted_target()
        .filter(|t| t.weapon == weapon)
        .map(|t| t.point)
}

/// The mounted gun's aim point for the character-level aim point, which draws the gun.
#[inline]
pub fn mounted_aim_point() -> Option<Vec3> {
    fresh_mounted_target().map(|t| t.point)
}

/// Stop acting: let go of every control and forget the mounted gun. Called whenever the bindings
/// do not inject.
pub fn stand_down() {
    crate::vehicles::rail::stand_down();
    physical::calibration::leave();
    let mut state = STATE.lock();
    let mut events = Vec::new();
    let mut handle_events = Vec::new();
    physical::release_all(&mut state.controls, &mut events);
    parachute::release_all(&mut state.handles, &mut handle_events);
    state.controls = ControlState::default();
    state.handles_on = false;
    state.last_holds = [None; 2];
    state.last_handles = [None; 2];
    state.anchor_weights = [0.0; 2];
    state.mounted_reference = None;
    MOUNTED_TARGET.clear();
    state.mounted_last = None;
    state.mounted_grip = MountedGrip::new();
    state.mounted_barrel = None;
    state.mounted_handle_points = [None; 2];
    mounted_recoil::set_holds([false; 2]);
    MOUNTED_WEAPON.store(0, Ordering::Relaxed);
    publish_mounted_holds([false; 2]);
    state.mounted_handles = None;
    state.placement = None;
    state.latch = None;
    state.entry = None;
    let retired_owner = state.entry_owner.take();
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
    drop(retired_owner);
    log_handle_events(&handle_events);
    store::clear(Source::WingGuide);
}

/// Put the hands that hold a physical control, a parachute handle, or a mounted gun's handles on
/// them, keeping the controllers' rotation. `hands` are the cockpit-frame hands indexed by
/// [`Hand::index`], as the controllers hold them.
pub fn apply(hands: &mut [Option<CockpitHand>; 2]) {
    let tier_status = body_frame::tiers();
    // The grapple states' anchor weights are the line's
    // ([`jc3vr_body::controllers::hands::anchor`]), not the handles'.
    let grapple = matches!(
        tier_status.state,
        Some(tiers::TierState::GrappleReel | tiers::TierState::GrappleHang)
    );
    let grips = hands.map(|h| h.map(|h| h.grip.position));
    let kick_offset = mounted_recoil::offset();
    let state = STATE.lock();
    let kick = state.mounted_barrel.map_or(Vec3::ZERO, |barrel| {
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
                let point = parachute::anchor_point(
                    &VEHICLES.get().parachute_handles,
                    hold,
                    h.grip.position,
                );
                (point, state.anchor_weights[i])
            });
        let mounted = state
            .mounted_handles
            .and_then(|handles| handles[i])
            .map(|point| {
                let weight = mounted_hand_weight(MountedHandWeight {
                    manned: tier_status.state == Some(tiers::TierState::MountedGun),
                    grapple,
                    anchor_weight: tier_status.anchor_weights[i],
                    share: state.mounted_grip.shares()[i],
                });
                (point + kick, weight)
            })
            .filter(|(_, weight)| *weight > 0.0);
        if let Some((point, weight)) = control.or(handle).or(mounted) {
            hands[i] = Some(move_grip(h, point, weight));
        }
    }
}

/// What decides how far a hand is drawn onto its mounted gun's handle.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct MountedHandWeight {
    /// Whether the character mans the gun in the native mounted state, whose tier anchors the
    /// hands.
    pub(super) manned: bool,
    /// Whether a grapple state owns the hands' anchor weights.
    pub(super) grapple: bool,
    /// The hand's anchor weight from its tier.
    pub(super) anchor_weight: f32,
    /// The hand's share of the pose on its handle ([`MountedGrip::shares`]).
    pub(super) share: f32,
}

/// How far a hand is drawn onto its mounted gun's handle, `0..=1`. Manning the gun, the anchored
/// tier's weight scales the hand's share. Off the native mounted state, a hand that took a
/// standalone gun's handle with its grip is drawn there by its grip's blend alone, since the tiers
/// leave it free (on the 14 November take the hands floated at the controllers beside a tripod
/// minigun they held). A grapple state's weights are the line's, never the handles'.
pub(super) fn mounted_hand_weight(inputs: MountedHandWeight) -> f32 {
    let weight = if inputs.grapple {
        0.0
    } else if inputs.manned {
        inputs.anchor_weight * inputs.share
    } else {
        inputs.share
    };
    if weight.is_finite() {
        weight.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub(super) struct State {
    pub(super) status: VehiclesStatus,
    pub(super) entry: Option<VehicleEntryIdentity>,
    pub(super) entry_owner: Option<Strong>,
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
    pub(super) mounted_reference: Option<Reference>,
    /// The gun the hands last aimed and the direction they gave it, in the body frame, which it
    /// keeps while no hand holds a handle.
    pub(super) mounted_last: Option<(usize, Vec3)>,
    /// The hands' hold on the mounted gun's handles.
    pub(super) mounted_grip: MountedGrip,
    /// The mounted gun's barrel in the cockpit frame, for the recoil's kick.
    pub(super) mounted_barrel: Option<Vec3>,
    /// The mounted gun's handles in the cockpit frame, where it places them, for the hold.
    pub(super) mounted_handle_points: [Option<Vec3>; 2],
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

impl State {
    pub(super) fn observe_entry(
        &mut self,
        seat: Option<VehicleSeat>,
        identity: Option<VehicleEntryIdentity>,
        events: &mut Vec<ControlEvent>,
    ) {
        if self.status.seat == seat && self.entry == identity {
            return;
        }
        physical::release_all(&mut self.controls, events);
        self.controls = ControlState::default();
        for hand in Hand::BOTH {
            let i = hand.index();
            if self.last_holds[i].take().is_some() {
                self.anchor_weights[i] = 0.0;
            }
        }
        self.placement = None;
        self.latch = None;
        self.render = None;
        self.steering_turn = None;
        self.afterburner = false;
        self.status.holds = [None; 2];
        self.status.controls = self.controls;
        self.status.outputs = Outputs::NONE;
        self.status.volumes = [None; 2];
        self.status.fit = None;
        self.status.wheel = None;
        self.status.rendered_angle = None;
        self.entry = identity;
    }
}

impl State {
    pub(super) const fn new() -> Self {
        Self {
            entry: None,
            entry_owner: None,
            status: VehiclesStatus {
                active: false,
                seat: None,
                offered: &[],
                volumes: [None; 2],
                holds: [None; 2],
                controls: ControlState::new(),
                outputs: Outputs::NONE,
                events: EventLog::new(),
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
            mounted_reference: None,
            mounted_last: None,
            mounted_grip: MountedGrip::new(),
            mounted_barrel: None,
            mounted_handle_points: [None; 2],
            mounted_handles: None,
            latch: None,
            render: None,
            steering_turn: None,
            afterburner: false,
        }
    }
}

pub(super) static STATE: Mutex<State> = Mutex::new(State::new());

/// The address of the mounted gun the hands aim, or zero: a lock-free copy for
/// [`is_mounted_weapon`].
pub(super) static MOUNTED_WEAPON: AtomicUsize = AtomicUsize::new(0);

/// The mounted gun's aim point for the per-weapon update. An old one belongs to a gun the player
/// has left or a paused game.
pub(super) static MOUNTED_TARGET: Published<MountedTarget> = Published::new(Stage::InputGame);

fn fresh_mounted_target() -> Option<MountedTarget> {
    MOUNTED_TARGET.fresh(VEHICLES.get().mounted.max_age())
}

pub(super) fn log_handle_events(events: &[HandleEvent]) {
    for event in events {
        match *event {
            HandleEvent::Taken(hand) => {
                tracing::info!(target: "controllers", hand = hand.label(), "parachute: took a handle");
            }
            HandleEvent::Released(hand, why) => {
                tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    reason = why.label(),
                    "parachute: let go of a handle"
                );
            }
        }
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
        match *event {
            ControlEvent::Grabbed(hand, kind) => {
                tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    control = kind.label(),
                    "vehicle control: took the control"
                );
            }
            ControlEvent::Released(hand, kind, why) => {
                tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    control = kind.label(),
                    reason = why.label(),
                    "vehicle control: let go of the control"
                );
            }
            ControlEvent::Button(_) => {
                tracing::debug!(target: "controllers", ?event, "vehicle control");
            }
        }
    }
    status.events.extend(events);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weight(manned: bool, share: f32) -> f32 {
        mounted_hand_weight(MountedHandWeight {
            manned,
            grapple: false,
            anchor_weight: if manned { 1.0 } else { 0.0 },
            share,
        })
    }

    #[test]
    fn a_hand_on_a_standalone_guns_handle_is_drawn_there_by_its_grip_alone() {
        // Off the native mounted state the tiers leave the hands free, and the grip's blend draws
        // a holding hand onto its handle, in and out.
        assert_eq!(weight(false, 1.0), 1.0);
        assert_eq!(weight(false, 0.4), 0.4);
        assert_eq!(weight(false, 0.0), 0.0);
        // Manning the gun, the anchored tier scales the share.
        assert_eq!(weight(true, 0.5), 0.5);
        let blending_in = mounted_hand_weight(MountedHandWeight {
            manned: true,
            grapple: false,
            anchor_weight: 0.5,
            share: 1.0,
        });
        assert_eq!(blending_in, 0.5);
    }

    #[test]
    fn under_a_grapple_no_hand_goes_to_a_handle() {
        let grapple = MountedHandWeight {
            manned: false,
            grapple: true,
            anchor_weight: 1.0,
            share: 1.0,
        };
        assert_eq!(mounted_hand_weight(grapple), 0.0);
        let broken = MountedHandWeight {
            grapple: false,
            share: f32::NAN,
            ..grapple
        };
        assert_eq!(mounted_hand_weight(broken), 0.0);
    }
}
