//! The controller-to-game bindings: on every engine input tick, map the latest controller frame
//! and its edges to the game's actions for the current mode, and inject them.
//!
//! - [`controls`] reduces the frame to logical controls (deadzones, edges, grab volumes);
//! - [`tables`] maps them to the wanted game actions per mode, following the plan of record;
//! - [`hold`] turns the wanted actions into effector writes with the game's hold timing;
//! - [`grab`] is the grab-volume query the grip bindings consult, over the volumes the weapon
//!   handling places ([`crate::controllers::weapons`]);
//! - [`yank`] detects the yank gesture on the grapple hand, which the tables turn into a grapple
//!   tap;
//! - [`inject`] applies the writes to the player and UI action maps.
//!
//! [`input_tick`] runs from the `InputDeviceManager::Update` post-call detour
//! ([`crate::hooks::input::look`]), after the device poll and before the game reads its actions.
//! It injects only while the bindings are enabled, the controller scheme is in effect, a controller
//! is active, and the mod's panel is down; otherwise it writes nothing, so a real gamepad under the
//! gamepad scheme is never fought. The right stick's body turn is not an action: it is returned to
//! the look detour, which feeds it to the headpose's VR turn scheme.

pub mod controls;
pub mod grab;
pub mod hold;
pub mod inject;
pub mod tables;
pub mod yank;

use jc3gi::input::input_action_map::Action;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        bindings::{
            controls::{Controls, yaw_of},
            hold::{HoldTracker, MapKind, Phase},
            inject::Outcome,
            tables::Bindings,
            yank::{Yank, YankConfig, YankDetector},
        },
        config::LocomotionDirection,
        frame::{self, ControllerFrame, Hand},
        hands::{self, tiers},
        mode::{self, Detection, Mode, Traversal},
        scheme, vehicles,
        weapons::{self, dual_wield::TriggerInput, haptics},
    },
    headpose,
};

/// What the input tick hands back to the look detour.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct InputTickOutput {
    /// The controllers' body-turn input for the VR turn scheme, `-1..=1`, positive turning right,
    /// or `None` when the controllers are not injecting.
    pub body_turn: Option<f32>,
}

/// Why the bindings did not inject on a tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Idle {
    /// The bindings toggle is off.
    Disabled,
    /// The gamepad scheme is in effect.
    GamepadScheme,
    /// No controller frame, or no active controller.
    NoControllers,
    /// The mod's panel is up.
    ModPanel,
}

/// One injected write, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReportedWrite {
    pub map: MapKind,
    pub action: Action,
    pub value: f32,
    pub phase: Phase,
    pub outcome: Outcome,
}

/// The last input tick, for the debug UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TickReport {
    pub detection: Detection,
    /// Why nothing was injected, when nothing was.
    pub idle: Option<Idle>,
    pub body_turn: f32,
    pub writes: Vec<ReportedWrite>,
}

/// Run the controllers' part of the input tick: detect the mode (which selects the action set),
/// then evaluate and inject the bindings. `dt` is the engine's tick delta, seconds.
pub(crate) fn input_tick(dt: f32) -> InputTickOutput {
    let config = Config::lock_query(|c| c.controllers);
    if !config.enabled {
        STATE.lock().reset();
        weapons::stand_down();
        vehicles::stand_down();
        return InputTickOutput::default();
    }
    let detection = mode::tick();
    tiers::tick(dt);
    // SAFETY: the input tick runs on the main thread outside the world update.
    unsafe { hands::anchor::input_tick(&config) };

    let frame = frame::latest().filter(ControllerFrame::any_active);
    let idle = if !config.bindings.enabled {
        Some(Idle::Disabled)
    } else if !scheme::is_controllers() {
        Some(Idle::GamepadScheme)
    } else if frame.is_none() {
        Some(Idle::NoControllers)
    } else if mode::last_snapshot().is_some_and(|s| s.ui.mod_panel) {
        Some(Idle::ModPanel)
    } else {
        None
    };
    // The edges are taken even while idle, so that a press from before injection resumed is not
    // replayed then.
    let edges = frame::take_edges();

    let mut state = STATE.lock();
    let (Some(frame), None) = (frame, idle) else {
        state.reset();
        weapons::stand_down();
        vehicles::stand_down();
        state.report = TickReport {
            detection,
            idle,
            ..Default::default()
        };
        return InputTickOutput::default();
    };

    let dominant = config.dominant_hand;
    let move_rotation = match config.locomotion.direction {
        LocomotionDirection::Head => 0.0,
        LocomotionDirection::OffHand => off_hand_rotation(&frame, dominant.other()),
    };
    let start = weapons::begin_tick(detection.mode);
    let vehicle_volumes = vehicles::begin_tick(detection.mode);
    let volumes = Hand::BOTH.map(|hand| {
        let i = hand.index();
        start.volumes[i].or(vehicle_volumes[i])
    });
    let mut controls = Controls::from_frame(
        &frame,
        &edges,
        &config.bindings,
        dominant,
        move_rotation,
        volumes,
    );
    controls.weapon_cycle =
        !config.weapons.holsters.enabled || config.weapons.holsters.stick_click_cycles;
    controls.dual_wield = start.dual_wield;
    weapons::dual_wield::observe_triggers(
        start.dual_wield,
        controls.hands.map(|h| TriggerInput {
            held: h.trigger_press.held(),
            pressed: h.trigger_press.pressed,
        }),
    );
    let grapple_hand = dominant.other();
    let yank = state.yank.step(
        dt,
        grapple_hand_distance(grapple_hand, &config.bindings.yank),
        yank_armed(&detection),
        &config.bindings.yank,
    );
    if let Some(yank) = yank {
        controls.yank = true;
        tracing::debug!(target: "controllers", ?yank, "yank");
        if config.weapons.haptics.grapple {
            haptics::pulse(
                grapple_hand,
                config.weapons.haptics.yank,
                haptics::PulseSource::Yank,
            );
        }
    }
    let mut step = state
        .bindings
        .step(detection.mode, &controls, dt, &config.bindings);
    if step.crouch_toggle {
        crate::controllers::roomscale::toggle_seated_crouch();
    }
    weapons::finish_tick(step.grabs, &mut step.desired, dt);
    vehicles::finish_tick(detection.mode, step.grabs, &controls, &mut step.desired, dt);
    let writes = state.tracker.resolve(&step.desired, dt);
    // SAFETY: the input tick runs on the main thread after the device poll.
    let outcomes = unsafe { inject::apply(&writes) };
    state.report = TickReport {
        detection,
        idle: None,
        body_turn: step.body_turn,
        writes: writes
            .iter()
            .zip(outcomes)
            .map(|(w, outcome)| ReportedWrite {
                map: w.target.map,
                action: w.target.action,
                value: w.value,
                phase: w.phase,
                outcome,
            })
            .collect(),
    };
    InputTickOutput {
        body_turn: Some(step.body_turn),
    }
}

/// The last input tick's report.
pub fn last_report() -> TickReport {
    STATE.lock().report.clone()
}

/// The yank gesture's readout: how many yanks were detected, and the last one.
pub fn yank_status() -> (u64, Option<Yank>) {
    let state = STATE.lock();
    (state.yank.count, state.yank.last)
}

static STATE: Mutex<State> = Mutex::new(State {
    bindings: Bindings::new(),
    tracker: HoldTracker::new(),
    yank: YankDetector::new(),
    report: TickReport {
        detection: Detection {
            mode: mode::Mode::OnFoot(mode::OnFootVariant::Ground),
            traversal: None,
            held: None,
        },
        idle: None,
        body_turn: 0.0,
        writes: Vec::new(),
    },
});

struct State {
    bindings: Bindings,
    tracker: HoldTracker,
    yank: YankDetector,
    report: TickReport,
}

impl State {
    /// Stop injecting: forget every hold and latch without writing. The device poll releases the
    /// held effectors on its own, keeping the last hold time written.
    fn reset(&mut self) {
        self.bindings = Bindings::new();
        self.tracker.clear();
    }
}

/// The angle from the head's heading to the off-hand controller's, both in the cockpit frame, for
/// off-hand-relative locomotion. The game already moves relative to the view, which follows the
/// head, so rotating the stick by the difference makes its forward the controller's. Zero when
/// either heading is unknown.
fn off_hand_rotation(frame: &ControllerFrame, off: Hand) -> f32 {
    let aim = &frame.hand(off).aim;
    let hand = aim.orientation_valid.then(|| yaw_of(aim.pose.orientation));
    let head = headpose::xr::cockpit_pose().map(|pose| yaw_of(pose.orientation));
    match (hand.flatten(), head.flatten()) {
        (Some(hand), Some(head)) => hand - head,
        _ => 0.0,
    }
}

/// The grapple hand's distance from the body point a yank pulls toward, in the cockpit frame, or
/// `None` while the hand is untracked or the hands are inert.
fn grapple_hand_distance(hand: Hand, config: &YankConfig) -> Option<f32> {
    let active = hands::active_hands_raw()?;
    let grip = active.hands[hand.index()]?.grip.position;
    Some(grip.distance(yank::body_point(active.head.position, config)))
}

/// Whether a yank may fire: a grapple line is live, the character is on foot or airborne, and the
/// line is not already reeling the character in.
fn yank_armed(detection: &Detection) -> bool {
    let wire = mode::last_snapshot()
        .and_then(|s| s.character)
        .is_some_and(|c| c.grapple_wire);
    let reeling = matches!(
        detection.traversal,
        Some(Traversal::PreReel | Traversal::ReelingIn)
    );
    wire && !reeling && matches!(detection.mode, Mode::OnFoot(_) | Mode::Airborne(_))
}
