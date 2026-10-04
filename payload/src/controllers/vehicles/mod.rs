//! Vehicles, mounted guns, the wingsuit, and the parachute under the controller scheme: the plan of
//! record's "Vehicle, mounted, and wingsuit weapons", its vehicle and wingsuit binding tables, and
//! its decisions on physical controls and arm steering (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`config`]: the settings, all under [`ControllersConfig::vehicles`];
//! - [`physical`]: the opt-in wheel, handlebars, cyclic, collective, flight stick, and throttle of
//!   the driver's seat, grabbed with the grip, which write the vehicle's analog axes and hold the
//!   hands;
//! - [`steering`]: how the held wheel turns and maps to the steering;
//! - [`wheel`] and [`fit`]: the wheel and the handlebars placed where the driven vehicle's own are;
//! - [`driving_view`]: the driver's view moved per vehicle to a standard place behind the wheel;
//! - [`wheel_render`]: the vehicle's rendered steering wheel turned with the steering;
//! - [`mounted`]: mounted guns and gunner seats, whose handles hold the hands and aim the gun;
//! - [`arm_steering`]: the arm-pose steering of the wingsuit;
//! - [`parachute`]: the parachute's handles, taken with the grip and pulled to steer and brake;
//! - [`driver_aim`]: the driver's vehicle weapons aimed by the dominant hand, as an option;
//! - [`look_steer`]: the wingsuit's head look-steer removed.
//!
//! The stick bindings of every class are the tables' ([`crate::controllers::bindings::tables`]).
//! This module runs on the input tick in two halves around them, like the weapon handling:
//!
//! 1. [`begin_tick`] reads the mounted gun, fits the wheel and the handlebars to the vehicle, and
//!    places the physical controls' and the parachute handles' grab volumes, and returns the volume
//!    each hand is in. A hand holding a control or a handle is always in it, so that a press made
//!    while it still holds (a release and a press between two ticks) takes the control again
//!    rather than firing a weapon or the grapple.
//!
//! A control or a handle is held only while its hand's grip is: releasing the grip lets go.
//! 2. [`finish_tick`] acts on the tick's grabs, moves the held controls, replaces the tables'
//!    analog writes for each axis a control drives, and publishes the wheel angle for the rendered
//!    wheel; in the wingsuit it adds the arm pose to the move stick, and under the parachute the
//!    held handles.
//!
//! The hands take the anchored poses in [`apply`], which every hand subsystem's hands pass through
//! ([`crate::controllers::hands::active_hands`]). Everything acts only while the bindings inject,
//! which is only under the controller scheme with a controller active.
//!
//! Turrets are left to the game: a turret turns toward its weapon's aim point at its own limited
//! rate, so a turret aimed by hand trails the hand exactly as it trails the gaze. The motorcycle's
//! and jetski's personal weapon is a wielded weapon on the hand's attach bone, so the per-hand gun
//! aim already aims it by hand ([`crate::controllers::aim::gun`]).
//!
//! [`ControllersConfig::vehicles`]: crate::controllers::config::ControllersConfig::vehicles

pub mod arm_steering;
pub mod config;
pub mod driver_aim;
pub mod driving_view;
pub mod fit;
mod highlights;
pub mod look_steer;
pub mod mounted;
pub mod parachute;
pub mod physical;
pub mod steering;
pub mod wheel;
pub mod wheel_render;

use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use glam::{Quat, Vec2, Vec3};
use jc3gi::{
    character::character::Character,
    input::input_action_map::Action::{self, *},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::{
        bindings::{
            controls::Controls,
            grab::{self, GrabVolume},
            hold::{Desired, MapKind, Target, Want},
        },
        body::{self, BodyDimensions},
        config::ControllersConfig,
        frame::{Hand, Pose},
        hands::{self, ActiveHands, CockpitHand, tiers},
        mode::{AirborneVariant, Mode, VehicleSeat},
        overlay::{primitive::Source, store},
        vehicles::{
            arm_steering::{self as arm, ArmPose},
            config::{ParachuteHandlesConfig, PhysicalControlsConfig},
            fit::{FitInputs, FitReadout, VehicleLatch},
            mounted::{MountedGun, MountedTarget, Reference},
            parachute::{HandleEvent, HandleHold, HandleState},
            physical::{ControlEvent, ControlKind, ControlState, Hold, Outputs, Placement},
            wheel::{CockpitTransform, WheelFrame},
            wheel_render::RenderWheel,
        },
    },
    headpose,
};

/// The vehicles' hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(driver_aim::hook_library())
        .with_hook_library(look_steer::hook_library())
        .with_hook_library(wheel_render::hook_library())
}

/// The mounted gun as the last input tick saw it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MountedReadout {
    pub gun: MountedGun,
    /// The barrel direction the hands ask for, in the world.
    pub direction: Option<Vec3>,
    /// The aim point handed to the gun.
    pub target: Option<Vec3>,
}

/// The arm steering as the last input tick saw it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmReadout {
    pub pose: ArmPose,
    /// The spread, as a share of the arm span, the arms need this tick to steer: the engaging
    /// spread, or the narrower one while they already steer.
    pub needed_spread: f32,
    /// The shoulders' height in the cockpit frame, which the pose's height is measured from.
    pub shoulder_y: f32,
    /// The arms' own stick vector, `None` while they are not spread.
    pub arms: Option<Vec2>,
    /// The stick vector written to the move half-axes: the stick and the arms together.
    pub written: Vec2,
}

/// The parachute handles as the last input tick saw them.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandlesReadout {
    /// Whether each hand holds its handle, indexed by [`Hand::index`].
    pub held: [bool; 2],
    /// Each hand's pull, `0..=1`.
    pub pulls: [f32; 2],
    /// The handles' own stick vector, `None` while neither is held.
    pub handles: Option<Vec2>,
    /// The stick vector written to the move half-axes: the stick and the handles together, `None`
    /// while the tables' stick writes stand.
    pub written: Option<Vec2>,
}

/// The vehicles' readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VehiclesStatus {
    /// Whether the last input tick ran the vehicle handling.
    pub active: bool,
    /// The seat, while in a vehicle.
    pub seat: Option<VehicleSeat>,
    /// The physical controls the seat offers with their toggles on.
    pub offered: &'static [ControlKind],
    /// The grab volume each hand is in, indexed by [`Hand::index`].
    pub volumes: [Option<GrabVolume>; 2],
    /// The control each hand holds.
    pub holds: [Option<ControlKind>; 2],
    /// The controls' moving parts.
    pub controls: ControlState,
    /// The values the held controls wrote on the last tick.
    pub outputs: Outputs,
    /// The most recent grabs and releases, newest last.
    pub events: Vec<ControlEvent>,
    pub mounted: Option<MountedReadout>,
    pub arms: Option<ArmReadout>,
    /// The parachute handles, while they act.
    pub handles: Option<HandlesReadout>,
    /// What the wheel and handlebars fit found, while driving.
    pub fit: Option<FitReadout>,
    /// The wheel as placed, in the cockpit frame.
    pub wheel: Option<WheelFrame>,
    /// The angle the rendered wheel is turned to, radians, while it is turned.
    pub rendered_angle: Option<f32>,
}

pub fn status() -> VehiclesStatus {
    STATE.lock().status.clone()
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

/// The first half of the vehicle handling's input tick: read the mounted gun, place the physical
/// controls, and return the control each hand is in. Called while the bindings inject, before the
/// tables.
pub(crate) fn begin_tick(mode: Mode) -> [Option<GrabVolume>; 2] {
    let Some(active) = hands::active_hands_raw() else {
        stand_down();
        return [None; 2];
    };
    let config = active.config;
    let seat = match mode {
        Mode::Vehicle(seat) => Some(seat),
        _ => None,
    };
    let physical_config = config.vehicles.physical;
    let xf = CockpitTransform {
        head_world: headpose::query().position,
        head_cockpit: active.head.position,
        body: headpose::xr::body_rotation(),
        world_scale: active.world_scale,
    };
    let mut latch = STATE.lock().latch.take();
    let fit = fit::fit(
        &FitInputs {
            physical: &physical_config,
            view: &config.vehicles.driving_view,
            seat,
            xf: &xf,
            anchor: headpose::anchor(),
        },
        &mut latch,
    );
    driving_view::publish(fit.view);
    let offered = offered(seat, &config, fit.fork);
    let placement = Placement {
        wheel: fit.wheel,
        bars: fit.bars,
        ..Placement::new(physical_config, config.dominant_hand)
    };
    let grips = active.hands.map(|h| h.map(|h| h.grip.position));
    let mounted = mounted_tick(&active);
    let handles_on = handles_act(mode, &config);
    let handles_config = config.vehicles.parachute_handles;

    let mut events = Vec::new();
    let mut handle_events = Vec::new();
    let mut state = STATE.lock();
    if state.status.seat != seat {
        physical::release_all(&mut state.controls, &mut events);
        state.controls = ControlState::default();
    }
    if !handles_on {
        parachute::release_all(&mut state.handles, &mut handle_events);
    }
    let mut candidates = physical::candidates(&placement, &state.controls, offered, grips);
    if handles_on {
        let dims = body_dimensions(&config);
        let shoulders = body::shoulder_positions(active.head.position, Quat::IDENTITY, &dims);
        let centres = parachute::handle_centres(&handles_config, shoulders);
        candidates.extend(parachute::candidates(
            &handles_config,
            &state.handles,
            centres,
            grips,
        ));
    }
    let volumes = Hand::BOTH.map(|hand| {
        let i = hand.index();
        if let Some(hold) = state.controls.holds[i] {
            return Some(GrabVolume::VehicleControl(hold.kind));
        }
        if state.handles.holds[i].is_some() {
            return Some(GrabVolume::ParachuteHandle(hand));
        }
        grab::grab_volume(grips[i]?, hand, &candidates)
    });
    state.handles_on = handles_on;
    state.handles_config = handles_config;
    state.placement = Some(placement);
    state.latch = latch;
    state.render = fit.render.filter(|_| physical_config.animate_wheel);
    state.steering_turn = fit.steering_turn;
    state.status.fit = fit.readout;
    state.status.wheel = seat.is_some_and(|s| s.driver).then_some(fit.wheel);
    state.mounted_target = mounted.target;
    MOUNTED_WEAPON.store(mounted.target.map_or(0, |t| t.weapon), Ordering::Relaxed);
    state.mounted_handles = mounted.handles.or(state.mounted_handles);
    state.status.active = true;
    state.status.seat = seat;
    state.status.offered = offered;
    state.status.volumes = volumes;
    state.status.mounted = mounted.readout;
    let arms_engaged = state.status.arms.is_some_and(|a| a.arms.is_some());
    if !handles_on {
        state.status.handles = None;
    }
    push_events(&mut state.status, events);
    drop(state);
    log_handle_events(&handle_events);

    let shown = (physical_config.show_wheel && seat.is_some_and(|s| s.driver)).then_some(fit.wheel);
    let wings = (arms_act(mode, &config) && config.vehicles.arm_steering.show_guide)
        .then_some(arms_engaged);
    highlights::submit(&config, &candidates, volumes, wings, shown, &xf);
    volumes
}

/// The second half of the vehicle handling's input tick: act on the grabs, move the held controls,
/// write their axes over the tables', add the arm pose to the move stick in the wingsuit, and steer
/// with the held handles under the parachute.
pub(crate) fn finish_tick(
    mode: Mode,
    grabs: [Option<GrabVolume>; 2],
    controls: &Controls,
    desired: &mut Vec<Desired>,
    dt: f32,
) {
    let Some(active) = hands::active_hands_raw() else {
        return;
    };
    let config = active.config;
    let grips = active.hands.map(|h| h.map(|h| h.grip.position));
    let orientations = active.hands.map(|h| h.map(|h| h.grip.orientation));
    let held = Hand::BOTH.map(|hand| controls.hand(hand).grip.held());
    let handle_grabs = grabs.map(|g| matches!(g, Some(GrabVolume::ParachuteHandle(_))));
    let grabs = grabs.map(|g| match g {
        Some(GrabVolume::VehicleControl(kind)) => Some(kind),
        _ => None,
    });

    let mut events = Vec::new();
    let mut handle_events = Vec::new();
    let mut state = STATE.lock();
    let Some(placement) = state.placement else {
        return;
    };
    let offered = state.status.offered;
    let outputs = physical::step(
        &mut state.controls,
        &placement,
        &physical::StepInputs {
            kinds: offered,
            grips,
            orientations,
            held,
            grabs,
            dt,
        },
        &mut events,
    );
    write_outputs(&outputs, desired);
    let rendered = state.render.map(|target| {
        let angle = rendered_angle(&state.controls, &placement.config, state.steering_turn);
        (target, angle)
    });
    wheel_render::publish(rendered);
    state.status.rendered_angle = rendered.map(|(_, angle)| angle);
    let handles_config = state.handles_config;
    let pulls = state.handles_on.then(|| {
        parachute::step(
            &mut state.handles,
            &handles_config,
            &parachute::StepInputs {
                grips,
                held,
                grabs: handle_grabs,
            },
            &mut handle_events,
        )
    });
    let blend = config.hands.anchor.blend_s;
    for hand in Hand::BOTH {
        let i = hand.index();
        let hold = state.controls.holds[i];
        let handle = state.handles.holds[i];
        if hold.is_some() {
            state.last_holds[i] = hold;
            state.last_handles[i] = None;
        } else if handle.is_some() {
            state.last_handles[i] = handle;
            state.last_holds[i] = None;
        }
        let target = if hold.is_some() || handle.is_some() {
            1.0
        } else {
            0.0
        };
        state.anchor_weights[i] =
            tiers::step_weight(state.anchor_weights[i], target, dt, blend, blend);
        if state.anchor_weights[i] <= 0.0 {
            state.last_holds[i] = None;
            state.last_handles[i] = None;
        }
    }
    let engaged = state.status.arms.is_some_and(|a| a.arms.is_some());
    let arms = arm_tick(mode, controls, &active, desired, engaged);
    if arms.is_some_and(|a| a.arms.is_some()) != engaged {
        tracing::debug!(
            target: "controllers",
            engaged = !engaged,
            pose = ?arms.map(|a| a.pose),
            "wingsuit arm steering"
        );
    }
    let handles =
        pulls.map(|pulls| handles_tick(&state.handles, pulls, controls, desired, &handles_config));
    state.status.holds = state.controls.holds.map(|h| h.map(|h| h.kind));
    state.status.controls = state.controls;
    state.status.outputs = outputs;
    state.status.arms = arms;
    state.status.handles = handles;
    push_events(&mut state.status, events);
    drop(state);
    log_handle_events(&handle_events);
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
    state.last_holds = [None; 2];
    state.last_handles = [None; 2];
    state.anchor_weights = [0.0; 2];
    state.mounted_reference = None;
    state.mounted_target = None;
    MOUNTED_WEAPON.store(0, Ordering::Relaxed);
    state.mounted_handles = None;
    state.placement = None;
    state.latch = None;
    driving_view::publish(None);
    state.render = None;
    state.steering_turn = None;
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
    let state = STATE.lock();
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
            .map(|point| (point, tier_status.anchor_weights[i]))
            .filter(|(_, weight)| *weight > 0.0);
        if let Some((point, weight)) = control.or(handle).or(mounted) {
            hands[i] = Some(move_grip(h, point, weight));
        }
    }
}

/// How many grabs and releases the readout keeps.
const MAX_EVENTS: usize = 6;

struct State {
    status: VehiclesStatus,
    controls: ControlState,
    placement: Option<Placement>,
    /// The control each hand held last, kept while its anchor blends out.
    last_holds: [Option<Hold>; 2],
    /// The handle each hand held last, kept while its anchor blends out.
    last_handles: [Option<HandleHold>; 2],
    /// Each hand's share of the pose on its control or handle.
    anchor_weights: [f32; 2],
    handles: HandleState,
    /// Whether the parachute handles act this tick: under the parachute, with them switched on.
    handles_on: bool,
    /// The handles' settings as the last tick read them, kept while a hand blends off its handle.
    handles_config: ParachuteHandlesConfig,
    mounted_reference: Option<Reference>,
    mounted_target: Option<MountedTarget>,
    /// The mounted gun's handles in the cockpit frame, kept while the hands blend off them.
    mounted_handles: Option<[Option<Vec3>; 2]>,
    /// What the fit keeps for the driven vehicle.
    latch: Option<VehicleLatch>,
    /// The vehicle's wheel part the rendered wheel turns, while the animation is on.
    render: Option<RenderWheel>,
    /// The driven vehicle's own steering, as the last fit read it.
    steering_turn: Option<f32>,
}

static STATE: Mutex<State> = Mutex::new(State {
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
    },
    controls: ControlState::new(),
    placement: None,
    last_holds: [None; 2],
    last_handles: [None; 2],
    anchor_weights: [0.0; 2],
    handles: HandleState::new(),
    handles_on: false,
    handles_config: ParachuteHandlesConfig::new(),
    mounted_reference: None,
    mounted_target: None,
    mounted_handles: None,
    latch: None,
    render: None,
    steering_turn: None,
});

/// The address of the weapon in [`State::mounted_target`], or zero: a lock-free copy for
/// [`is_mounted_weapon`].
static MOUNTED_WEAPON: AtomicUsize = AtomicUsize::new(0);

/// The physical controls `seat` offers with their toggles on: only the driver's seat has any. A
/// vehicle with a steering fork (`fork`) offers handlebars.
fn offered(
    seat: Option<VehicleSeat>,
    config: &ControllersConfig,
    fork: bool,
) -> &'static [ControlKind] {
    let physical = &config.vehicles.physical;
    let Some(seat) = seat.filter(|s| s.driver && physical.any()) else {
        return &[];
    };
    let kinds = ControlKind::for_seat(seat.class, fork);
    match kinds {
        [one] if one.enabled(physical) => kinds,
        [first, second] => match (first.enabled(physical), second.enabled(physical)) {
            (true, true) => kinds,
            (true, false) => &kinds[..1],
            (false, true) => &kinds[1..],
            (false, false) => &[],
        },
        _ => &[],
    }
}

/// What the mounted-gun half of the input tick produced.
struct MountedTick {
    readout: Option<MountedReadout>,
    target: Option<MountedTarget>,
    handles: Option<[Option<Vec3>; 2]>,
}

/// Read the mounted gun and derive its aim from the hands.
fn mounted_tick(active: &ActiveHands) -> MountedTick {
    let config = &active.config;
    let none = MountedTick {
        readout: None,
        target: None,
        handles: None,
    };
    if !(config.vehicles.mounted.aim_by_hands || config.vehicles.mounted.anchor_hands) {
        STATE.lock().mounted_reference = None;
        return none;
    }
    // SAFETY: the input tick runs on the main thread outside the world update.
    let Some(gun) = (unsafe {
        Character::GetLocalPlayerCharacter()
            .as_ref()
            .and_then(|owner| mounted::read(owner))
    }) else {
        STATE.lock().mounted_reference = None;
        return none;
    };
    let head = headpose::query();
    let body = headpose::xr::body_rotation();
    let to_cockpit = |world: Vec3| {
        active.head.position
            + body.inverse() * (world - head.position) / active.world_scale.max(1e-3)
    };
    let handles = gun.handles.map(|h| h.map(to_cockpit));
    let grips = active.hands.map(|h| h.map(|h| h.grip.position));

    let aim = config.vehicles.mounted.aim_by_hands;
    let direction = mounted::hands_midpoint(grips)
        .filter(|_| aim)
        .and_then(|(mid, tracked)| {
            let mut state = STATE.lock();
            let current = state
                .mounted_reference
                .filter(|r| r.weapon == gun.address && r.hands == tracked);
            let reference = current.or_else(|| {
                let handles_mid = match handles {
                    [Some(l), Some(r)] => Some((l + r) * 0.5),
                    [one, other] => one.or(other),
                };
                mounted::take_reference(
                    gun.address,
                    tracked,
                    mid,
                    to_cockpit(gun.pivot),
                    handles_mid,
                    body.inverse() * gun.barrel,
                    config.vehicles.mounted.min_lever_m,
                )
            });
            state.mounted_reference = reference;
            drop(state);
            mounted::aim_direction(&reference?, mid).map(|d| body * d)
        });
    let target = direction.map(|d| MountedTarget {
        weapon: gun.address,
        point: gun.pivot + d * config.aim.max_distance_m.max(1.0),
        at: Instant::now(),
    });
    MountedTick {
        readout: Some(MountedReadout {
            gun,
            direction,
            target: target.map(|t| t.point),
        }),
        target,
        handles: config.vehicles.mounted.anchor_hands.then_some(handles),
    }
}

/// Whether the arm steering acts in `mode`: in the wingsuit, with it switched on.
fn arms_act(mode: Mode, config: &ControllersConfig) -> bool {
    config.vehicles.arm_steering.wingsuit && mode == Mode::Airborne(AirborneVariant::Wingsuit)
}

/// Add the arm pose to the move stick in the wingsuit, when its arm steering is on. `engaged` is
/// whether the arms steered on the last tick.
fn arm_tick(
    mode: Mode,
    controls: &Controls,
    active: &ActiveHands,
    desired: &mut Vec<Desired>,
    engaged: bool,
) -> Option<ArmReadout> {
    let config = &active.config.vehicles.arm_steering;
    if !arms_act(mode, &active.config) {
        return None;
    }
    let left = active.hands[Hand::Left.index()]?.grip.position;
    let right = active.hands[Hand::Right.index()]?.grip.position;
    let dims = body_dimensions(&active.config);
    let shoulder_y = active.head.position.y - (dims.eye_height - dims.shoulder_height);
    let pose = arm_steering::arm_pose(left, right, shoulder_y, dims.arm_span);
    let arms = arm::steer(&pose, config, engaged);
    let written = arm::combine(controls.off().stick, arms);
    write_move(desired, written);
    Some(ArmReadout {
        pose,
        needed_spread: arm::needed_spread(config, engaged),
        shoulder_y,
        arms,
        written,
    })
}

/// Whether the parachute handles act in `mode`.
fn handles_act(mode: Mode, config: &ControllersConfig) -> bool {
    config.vehicles.parachute_handles.enabled && mode == Mode::Airborne(AirborneVariant::Parachute)
}

/// Steer with the held handles under the parachute: their vector added to the move stick, written
/// to the move half-axes the parachute task reads, as the stick's. With neither handle held, the
/// tables' stick writes stand.
fn handles_tick(
    state: &HandleState,
    pulls: [f32; 2],
    controls: &Controls,
    desired: &mut Vec<Desired>,
    config: &ParachuteHandlesConfig,
) -> HandlesReadout {
    let held = state.holds.map(|h| h.is_some());
    let handles = held
        .contains(&true)
        .then(|| parachute::steer(pulls, config));
    let written = handles.map(|h| arm::combine(controls.off().stick, Some(h)));
    if let Some(written) = written {
        write_move(desired, written);
    }
    HandlesReadout {
        held,
        pulls,
        handles,
        written,
    }
}

/// Write `stick` (+X right, +Y forward) over the tables' move half-axes and their length.
fn write_move(desired: &mut Vec<Desired>, stick: Vec2) {
    replace_axis(desired, stick.y, MOVE_BACKWARD, MOVE_FORWARD);
    replace_axis(desired, stick.x, MOVE_LEFT, MOVE_RIGHT);
    replace_axis(desired, stick.length().min(1.0), MOVE_ALL, MOVE_ALL);
}

fn log_handle_events(events: &[HandleEvent]) {
    for event in events {
        tracing::debug!(target: "controllers", ?event, "parachute handle");
    }
}

/// The dimensions the hands are measured against: the character's while the hands are remapped
/// onto them, the player's otherwise.
fn body_dimensions(config: &ControllersConfig) -> BodyDimensions {
    if config.body.remap_hands {
        BodyDimensions::CHARACTER_ESTIMATE
    } else {
        body::player()
    }
}

/// Write the held controls' axes over the tables' writes for the same actions.
pub fn write_outputs(out: &Outputs, desired: &mut Vec<Desired>) {
    if let Some(steer) = out.steer {
        replace_axis(desired, steer, TURN_LEFT, TURN_RIGHT);
    }
    if let Some(lean) = out.lean {
        replace_axis(desired, lean, BIKE_LEAN_LEFT, BIKE_LEAN_RIGHT);
    }
    if let Some(cyclic) = out.cyclic {
        replace_axis(desired, cyclic.x, HELI_ROLL_LEFT, HELI_ROLL_RIGHT);
        replace_axis(desired, cyclic.y, HELI_BACKWARD, HELI_FORWARD);
    }
    if let Some(collective) = out.collective {
        replace_axis(desired, collective, HELI_DEC_ALTITUDE, HELI_INC_ALTITUDE);
    }
    if let Some(stick) = out.flight_stick {
        replace_axis(desired, stick.x, PLANE_ROLL_LEFT, PLANE_ROLL_RIGHT);
        // Pushing the stick forward pitches the nose down.
        replace_axis(desired, stick.y, PLANE_PITCH_UP, PLANE_PITCH_DOWN);
    }
    if let Some(throttle) = out.throttle {
        replace_axis(desired, throttle, PLANE_DEC_TRUST, PLANE_INC_TRUST);
    }
}

/// Replace every player-map want on `negative` and `positive` with the signed `value` split into
/// the two non-negative half-axes, as the tables write an axis.
fn replace_axis(desired: &mut Vec<Desired>, value: f32, negative: Action, positive: Action) {
    desired.retain(|d| {
        !(d.target.map == MapKind::Player
            && (d.target.action == negative || d.target.action == positive))
    });
    let value = if value.is_finite() { value } else { 0.0 };
    for (action, half) in [(negative, -value), (positive, value)] {
        if half > 0.0 {
            desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action,
                },
                want: Want::Analog(half.min(1.0)),
            });
        }
    }
}

/// `hand` with its grip moved toward `point` by `weight`, keeping its rotation.
fn move_grip(hand: CockpitHand, point: Vec3, weight: f32) -> CockpitHand {
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

fn push_events(status: &mut VehiclesStatus, events: Vec<ControlEvent>) {
    for event in &events {
        tracing::debug!(target: "controllers", ?event, "vehicle control");
    }
    status.events.extend(events);
    let excess = status.events.len().saturating_sub(MAX_EVENTS);
    status.events.drain(..excess);
}

/// The angle to turn the rendered wheel to, radians clockwise: the held or returning wheel's, or
/// otherwise the wheel angle that would give the vehicle's own steering, so the stick turns it too.
fn rendered_angle(
    controls: &ControlState,
    config: &PhysicalControlsConfig,
    steering_turn: Option<f32>,
) -> f32 {
    let held = controls
        .holds
        .iter()
        .flatten()
        .any(|h| h.kind == ControlKind::Wheel);
    if held || controls.wheel_angle != 0.0 {
        return controls.wheel_angle;
    }
    steering_turn.map_or(0.0, |turn| {
        steering::angle_for_steer(turn.clamp(-1.0, 1.0), &config.wheel)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::mode::VehicleClass;

    fn analog(desired: &[Desired], action: Action) -> Option<f32> {
        desired.iter().find_map(|d| match d.want {
            Want::Analog(v) if d.target.action == action => Some(v),
            _ => None,
        })
    }

    fn want(action: Action, value: f32) -> Desired {
        Desired {
            target: Target {
                map: MapKind::Player,
                action,
            },
            want: Want::Analog(value),
        }
    }

    #[test]
    fn a_held_control_replaces_the_stick_s_axis_and_leaves_the_rest() {
        let mut desired = vec![
            want(TURN_LEFT, 0.7),
            want(ACCELERATE, 1.0),
            want(PLANE_ROLL_LEFT, 0.7),
        ];
        let out = Outputs {
            steer: Some(0.4),
            ..Outputs::default()
        };
        write_outputs(&out, &mut desired);
        assert_eq!(analog(&desired, TURN_LEFT), None);
        assert_eq!(analog(&desired, TURN_RIGHT), Some(0.4));
        assert_eq!(analog(&desired, ACCELERATE), Some(1.0));
        assert_eq!(analog(&desired, PLANE_ROLL_LEFT), Some(0.7));
    }

    #[test]
    fn the_flight_stick_forward_pitches_down_and_the_collective_splits_its_halves() {
        let mut desired = Vec::new();
        let out = Outputs {
            flight_stick: Some(Vec2::new(0.0, 0.5)),
            collective: Some(-0.25),
            throttle: Some(0.0),
            ..Outputs::default()
        };
        write_outputs(&out, &mut desired);
        assert_eq!(analog(&desired, PLANE_PITCH_DOWN), Some(0.5));
        assert_eq!(analog(&desired, PLANE_PITCH_UP), None);
        assert_eq!(analog(&desired, HELI_DEC_ALTITUDE), Some(0.25));
        assert_eq!(analog(&desired, HELI_INC_ALTITUDE), None);
        assert_eq!(analog(&desired, PLANE_INC_TRUST), None);
    }

    #[test]
    fn only_the_driver_is_offered_the_enabled_controls() {
        // The defaults enable every physical control, so disable them all here and re-enable
        // only the one this test means to exercise.
        let mut config = ControllersConfig::new();
        config.vehicles.physical.wheel.enabled = false;
        config.vehicles.physical.handlebars.enabled = false;
        config.vehicles.physical.cyclic.enabled = false;
        config.vehicles.physical.collective.enabled = false;
        config.vehicles.physical.flight_stick.enabled = false;
        config.vehicles.physical.throttle.enabled = false;
        let seat = |class, driver| Some(VehicleSeat { class, driver });
        assert!(offered(seat(VehicleClass::Car, true), &config, false).is_empty());
        config.vehicles.physical.collective.enabled = true;
        assert_eq!(
            offered(seat(VehicleClass::Helicopter, true), &config, false),
            &[ControlKind::Collective]
        );
        assert!(offered(seat(VehicleClass::Helicopter, false), &config, false).is_empty());
        assert!(offered(None, &config, false).is_empty());
        config.vehicles.physical.handlebars.enabled = true;
        assert_eq!(
            offered(seat(VehicleClass::Sea, true), &config, true),
            &[ControlKind::Handlebars]
        );
    }

    #[test]
    fn the_rendered_wheel_follows_the_held_wheel_and_otherwise_the_steering() {
        let config = PhysicalControlsConfig::new();
        let lock = config.wheel.lock_deg.to_radians();
        let mut controls = ControlState::new();
        let half = rendered_angle(&controls, &config, Some(0.5));
        assert!((steering::steer_for_angle(half, &config.wheel) - 0.5).abs() < 1e-4);
        assert!((rendered_angle(&controls, &config, Some(3.0)) - lock).abs() < 1e-5);
        assert_eq!(rendered_angle(&controls, &config, None), 0.0);
        // A released wheel still on its way back is drawn where it is.
        controls.wheel_angle = 0.4;
        assert_eq!(rendered_angle(&controls, &config, Some(0.0)), 0.4);
        controls.holds[1] = Some(Hold {
            kind: ControlKind::Wheel,
            offset: Vec3::ZERO,
        });
        assert_eq!(rendered_angle(&controls, &config, Some(0.5)), 0.4);
    }

    #[test]
    fn a_grip_moves_onto_its_control_by_the_weight() {
        let pose = Pose {
            position: Vec3::ZERO,
            orientation: Quat::from_rotation_y(0.3),
        };
        let hand = CockpitHand {
            wrist: pose,
            grip: pose,
            aim: pose,
        };
        let moved = move_grip(hand, Vec3::X, 0.5);
        assert_eq!(moved.grip.position, Vec3::new(0.5, 0.0, 0.0));
        assert_eq!(moved.wrist.orientation, pose.orientation);
    }
}
