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
//! - `recovery` masks a hand after a loss of its pose or of injection until its controls rest
//!   ([`HandRecovery`]);
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
mod recovery;
pub mod tables;
pub mod yank;

use std::time::Instant;

use jc3gi::input::input_action_map::Action;
use parking_lot::Mutex;

pub use crate::controllers::bindings::recovery::{Blocker, HandRecovery};
use crate::{
    config::Config,
    controllers::{
        aim::sights,
        bindings::{
            controls::{Controls, yaw_of},
            hold::{HoldTracker, MapKind, Phase},
            inject::Outcome,
            recovery::{MaskEvent, MaskLog, NeutralPolicy, Recovery},
            tables::Bindings,
            yank::{Yank, YankConfig, YankDetector},
        },
        config::LocomotionDirection,
        frame::{self, ControllerFrame, Hand},
        hands::{self, tiers},
        mode::{self, Detection, Mode},
        scheme, summary, vehicles,
        weapons::{
            self,
            dual_wield::{self, TriggerInput, TriggerTick},
            haptics,
        },
    },
    headpose,
    hud::pointer::{clear_controller, owns_menu_trigger, update_controller},
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
    let (config, neutral_policy) = Config::lock_query(|c| {
        (
            c.controllers,
            NeutralPolicy::new(
                &c.controllers.input,
                &c.controllers.bindings,
                &c.headpose.vr_turn,
                c.controllers.dominant_hand,
            ),
        )
    });
    if !config.enabled {
        clear_controller();
        let mut state = STATE.lock();
        state.reset();
        state.recovery.suspend();
        state.publish_recovery();
        frame::take_edges();
        weapons::stand_down();
        vehicles::stand_down();
        return InputTickOutput::default();
    }
    let detection = mode::tick();
    tiers::tick(dt);
    // SAFETY: the input tick runs on the main thread outside the world update.
    unsafe { hands::anchor::input_tick(&config) };

    let frame = frame::latest()
        .filter(ControllerFrame::any_active)
        .filter(|frame| frame.focused);
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
    let mut edges = frame::take_edges();

    let mut state = STATE.lock();
    let (Some(mut frame), None) = (frame, idle) else {
        clear_controller();
        state.reset();
        state.recovery.suspend();
        state.publish_recovery();
        weapons::stand_down();
        vehicles::stand_down();
        state.report = TickReport {
            detection,
            idle,
            ..Default::default()
        };
        return InputTickOutput::default();
    };

    let tracking_lost = state.recovery.observe(&frame);
    if tracking_lost {
        clear_controller();
        state.reset();
        weapons::stand_down();
        vehicles::stand_down();
        edges = Default::default();
    }
    // Mode selection precedes the next OpenXR sample. Preserve paused holds while waiting for it.
    if frame.action_set != detection.mode.action_set() {
        clear_controller();
        state.publish_recovery();
        state.report = TickReport {
            detection,
            ..Default::default()
        };
        return InputTickOutput::default();
    }
    state
        .recovery
        .filter(&mut frame, &mut edges, &neutral_policy);
    state.publish_recovery();
    if tracking_lost {
        clear_controller();
    } else {
        update_controller(
            Some(&frame),
            &edges,
            detection.mode == Mode::Ui,
            config.dominant_hand,
            state.recovery.ready(config.dominant_hand),
        );
    }

    let dominant = config.dominant_hand;
    let move_rotation = match config.locomotion.direction {
        LocomotionDirection::Head => 0.0,
        LocomotionDirection::OffHand => off_hand_rotation(&frame, dominant.other()),
    };
    // A game menu or the map pauses the weapon and vehicle handling rather than standing it down,
    // so that what the hands hold (a foregrip, a primed grenade, a physical control) is still held
    // when gameplay resumes.
    let in_menu = detection.mode == Mode::Ui;
    let (start, vehicle_volumes) = if in_menu {
        (weapons::TickStart::default(), [None; 2])
    } else {
        (
            weapons::begin_tick(detection.mode),
            vehicles::begin_tick(detection.mode),
        )
    };
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
    controls.pointer_hand = owns_menu_trigger();
    let pistols = start.dual_wield && dual_wield::triggers_fire_pistols(detection.mode);
    controls.dual_wield = pistols;
    controls.sight_chord = config.aim.sight_tuning.enabled && sights::tuning::lining_up();
    dual_wield::observe_triggers(start.dual_wield.then(|| TriggerTick {
        dominant,
        triggers: controls.hands.map(|h| TriggerInput {
            held: pistols && h.trigger_press.held(),
            pressed: pistols && h.trigger_press.pressed,
        }),
    }));
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
    summary::refresh(detection.mode, &controls, &config, &frame);
    if step.crouch_toggle {
        crate::controllers::roomscale::toggle_seated_crouch();
    }
    if step.sight_capture {
        sights::tuning::request_capture();
    }
    if !in_menu {
        weapons::finish_tick(
            &weapons::FinishInputs {
                grabs: step.grabs,
                grips: controls.hands.map(|h| h.grip),
                detection,
                dt,
            },
            &mut step.desired,
        );
        vehicles::finish_tick(detection.mode, step.grabs, &controls, &mut step.desired, dt);
    }
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

/// Each hand's input recovery at the last input tick, in [`Hand::index`] order. Safe to call with
/// the config locked: it takes only a lock that is never held while taking another.
pub fn recovery_status() -> [HandRecovery; 2] {
    *RECOVERY_STATUS.lock()
}

/// The yank gesture's readout: how many yanks were detected, and the last one.
pub fn yank_status() -> (u64, Option<Yank>) {
    let state = STATE.lock();
    (state.yank.count, state.yank.last)
}

static RECOVERY_STATUS: Mutex<[HandRecovery; 2]> = Mutex::new([HandRecovery::Unusable; 2]);

static STATE: Mutex<State> = Mutex::new(State {
    recovery: Recovery::new(),
    mask_log: MaskLog::new(),
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
    recovery: Recovery,
    mask_log: MaskLog,
    bindings: Bindings,
    tracker: HoldTracker,
    yank: YankDetector,
    report: TickReport,
}

impl State {
    /// Stop injecting: forget every hold and latch without writing. The device poll releases the
    /// held effectors on its own, keeping the last hold time written.
    fn reset(&mut self) {
        self.recovery.disarm();
        self.bindings = Bindings::new();
        self.tracker.clear();
        self.yank = YankDetector::new();
    }

    /// Publish each hand's recovery for the debug UI, and log a hand that keeps waiting for its
    /// controls to rest and its rearm, rate-limited by [`MaskLog`]: a masked hand otherwise looks
    /// like a broken binding.
    fn publish_recovery(&mut self) {
        let status = self.recovery.status();
        *RECOVERY_STATUS.lock() = status;
        let events = self.mask_log.step(status, Instant::now());
        for (hand, event) in Hand::BOTH.into_iter().zip(events) {
            match event {
                Some(MaskEvent::StillMasked { waited, blocker }) => tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    ?waited,
                    ?blocker,
                    "hand input masked until its controls rest"
                ),
                Some(MaskEvent::Rearmed { waited }) => tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    ?waited,
                    "hand input rearmed"
                ),
                None => {}
            }
        }
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
    let tracked = frame::with_latest(|frame| {
        frame.focused && frame.hand(hand).active && frame.hand(hand).grip.is_tracked()
    })?;
    if !tracked {
        return None;
    }
    let active = hands::active_hands_raw()?;
    let grip = active.hands[hand.index()]?.grip.position;
    Some(grip.distance(yank::body_point(active.head.position, config)))
}

/// Whether a yank may fire: a grapple line is live, the character is on foot or airborne, and the
/// line is not already carrying the character. During a reel-in or a hang the grapple hand holds the
/// line and drives the hang swing, and a pull on the line there is a swing, not a yank.
fn yank_armed(detection: &Detection) -> bool {
    let wire = mode::last_snapshot()
        .and_then(|s| s.character)
        .is_some_and(|c| c.grapple_wire);
    yank_armed_in(wire, detection)
}

/// [`yank_armed`] over its inputs.
fn yank_armed_in(wire: bool, detection: &Detection) -> bool {
    wire && detection.traversal.is_none()
        && matches!(detection.mode, Mode::OnFoot(_) | Mode::Airborne(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::mode::{AirborneVariant, OnFootVariant, Traversal};

    #[test]
    fn the_yank_is_armed_only_on_a_free_line() {
        let on_foot = Detection {
            mode: Mode::OnFoot(OnFootVariant::Ground),
            ..Default::default()
        };
        assert!(yank_armed_in(true, &on_foot));
        assert!(!yank_armed_in(false, &on_foot));
        let falling = Detection {
            mode: Mode::Airborne(AirborneVariant::Falling),
            ..Default::default()
        };
        assert!(yank_armed_in(true, &falling));
        for traversal in [
            Traversal::PreReel,
            Traversal::ReelingIn,
            Traversal::PreHang,
            Traversal::ReeledIn,
        ] {
            let carried = Detection {
                traversal: Some(traversal),
                ..on_foot
            };
            assert!(!yank_armed_in(true, &carried), "{traversal:?}");
        }
        assert!(!yank_armed_in(
            true,
            &Detection {
                mode: Mode::Ui,
                ..on_foot
            }
        ));
    }
}
